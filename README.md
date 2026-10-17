# Fujo

Typed, schema-first command-line applications in Rust. Declare each input once.
The compiler binds its native value to the handler, and the resolver fills it
from arguments, stdin, environment variables, configuration, prompts, or a default.

```rust
use fujo::{Cli, Command, arg, flag};
use std::process::ExitCode;

fn main() -> ExitCode {
    let greet = Command::new("greet")
        .inputs((arg::string("name"), flag::boolean("loud").alias("l")))
        .action(|ctx, (name, loud), ()| {
            let message = format!("Hello, {name}!");
            ctx.out.log(if *loud {
                message.to_uppercase()
            } else {
                message
            });
            Ok(())
        });
    Cli::new("greet").default(greet).run()
}
```

`name` reaches the handler as `&String` and `loud` as `&bool`. Reading an input
the command never registered, treating a number as a string, or forgetting a
bundle element is a compile error.

## One input, several sources

```rust
use fujo::{flag, prompt::Prompt};

let region = flag::enumeration("region", ["us", "eu"])
    .env("DEPLOY_REGION")
    .config("deploy.region")
    .prompt(Prompt::select("Region?", []))
    .default("us".to_owned());
```

Each input independently follows one order:

```text
CLI → stdin → environment → config → prompt → default
```

| Supplied through                      | Handler value | Recorded stage |
| ------------------------------------- | ------------- | -------------- |
| `--region eu`                         | `"eu"`        | `cli`          |
| `DEPLOY_REGION=eu`                    | `"eu"`        | `env`          |
| config `{"deploy": {"region": "eu"}}` | `"eu"`        | `config`       |
| nothing, in a terminal                | the answer    | `prompt`       |
| nothing, piped                        | `"us"`        | `default`      |

Every source is opt-in. `ctx.sources()` records the winning stage for each input,
and `was_explicit()` distinguishes supplied values from defaults. Resolution
errors are collected, so several missing or invalid inputs are reported together.

## Install

