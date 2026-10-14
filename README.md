# DreamCLI for Rust

A schema-first CLI framework with typed inputs, command groups, and one resolution
pipeline for arguments, stdin, environment variables, configuration, prompts, and
defaults. This is a native Rust port of the sibling `dreamcli` TypeScript project.

The Cargo workspace separates responsibilities:

| Crate               | Responsibility                                                               |
| ------------------- | ---------------------------------------------------------------------------- |
| `dreamcli`          | Public API, command execution, middleware, plugins, help, and native runtime |
| `dreamcli-core`     | Typed input builders, parsing, resolution, provenance, and schema metadata   |
| `dreamcli-output`   | `Write` destinations, capture, tables, colors, and activity guards           |
| `dreamcli-complete` | Bash, Zsh, Fish, and PowerShell completion generation                        |
| `dreamcli-error`    | Shared structured errors and error categories                                |

The core has no process-global reads. Output depends on the shared error crate,
so it can be used without pulling in the parser and resolver. The public crate
re-exports the APIs applications normally need.

## Use from another Rust project

The port is available locally; the crates have not been published.

```toml
[dependencies]
dreamcli = { path = "../dreamcli-rs/crates/dreamcli" }
```

Rust 1.99 or newer is required. The workspace pins its development toolchain in
`rust-toolchain.toml`.

## Define a command

```rust
use dreamcli::Cli;
use dreamcli::Command;
use dreamcli::arg;
use dreamcli::flag;
use std::process::ExitCode;

fn main() -> ExitCode {
    let name = arg::string("name").describe("Who to greet");
    let loud = flag::boolean("loud").alias("l");
    let times = flag::parse::<u16>("times").default(1);

    let greet = Command::new("greet")
        .arg(name.clone())
        .flag(loud.clone())
        .flag(times.clone())
        .action(move |ctx| {
            let message = format!("Hello, {}!", ctx.get(&name)?);
            let message = if ctx.get(&loud)? {
                message.to_uppercase()
            } else {
                message
            };
            for _ in 0..ctx.get(&times)? {
                ctx.out.log(&message);
            }
            Ok(())
        });

    Cli::new("greet")
        .version(env!("CARGO_PKG_VERSION"))
        .default(greet)
        .completions()
        .run()
}
```

An input is named when its handle is created. Clone the handle to register its
schema and move the other copy into a handler. `ctx.get()` infers the Rust type
and presence from that handle:

| Input                                      | Handler value                           |
| ------------------------------------------ | --------------------------------------- |
| `flag::string("name")`                     | `Option<String>`                        |
| `flag::string("name").required()`          | `String`                                |
| `flag::parse::<u16>("port").default(8080)` | `u16`                                   |
| `flag::boolean("force")`                   | `bool`, default `false`                 |
| `flag::count("verbose").alias("v")`        | `u64`, default `0`; `-vvv` supplies `3` |
| `flag::array("tags", flag::string("tag"))` | `Vec<String>`, default empty            |
| `arg::string("name")`                      | `String`                                |
| `arg::string("name").optional()`           | `Option<String>`                        |
| `arg::string("files").variadic()`          | `Vec<String>`                           |

A key/value positional can use `.variadic_entries()` to consume the remaining
arguments into one `BTreeMap<String, T>`.

`get()` clones the resolved value. `get_ref()` borrows it, returning `&T` for a
required handle and `Option<&T>` for an optional handle. This also supports custom
types that cannot be cloned. Flags and positional arguments have distinct Rust
types, so registering an argument with `.flag()` is a compile-time error.

## Native values

`flag::parse::<T>()` and `arg::parse::<T>()` accept any `FromStr` type whose value
is `Send + Sync + 'static`. Integer types, application enums, identifiers, and
other custom values can remain native throughout handler execution. Custom types
do not need Serde. `.choices(["us", "eu"])` adds accepted textual choices to
such a parser, including help and completions.

| Factory                      | Rust value                                                            |
| ---------------------------- | --------------------------------------------------------------------- |
| `string` / `enumeration`     | `String`                                                              |
| `number`                     | `f64`, with `.integer()`, `.min()`, and `.max()`                      |
| `boolean`                    | `bool`, with `.negatable()` on flags                                  |
| `path`                       | `PathBuf`                                                             |
| `url`                        | `url::Url`                                                            |
| `date`                       | `chrono::DateTime<Utc>`                                               |
| `duration`                   | `std::time::Duration`; accepts `1h30m`, `250ms`, or bare milliseconds |
| `bytes`                      | `u64`; `1kb` means 1024 bytes                                         |
| `count`                      | `u64`                                                                 |
| `key_value` / `key_value_of` | `BTreeMap<String, T>`                                                 |
| `array`                      | `Vec<T>`                                                              |

Strings support non-empty, length, and Rust regex constraints. `.validate()` adds
a validator over the actual Rust value. `flag::custom()` and `arg::custom()` accept
an explicit decoder for applications that need structured config or prompt
values. `.sensitive()` hides values from framework diagnostics, debug output,
help defaults, and exported schema defaults; handlers can still access them.