The crates have not been published. Clone [Fujo](https://github.com/kjanat/fujo)
and use its local crates from a sibling application:

```sh
git clone https://github.com/kjanat/fujo.git
```

```toml
[dependencies]
fujo = { path = "../fujo/crates/fujo" }
```

Rust 1.99 or newer is required. The workspace pins its development toolchain in
`rust-toolchain.toml`. The default build includes completion generation, native
terminal prompts, and animated terminal activities; see [Cargo features](#cargo-features)
for minimal builds and the opt-in `regex`, `url`, and `chrono` integrations.

## Define a command

`.inputs()` registers a bundle and delivers its resolved native references to
the callback. Required/defaulted inputs become `&T`; optional inputs become
`Option<&T>`. The initial state is `()`. Bundles can be individual inputs, nested
tuples, or [named structs](#named-inputs), and non-Clone values are supported.
The callback's `Scope` exposes output, metadata, and provenance; it has no
arbitrary input or state lookup.

```rust
use fujo::Cli;
use fujo::Command;
use fujo::arg;
use fujo::flag;
use std::process::ExitCode;

fn main() -> ExitCode {
    let name = arg::string("name").describe("Who to greet");
    let loud = flag::boolean("loud").alias("l");
    let times = flag::parse::<u16>("times").default(1);

    let greet =
        Command::new("greet")
            .inputs((name, loud, times))
            .action(|ctx, (name, loud, times), ()| {
                let message = format!("Hello, {name}!");
                let message = if *loud {
                    message.to_uppercase()
                } else {
                    message
                };
                for _ in 0..*times {
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

The existing dynamic API remains available through `.arg()`, `.flag()`, and
`.action(|ctx| ...)`. Clone a handle to register its schema and move the other
copy into that handler. `ctx.get()` infers the Rust value type and presence:

| Input                                      | Handler value                           |
| ------------------------------------------ | --------------------------------------- |
| `flag::string("name")`                     | `Option<String>`                        |
| `flag::string("name").required()`          | `String`                                |
| `flag::parse::<u16>("port").default(8080)` | `u16`                                   |
| `flag::boolean("force")`                   | `bool`, default `false`                 |
| `flag::count("verbose").alias("v")`        | `u64`, default `0`; `-vvv` supplies `3` |
| `flag::array("tags", value::string())`     | `Vec<String>`, default empty            |
| `arg::string("name")`                      | `String`                                |
| `arg::string("name").optional()`           | `Option<String>`                        |
| `arg::string("files").variadic()`          | `Vec<String>`                           |

A key/value positional can use `.variadic_entries()` to consume the remaining
arguments into one `BTreeMap<String, T>`.

`get()` clones the resolved value. `get_ref()` borrows it, returning `&T` for a
required handle and `Option<&T>` for an optional handle. This also supports custom
types that cannot be cloned. Flags and positional arguments have distinct Rust
types, so registering an argument with `.flag()` is a compile-time error.

Dynamic handles are looked up by surface and name, and state by Rust type.
Those APIs check registration and state availability at runtime. Use `.inputs()`
when the handler should be statically bound to its declared values and state.

## Named inputs

Tuples stay compact for small commands. Larger commands declare a struct with
`fujo::inputs!`. The macro also generates the values struct delivered to
callbacks, so field names replace tuple positions:

```rust
use fujo::{Arg, Command, Flag, Required, arg, flag};
use std::path::PathBuf;

fujo::inputs! {
    struct Build => BuildValues {
        root: Arg<PathBuf>,
        jobs: Flag<u16, Required>,
        label: Flag<String>,
    }
}

let build = Command::new("build")
    .inputs(Build {
        root: arg::path("root"),
        jobs: flag::parse("jobs").default(4),
        label: flag::string("label"),
    })
    .action(|ctx, BuildValues { root, jobs, label }, ()| {
        let label = label.map_or("build", String::as_str);
        ctx.out.log(format!("{label}: {} with {jobs} jobs", root.display()));
        Ok(())
    });
```

`Arg<T>` is required by default and `Flag<T>` is optional, matching the
factories; `Flag<T, Required>` names a flag with a default or `.required()`. A
field holds any bundle: a handle, a tuple, or another declared struct. The struct
registers its inputs and fixes the callback types in one step, exactly like a
tuple, so a handle with the wrong surface, value type, or presence is rejected
where the struct is built. Declared bundles are `Clone`; value structs are `Copy`.

`fujo::flags!` declares a flag-only struct. Register it once with
`Cli::inputs(globals.clone())` and include it as a field of each command's
struct to bind the same globals in every handler. A positional argument in a
`flags!` struct is a compile error.

## Bound inputs and state

Typed derives consume the previous state and return its successor. Return a
struct or tuple containing earlier values when later stages need both:

```rust
use fujo::{Command, arg, flag};

let build = Command::new("build")
    .inputs((arg::path("root"), flag::parse::<u16>("jobs").default(4)))
    .derive(|_, (root, jobs), ()| Ok((root.clone(), *jobs)))
    .action(|ctx, _, (root, jobs)| {
        ctx.out.log(format!("{}: {jobs} jobs", root.display()));
        Ok(())
    });
```

The compiler checks the input bundle's shape, native types, and the state type
produced by the preceding derivation. State is owned per invocation and can
contain local values such as `Rc`; it does not need Serde or `Clone`. To share
globals, register a flag bundle with `Cli::inputs(globals.clone())` and include
it in each leaf's `.inputs((globals, local_inputs))` bundle. This also works for
nested commands and propagated flags. Duplicate names and cross-input policies
remain schema/runtime validation concerns.

## Typed middleware

A typed command runs its stages in declaration order. A middleware is a stage:
it receives the state from the stages before it and a continuation for every
stage after it, including the action.

```rust
use fujo::{Command, arg};

let plan = Command::new("plan")
    .inputs(arg::path("root"))
    .derive(|_, root, ()| Ok(root.display().to_string()))
    .middleware(|ctx, _, label, next| {
        let activity = ctx.out.spinner(format!("Planning {label}"));
        let result = next.run(label);
        match &result {
            Ok(()) => activity.succeed("Planned"),
            Err(_) => activity.fail("Planning failed"),
        }
        result
    })
    .derive(|_, _, label| Ok(label.len()))
    .action(|ctx, _, length| {
        ctx.out.log(length);
        Ok(())
    });
```

`next.run(state)` hands the state on and returns the result of the later stages;
the middleware can inspect or replace that result. Dropping `next` instead
short-circuits the later stages and drops the state with it. A synchronous
`.middleware()` wraps only synchronous stages: after it, `derive_async`,
`middleware_async`, and `action_async` are unavailable, so the combination is a
compile error. `.middleware_async()` receives an `AsyncNext` and can wrap
either kind of stage, including earlier asynchronous derivations.

A dynamic `.middleware()` registered on the `Command` before `.inputs()` wraps
the whole typed pipeline, including derivation, and sees only `Context`.

Smaller stages cover the common shapes. `tap` borrows the state and passes it
on, and its error short-circuits; `map` transforms the state without a scope;
`catch` sees the error of every later stage and may replace it; `finally`
observes the outcome after everything else ran. `tap_async` is the asynchronous
tap. `catch` and `finally` keep the tail type, so they fit before asynchronous
stages.

## Stages as values

A stage is a value. A type implementing `Middleware` or `Derive` plugs in with
`wrap` and `then`; `AsyncMiddleware` and `AsyncDerive` plug in with `wrap_async`
and `then_async`. The implementation is generic over the input bundle, so one
stage serves every command:

```rust
use fujo::typed::Next;
use fujo::{Command, InputSet, Middleware, Scope, arg};

struct Spinner(&'static str);
impl<I: InputSet, S> Middleware<I, S> for Spinner {
    fn call<'a>(&'a self, ctx: Scope<'a>, _: I::Values<'a>, state: S, next: Next<'a, S>)
    -> fujo::Result<()> {
        let activity = ctx.out.spinner(self.0);
        let result = next.run(state);
        match &result {
            Ok(()) => activity.succeed("Done"),
            Err(_) => activity.fail("Failed"),
        }
        result
    }
}

let plan = Command::new("plan")
    .inputs(arg::path("root"))
    .wrap(Spinner("Planning"))
    .action(|ctx, root, ()| {
        ctx.out.log(root.display().to_string());
        Ok(())
    });
```

`fujo::pipeline(inputs)` builds stages without a command. A command inherits
them with `inherit(&pipeline)` after its own stages; the pipeline's state and
tail become the command's, so a pipeline with a synchronous middleware keeps
later asynchronous stages out at compile time. `inherit` registers the
pipeline's inputs on the command. `named("label")` labels the stage declared
last for `stages()` and `--explain`:

```rust
use fujo::{Command, flag};

let shared = fujo::pipeline(flag::string("profile").default("dev".into()))
    .derive(|_, profile, ()| Ok(profile.to_uppercase()));
let show = Command::new("show")
    .inputs(())
    .inherit(&shared)
    .action(|ctx, (), profile| {
        ctx.out.log(profile);
        Ok(())
    });
```

A pipeline can also run once at the root or on a group, before dispatch.
`Cli::share(&pipeline)` or `Command::share(&pipeline)` runs it for every
invocation; a command takes its state with `.shared(&pipeline)` as its first
stage, or anywhere with `ctx.shared(&pipeline)?`. Each read clones the value,
so it must be `Clone + Send`. `share` takes flag-only pipelines and registers
their flags on the root or group, and a group's shared flags reach its children.
A shared pipeline wraps only itself; use `inherit` when its middleware should
wrap the command.

```rust
use fujo::{Cli, Command, flag};

let session = fujo::pipeline(flag::string("profile").default("dev".into()))
    .derive(|_, profile, ()| Ok(profile.to_uppercase()));
let show = Command::new("show")
    .inputs(())
    .shared(&session)
    .action(|ctx, (), profile| {
        ctx.out.log(profile);
        Ok(())
    });
let app = Cli::new("demo").share(&session).command(show);
```

`retry(n)` reruns every later stage and the action up to `n` more times with a
clone of the state. `timeout(limit, sleep)` fails with `TIMEOUT` and drops the
later stages when the application's timer finishes first, for example
`timeout(Duration::from_secs(30), tokio::time::sleep)`. A `timeout` declared
before `retry` bounds all attempts together; one declared after it bounds each
attempt.

`Command::stages()` lists dynamic steps, then typed stages, in execution order
with their kind, handler label, and whether they are asynchronous;
`Command::action_info()` names the action and whether its handler is
asynchronous. With `Builtins { explain: true, .. }`, `<command> --explain`
prints the shared stages of the root and the groups on the path, then the
command's stages and action, and runs nothing; `--json` prints it as a document.

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
| `url`                        | `url::Url` (feature `url`)                                            |
| `date`                       | `chrono::DateTime<Utc>` (feature `chrono`)                            |
| `duration`                   | `std::time::Duration`; accepts `1h30m`, `250ms`, or bare milliseconds |
| `bytes`                      | `u64`; `1kb` means 1024 bytes                                         |
| `count`                      | `u64`                                                                 |
| `key_value` / `key_value_of` | `BTreeMap<String, T>`                                                 |
| `array`                      | `Vec<T>`                                                              |

Strings support non-empty and length constraints; feature `regex` adds Rust
regex constraints through `.pattern()`. `.validate()` adds
a validator over the actual Rust value. `flag::custom()` and `arg::custom()` accept
an explicit decoder for applications that need structured config or prompt
values. `.sensitive()` hides values from framework diagnostics, debug output,
help defaults, and exported schema defaults; handlers can still access them.

Collections take a reusable value parser, which has no environment, config,
prompt, alias, or default bindings. Attach those settings to the collection:

```rust
use fujo::{flag, value};

let tag = value::string().non_empty();
let tags = flag::array("tags", tag.clone()).env("TAGS").unique();
let label = tag.flag("label");
let name = value::string().arg("name");
```

Scalar inputs do not expose collection splitting or uniqueness methods. Schema
metadata is readable through `schema()` and immutable outside core.

`parse::<T>()` exports JSON values and defaults for common primitive types.
Application types can supply `.encode_json(|value| ...)` and
`.json_schema(json!({ ... }))` without implementing Serde. Encoding works for
collection elements too. `.default_description("auto")` or `.hide_default()`
changes the help annotation while retaining the machine-readable default;
calling `.default()` again resets the annotation.

## Resolve values

Conditional `.interactive()` callbacks receive all values resolved through config
before prompts and defaults. Use `inputs.peek(&handle)` to inspect an input that
may still be absent.

Input and confirmation prompts support `.default(...)`. Blank input without a
prompt default falls through to the input's default or missing-value handling.
Blank confirmation defaults to `true`, or the configured boolean default.
`Prompt::input(...).validate(...)` checks typed answers; terminal prompts retry
invalid answers, and scripted prompts retry while answers remain, then fall
through. Prompt defaults bypass that answer validator. Cancellation stops the
invocation with exit code 130.

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

Dynamic middleware receives `Next`, which can be consumed once with `next.run(ctx)`
or dropped to stop the chain. In the dynamic API, `.derive()` computes native
state from resolved inputs and `.state::<T>()` retrieves it by Rust type identity.

```rust
use fujo::{Command, CliError};

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
after execution, and on error.

## Async execution

Use native async closures with `.action_async()`, `.derive_async()`, and
`.middleware_async()`. Both dynamic and bound handlers support async execution:

```rust
use fujo::{Cli, Command, flag};

let command = Command::new("demo")
    .inputs(flag::string("name").default("world".into()))
    .derive_async(async |_, name, ()| Ok(name.to_uppercase()))
    .action_async(async |ctx, _, greeting| {
        ctx.out.log(greeting);
        Ok(())
    });
let app = Cli::new("demo").default(command);
// Within your application's async entrypoint:
// let result = app.execute_async(Vec::<String>::new(), Default::default()).await;
// Return app.run_async().await from an async main for native stdio.
```

`execute_async`, `run_with_async`, `run_os_with_async`, and `run_async` share
preparation and result handling with their synchronous counterparts. The library
selects no executor. Futures are local and may borrow native inputs or hold
non-Send state across suspension; await them directly or use a local task set.
They cannot be passed to a Send-only spawn API. Callback captures remain
`Send + Sync + 'static`. Prompts, config, filesystem preparation, and stdio remain
synchronous. The [async build](crates/fujo/examples/async_build.rs) and
[manifest](crates/fujo/examples/manifest.rs) examples use Tokio as an
application dependency; Tokio is not a library dependency.

Dynamic async middleware consumes `AsyncNext` and awaits `next.run(ctx)`. It can
wrap sync actions, sync derives, or a fully synchronous middleware tail. A sync
middleware cannot wrap async work; use `middleware_async` for that wrapper.
Such dynamic combinations fail with `ASYNC_MIDDLEWARE_REQUIRED` before execute
hooks or middleware run; typed commands reject them at compile time. Sync
runners reject async work with `ASYNC_REQUIRED`; help, version, and completions
remain available synchronously. Dropping a suspended invocation drops its state
and activity guards.

## Output, help, and completion

Handlers use `ctx.out` for logs, status messages, JSON, tables, color, spinners,
and progress. JSON mode reserves stdout for structured data; human messages go
to stderr. Quiet mode suppresses info and status while retaining logs, warnings,
and errors. TTY spinners animate every 80 ms while handlers work; progress handles
draw a bar, percentage, and item count on each update. Logs and tables temporarily
clear the shared activity line, then restore it. Finishing or dropping a spinner
stops and joins its timer. Activities are silent when piped or in JSON/quiet mode.
Captures retain messages and completion lines, excluding transient frames and
cursor controls; `events()` records logical activity lifecycles. Capture-only
outputs have no timer. Writer failures propagate to the run result.

The [terminal example](crates/fujo/examples/README.md#live-terminal-showcase)
shows these activities in an interactive terminal.

`--help` and optional `--version` run before input validation. `--json`, `--quiet`,
`-q`, and help can be released to application flags with `Builtins`. Everything
after `--` remains literal input.

`--help --json` writes the program definition to stdout; command help with JSON
mode writes the selected command definition. Both paths avoid running actions
or reading bound input sources. Help and completion choose kebab-case names only
when the active parser accepts them. `RunOptions.help` supports alphabetical or
declaration order and a custom comparator through `HelpOptions::sort_flags`.

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
file explicitly and appears in help and completions. Custom format loaders accept a parser closure and file extensions.
`cargo_manifest!()` provides compile-time Cargo package metadata.

Each project ancestor is checked for `.mycli.json`, `mycli.config.json`, and
`.config/mycli.json`, nearest first. User paths include XDG locations, macOS
Application Support, or Windows AppData; Unix system config is
`/etc/mycli/config.json`. `ConfigSettings::base_dir` anchors the ancestor search.
`path_loader` receives the discovered path and content for richer providers.

`execute()` runs in-process using only injected inputs and captures output:

```rust
use fujo::{Cli, Command, RunOptions, ResolveOptions, flag};
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
`testkit::run_cli(&app, ...)` preserves the complete application configuration.
`run_command_with_builtins` lets command-only tests release builtin names.
`Cli::definition()` exports Rust schema metadata with `format: "fujo"`
and `version: 1`.
`json_schema::input_schema()` describes JSON-representable resolved
values. Opaque custom Rust values are omitted from JSON exports unless they use a
codec with a JSON representation.

Missing filesystem capabilities are reported through `RunResult.skipped_checks`
and `Resolved.skipped_checks`. Set `ResolveOptions.filesystem_policy` to
`CheckPolicy::Required` to fail when declared checks cannot run, or to
`CheckPolicy::Disabled` to opt out explicitly. The default `Available` policy
runs injected checks and records skips. Native runs inject a filesystem.
Use `PathChecks::file()` for existing input files and
`PathChecks::file().allow_missing()` for output files that may be new.

## Examples

The [example CLI guide](crates/fujo/examples/README.md) describes each example,
its commands, and its tests. The [manifest example](crates/fujo/examples/manifest.rs)
is the complete application: named input bundles, shared globals, two commands,
typed middleware, concurrent async work, and terminal output. Build and
validation commands are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Workspace crates

| Crate           | Responsibility                                                               |
| --------------- | ---------------------------------------------------------------------------- |
| `fujo`          | Public API, command execution, middleware, plugins, help, and native runtime |
| `fujo-core`     | Typed input builders, parsing, resolution, provenance, and schema metadata   |
| `fujo-output`   | `Write` destinations, capture, tables, colors, and activity guards           |
| `fujo-complete` | Bash, Zsh, Fish, and PowerShell completion generation                        |
| `fujo-error`    | Shared structured errors and error categories                                |

The core has no process-global reads. Output depends on the shared error crate,
so it can be used without pulling in the parser and resolver. The public crate
re-exports the APIs applications normally need.

Fujo is a native Rust port of DreamCLI. The
[DreamCLI TypeScript project](https://github.com/kjanat/dreamcli) remains its
upstream behavioral reference. Port compatibility and Rust
differences are documented in [PORTING.md](PORTING.md).

## Cargo features

| Feature             | Default | Controls                                                                  |
| ------------------- | ------- | ------------------------------------------------------------------------- |
| `completion`        | Yes     | `fujo-complete`, `Cli::completions()`, and the `completion` re-export     |
| `terminal-prompts`  | Yes     | `TerminalPrompter` and its `rpassword` dependency                         |
| `terminal-activity` | Yes     | TTY activity rendering and spinner timer threads                          |
| `regex`             | No      | String `.pattern()` constraints and the `regex` dependency                |
| `url`               | No      | `flag::url()`, `arg::url()`, `value::url()`, and native `url::Url` values |
| `chrono`            | No      | `flag::date()`, `arg::date()`, `value::date()`, and native Chrono values  |

For a minimal application, or one that supplies its own prompt implementation:

```toml
[dependencies]
fujo = { path = "../fujo/crates/fujo", default-features = false }
```

Add `features = ["url", "chrono"]` to enable those integrations, with or without
defaults. Features are additive: another dependency enabling a feature enables
it for the shared crate. Internal dependencies disable defaults and forward
features explicitly. `fujo-core` has no default features;
`fujo-output` defaults to `terminal-activity` and can disable it independently.

Minimal builds retain input resolution, provenance, validation, middleware,
typed and async execution, config, output capture, and JSON/tables. Injected
`PromptEngine` implementations and `TestPrompter` work without `terminal-prompts`;
only automatic native prompting is removed. Spinner/progress handles still
record logical events and finish on Drop without `terminal-activity`, but emit
no activity text or frames. Duration and byte-size parsing need no regex feature.
Serde and `serde_json` remain mandatory because config, prompt interchange, and
metadata use them. Definition and JSON Schema exports remain available.

Output can also be adopted independently by a clap application or another parser:
depend only on `fujo-output` and pass the application's writers and
`OutputOptions` to `Output::with_writers`. That crate depends on neither core
resolution nor completion generation. Standalone `fujo-complete` uses core
metadata without bringing in the specialized value dependencies.