## Resolve values

Each input independently follows:

```text
CLI → stdin → environment → config → prompt → default
```

```rust
use dreamcli::{flag, prompt::Prompt};

let region = flag::enumeration("region", ["us", "eu"])
    .env("DEPLOY_REGION")
    .config("deploy.region")
    .prompt(Prompt::select("Region?", []))
    .default("us".to_owned());
```

Every source is opt-in. Resolution errors are collected so multiple missing or
invalid inputs can be reported together. `ctx.sources()` records the selected
stage for each input; `was_explicit()` distinguishes supplied values from defaults.
Conditional `.interactive()` callbacks receive all values resolved through config
before prompts and defaults. Use `inputs.peek(&handle)` to inspect an input that
may still be absent.

`.stdin()` selects piped input for a missing value or an explicit `-`. An explicit
dash keeps CLI precedence and records that its bytes came from stdin. Stdin is
read once, only when the selected invocation needs it. `StdinBinding` configures
dash/missing triggers, one-terminator trimming, and exclusive or broadcast use.
Collections splice stdin values into the order of CLI occurrences. Splitting is
configured separately for CLI, environment, and stdin; JSON object splitting is
available for key/value collections.

## Commands and application state

Register commands with `Cli::command()`, nest them with `group(...).command(...)`,
and add a single-command entrypoint with `.default()`. CLI `.flag()` inputs are
global. A command-level flag can use `.propagate()` to reach descendants.
Command dispatch skips flag operands, so a value matching a command name is
still a value.

Middleware receives `Next`, which can be consumed once with `next.run(ctx)` or
dropped to stop the chain. `.derive()` computes native application state from
resolved inputs; `.state::<T>()` retrieves it by Rust type identity.

```rust
use dreamcli::{Command, CliError};

struct Credentials(String);

let command = Command::new("deploy")
    .derive(|_| Ok(Credentials("local-token".into())))
    .middleware(|ctx, next| {
        ctx.out.status("Starting deployment");
        next.run(ctx)?;
        ctx.out.status("Finished deployment");
        Ok(())
    })
    .action(|ctx| {
        let credentials = ctx.state::<Credentials>()?;
        if credentials.0.is_empty() {
            return Err(CliError::new("AUTH_REQUIRED", "Authenticate first"));
        }
        Ok(())
    });
```

Plugin hooks are methods on the `Plugin` trait: before parsing, before execution,
after execution, and on error. Handlers and host I/O are synchronous. Applications
using async libraries choose and drive their own runtime inside an action.

## Output, help, and completion

Handlers use `ctx.out` for logs, status messages, JSON, tables, color, spinners,
and progress. JSON mode reserves stdout for structured data; human messages go
to stderr. Quiet mode suppresses info and status while retaining logs, warnings,
and errors. Activity handles update explicitly and clean up on `Drop`; they are
silent when piped or in JSON/quiet mode. Output writers are synchronous, and
writer failures propagate to the run result.

`--help` and optional `--version` run before input validation. `--json`, `--quiet`,
`-q`, and help can be released to application flags with `Builtins`. Everything
after `--` remains literal input.

Enable `.completions()` for both command and flag access:

```console
mycli completions bash
mycli --completions=zsh
mycli --completions fish
```

The generators also support PowerShell. Calling the flag without a shell detects
`SHELL` in native runs and falls back to Bash. Programmatic callers can use
`completion::generate(app.schema(), Shell::Bash)`.

## Configuration and testing

`.config(ConfigSettings::new("mycli"))` discovers JSON config files along project
ancestors, then user and system config directories. `--config PATH` selects one
file explicitly. Custom format loaders accept a parser closure and file extensions.
`cargo_manifest!()` provides compile-time Cargo package metadata.

`execute()` runs in-process using only injected inputs and captures output:

```rust
use dreamcli::{Cli, Command, RunOptions, ResolveOptions, flag};
use std::collections::BTreeMap;

let name = flag::string("name").env("NAME").required();
let app = Cli::new("demo").default(
    Command::new("demo").flag(name.clone()).action(move |ctx| {
        ctx.out.log(ctx.get(&name)?);
        Ok(())
    }),
);
let result = app.execute(Vec::<String>::new(), RunOptions {
    resolve: ResolveOptions {
        env: BTreeMap::from([("NAME".into(), "Ada".into())]),
        ..Default::default()
    },
    ..Default::default()
});
assert!(result.success());
assert_eq!(result.stdout, "Ada\n");
```

Inject `TestPrompter`, `FileSystem`, or a complete `Runtime` for other test cases.
`testkit::run_command()` uses the same executor. `Cli::definition()` exports Rust
schema metadata; `json_schema::input_schema()` describes JSON-representable resolved
values. Opaque custom Rust values are omitted from JSON exports unless they use a
codec with a JSON representation.

Runnable examples are in `crates/dreamcli/examples/`. Build and validation commands
are in [CONTRIBUTING.md](CONTRIBUTING.md). Port compatibility and deliberate Rust
differences are documented in [PORTING.md](PORTING.md).
