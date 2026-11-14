# Fujo

Fujo builds command-line applications in Rust. Declare inputs with their types
and sources, then receive their resolved values in the command handler.

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

The handler receives `name` as `&String` and `loud` as `&bool`. The compiler
rejects access to undeclared inputs, incorrect value types, and missing tuple
elements.

## One input, several sources

```rust
use fujo::{flag, prompt::Prompt};

let region = flag::enumeration("region", ["us", "eu"])
    .env("DEPLOY_REGION")
    .config("deploy.region")
    .prompt(Prompt::select("Region?", []))
    .default("us".to_owned());
```

Fujo resolves each input in this order:

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

Enable each source on the input declaration. `ctx.sources()` records which source
supplied each value. `was_explicit()` distinguishes supplied values from defaults.
Fujo reports missing and invalid inputs together.

## Install

Add Fujo from crates.io:

```sh
cargo add fujo
```

```toml
[dependencies]
fujo = "0.1.0"
```

Fujo requires Rust 1.99 or newer. `rust-toolchain.toml` pins the development
toolchain. The default build includes completions, terminal prompts, and animated
terminal activities. See [Cargo features](#cargo-features) for smaller builds and
the optional `regex`, `url`, and `chrono` integrations.

## Define a command

`.inputs()` registers inputs and passes their resolved values to the handler.
Required inputs and inputs with defaults become `&T`. Optional inputs become
`Option<&T>`. The initial state is `()`.

Register a single input, a nested tuple, or a [named struct](#named-inputs).
Values can be non-Clone types. The handler's `Scope` provides output, command
metadata, and input sources. Inputs and state arrive as separate parameters.

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

The dynamic API uses `.arg()`, `.flag()`, and `.action(|ctx| ...)`. Clone a handle
for registration and move the other copy into the handler. `ctx.get()` returns
the type and optionality declared by the handle:

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

Use `.variadic_entries()` on a key/value positional argument to collect the
remaining arguments into a `BTreeMap<String, T>`.

`get()` clones the value. `get_ref()` borrows it as `&T` or `Option<&T>` and
supports non-Clone values. Flags and positional arguments have separate types,
so passing an argument to `.flag()` fails to compile.

Dynamic access looks up inputs by kind and name, and state by Rust type. Fujo
checks their availability at runtime. `.inputs()` binds the handler's parameters
to its declarations at compile time.

## Named inputs

Use tuples for small commands. `fujo::inputs!` declares a struct for larger input
sets and a second struct for the values passed to handlers:

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

`Arg<T>` is required by default. `Flag<T>` is optional. Use `Flag<T, Required>`
for a flag with `.required()` or a default. Each field accepts an input handle,
a tuple, or another input struct. The compiler checks the supplied handles
against these field types. Input structs implement `Clone`; value structs
implement `Copy`.

`fujo::flags!` declares a struct containing only flags. Register global flags with
`Cli::inputs(globals.clone())`, then include that struct in each command's inputs.
The compiler rejects positional arguments in a `flags!` struct.

## Bound inputs and state

Each `.derive()` consumes the previous state and returns the next state. Keep
values needed by later stages in a struct or tuple:

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

The compiler checks the input types and the state passed between stages. Each
invocation owns its state, which can contain local values such as `Rc` without
implementing Serde or `Clone`.

Register global flags with `Cli::inputs(globals.clone())` and include them in a
command's `.inputs((globals, local_inputs))`. This also supports nested commands
and propagated flags. Fujo checks duplicate names and rules involving multiple
inputs when it validates the schema or runs the command.

## Typed middleware

Stages run in declaration order. Middleware receives the current state and a
continuation for the remaining stages and action:

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

`next.run(state)` runs the remaining stages and returns their result. Middleware
can inspect or replace that result. Dropping `next` skips those stages and drops
the state.

Synchronous `.middleware()` requires all later stages to be synchronous. The
compiler rejects a later `derive_async`, `middleware_async`, or `action_async`.
`.middleware_async()` receives an `AsyncNext`, can wrap either kind of stage,
and can follow asynchronous derivations.

Dynamic middleware registered before `.inputs()` wraps the whole typed command,
including its derivations, and receives a `Context`.

Several methods handle common middleware tasks:

- `tap` borrows the state and passes it on. An error stops execution.
- `map` transforms the state without a scope.
- `catch` receives errors from later stages and can replace them.
- `finally` receives the outcome after the remaining stages finish.
- `tap_async` provides an asynchronous `tap`.

`catch` and `finally` can precede asynchronous stages.

## Stages as values

Implement `Middleware` or `Derive` to reuse a stage with `.wrap()` or `.then()`.
Their asynchronous equivalents use `.wrap_async()` and `.then_async()`. A stage
generic over the input bundle can serve multiple commands:

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

`fujo::pipeline(inputs)` declares stages independently of a command.
`.inherit(&pipeline)` appends them to a command and registers their inputs.
The command takes the pipeline's resulting state and restrictions on later
stages. A synchronous middleware in the pipeline therefore prevents later
asynchronous stages.

`.named("label")` labels the most recently declared stage for `.stages()` and
`--explain`.

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

Use `Cli::share(&pipeline)` or `Command::share(&pipeline)` to run a pipeline once
at the root or a command group, before dispatch. A command reads its result with
`.shared(&pipeline)` as its first stage, or with `ctx.shared(&pipeline)?` from a
handler. Each read clones the state, which must implement `Clone + Send`.

Shared pipelines accept only flags. Fujo registers them on the root or group,
and group flags reach child commands. Shared middleware wraps its own pipeline.
Use `inherit` to wrap the command's stages too.

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

`.retry(n)` reruns the remaining stages and action up to `n` more times after a
failure, cloning the state for each attempt.

`.timeout(limit, sleep)` returns `TIMEOUT` and drops the remaining stages if the
application's timer finishes first. For example, use
`timeout(Duration::from_secs(30), tokio::time::sleep)`. A timeout before `retry`
covers all attempts. A timeout after `retry` applies to each attempt.

`Command::stages()` lists dynamic steps followed by typed stages in execution
order, with their kinds, handler labels, and async status. `Command::action_info()`
describes the action handler.

Enable `Builtins { explain: true, .. }` to inspect execution with
`<command> --explain`. It lists the root and group pipelines, command stages,
and action without running them. Add `--json` for JSON output.

## Native values

`flag::parse::<T>()` and `arg::parse::<T>()` accept `FromStr` types that implement
`Send + Sync + 'static`. Handlers receive those Rust values directly, including
integers, application enums, and custom identifiers. Serde is optional for these
types. `.choices(["us", "eu"])` restricts accepted text and adds choices to help
and completions.

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

Strings support non-empty and length checks. Enable `regex` to use `.pattern()`.
`.validate()` checks the Rust value. `flag::custom()` and `arg::custom()` accept
decoders for structured configuration or prompt answers.

`.sensitive()` redacts values in framework diagnostics, debug output, help
defaults, and exported schema defaults. Handlers receive the full value.

Collections use a `ValueParser<T>` for their elements. Attach sources, aliases,
and defaults to the collection itself:

```rust
use fujo::{flag, value};

let tag = value::string().non_empty();
let tags = flag::array("tags", tag.clone()).env("TAGS").unique();
let label = tag.flag("label");
let name = value::string().arg("name");
```

Splitting and uniqueness methods are available on collection inputs. `schema()`
exposes declaration metadata, which only the core crate can modify.

`parse::<T>()` exports JSON values and defaults for common primitive types.
Custom types can provide `.encode_json(|value| ...)` and
`.json_schema(json!({ ... }))`. These also work for collection elements.

`.default_description("auto")` and `.hide_default()` change how help displays
the default while preserving its exported value. Calling `.default()` again
resets that display setting.

## Resolve values

`.interactive()` callbacks receive values resolved through the configuration
stage, before prompts and defaults. `inputs.peek(&handle)` reads an input that
may still be absent.

Input and confirmation prompts accept `.default(...)`. Blank input without a
prompt default falls back to the input's default or missing-value handling.
Blank confirmation uses its configured default, or `true`.

`Prompt::input(...).validate(...)` checks answers. Terminal prompts retry invalid
answers. Scripted prompts retry until their answers run out, then fall back.
Prompt defaults bypass the answer validator. Cancellation exits with code 130.

`.stdin()` reads piped input for a missing value or an explicit `-`. An explicit
`-` keeps CLI precedence while recording stdin as the byte source. Fujo reads
stdin once, when the selected command needs it.

`StdinBinding` controls explicit-dash and missing-value triggers, removal of one
trailing terminator, and exclusive or broadcast use. Collections insert stdin
values at the corresponding CLI position. Configure splitting separately for
CLI, environment, and stdin. Key/value collections also accept JSON objects.

`.separator(',')` takes one character. Use `.separator(fujo::delimiter!("::"))`
for a longer delimiter. The macro rejects empty strings at compile time.
`schema::Delimiter::try_from(text)?` validates a runtime string.
`Split::Delimiter(fujo::delimiter!(", "))` works with `.split_cli()`,
`.split_env()`, and `.split_stdin()`.

## Commands and application state

Register commands with `Cli::command()`, nest them with `group(...).command(...)`,
and select a single-command entry point with `.default()`. Flags registered on
`Cli` are global. `.propagate()` makes a command's flag available to descendants.
Dispatch treats flag operands as values even when they match a command name.

Dynamic middleware receives a `Next`. Call `next.run(ctx)` once to continue or
drop it to stop execution. Dynamic `.derive()` builds state from resolved inputs;
`ctx.state::<T>()` retrieves it by Rust type.

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

The `Plugin` trait provides hooks before parsing, before execution, after
execution, and on error.

## Async execution

Both dynamic and typed handlers accept native async closures through
`.action_async()`, `.derive_async()`, and `.middleware_async()`:

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

`execute_async`, `run_with_async`, `run_os_with_async`, and `run_async` use the
same preparation and result handling as the synchronous runners. The application
provides the executor.

Invocation futures can borrow inputs and hold non-Send state across awaits.
Await them directly or run them in a local task set. Callback captures must
still implement `Send + Sync + 'static`. Configuration, prompts, filesystem
preparation, and stdio use synchronous operations. The
[async build](crates/fujo/examples/async_build.rs) and
[manifest](crates/fujo/examples/manifest.rs) examples use Tokio in the application.
Fujo has no runtime dependency on Tokio.

Dynamic async middleware awaits `next.run(ctx)`. It can wrap synchronous or
asynchronous work. Dynamic synchronous middleware wrapping async work fails with
`ASYNC_MIDDLEWARE_REQUIRED` before execution hooks or middleware run. Typed
commands reject that combination at compile time.

Synchronous runners reject async work with `ASYNC_REQUIRED`. They can still
serve help, version, and completions. Dropping a suspended invocation drops its
state and activity guards.

## Output, help, and completion

`ctx.out` writes logs, status messages, JSON, tables, color, spinners, and progress.
JSON mode sends structured data to stdout and human messages to stderr. Quiet
mode suppresses info and status, while preserving logs, warnings, and errors.

TTY spinners update every 80 ms. Progress handles draw a bar, percentage, and
item count on each update. Logs and tables clear the activity line while they
print, then restore it. Completing or dropping a spinner stops and joins its
timer. Piped output and JSON or quiet mode suppress activities.

Output capture stores messages and completion lines without animation frames or
cursor controls. `events()` records activity creation, updates, and completion.
Capture-only output starts no timer. Writer errors reach the run result.

The [terminal example](crates/fujo/examples/README.md#terminal-output)
shows spinner and progress output.

`--help` and the optional `--version` run before input validation. `Builtins`
lets applications reclaim `--json`, `--quiet`, `-q`, and help flags. Everything
after `--` is literal input.

`--help --json` exports the program definition. With a command selected, it
exports that command's definition. Neither runs actions nor reads bound input
sources. Help and completions use kebab-case names only when the parser accepts
them. `RunOptions.help` selects alphabetical or declaration order;
`HelpOptions::sort_flags` accepts a custom comparator.

Enable `.completions()` to add a command and flag:

```console
mycli completions bash
mycli --completions=zsh
mycli --completions fish
```

PowerShell is also supported. Without a shell argument, native runs read `SHELL`
and fall back to Bash. Applications can call
`completion::generate(app.schema(), Shell::Bash)` directly.

Choices complete their declared values. Paths complete file names. Free-text
inputs provide no suggestions. After `--`, completion treats words as arguments.
Zsh scripts work when sourced or installed as `_mycli` in `fpath`.

PowerShell completes paths, including `--config=PATH`. Some versions, including
7.6, reject empty completion results. On those versions the script returns
nonempty text and unmatched choices unchanged; empty free-text input may fall
back to file completion. The script detects support for empty results and uses
it to suppress that fallback where possible.

## Configuration and testing

`.config(ConfigSettings::new("mycli"))` searches project ancestors, then user and
system directories for JSON configuration. `--config PATH` selects a file and
appears in help and completions. Custom loaders take a parser closure and file
extensions. `cargo_manifest!()` supplies Cargo package metadata at compile time.

At each project ancestor, Fujo checks `.mycli.json`, `mycli.config.json`, and
`.config/mycli.json`, nearest first. User directories follow XDG, macOS
Application Support, or Windows AppData conventions. Unix system configuration
uses `/etc/mycli/config.json`. `ConfigSettings::base_dir` sets where the ancestor search starts.
`path_loader` passes both the discovered path and its content to a loader.

`execute()` accepts injected inputs and captures output in-process:

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

Inject `TestPrompter`, `FileSystem`, or a complete `Runtime` for other tests.
`testkit::run_cli(&app, ...)` keeps the application's configuration.
`run_command_with_builtins` lets command tests reclaim builtin names.

`Cli::definition()` exports schema metadata with `format: "fujo"` and `version: 1`.
`json_schema::input_schema()` describes resolved values that have JSON
representations. Custom Rust values appear in JSON exports when their codec
provides an encoding.

`RunResult.skipped_checks` and `Resolved.skipped_checks` record filesystem checks
that could not run. `ResolveOptions.filesystem_policy` controls this behavior:

- `CheckPolicy::Required` fails if a declared check cannot run.
- `CheckPolicy::Disabled` skips filesystem checks.
- `CheckPolicy::Available`, the default, runs available checks and records skips.

Native runs supply a filesystem. `PathChecks::file()` checks an existing input
file. Add `.allow_missing()` for a new output file.
`PathChecks::directory().create()` creates a missing directory. The `create`
method is available only for directory checks.

## Examples

The [example CLI guide](crates/fujo/examples/README.md) lists commands and tests
for each example. [manifest.rs](crates/fujo/examples/manifest.rs) combines named
inputs, global flags, two commands, typed middleware, concurrent async work, and
terminal output. See [CONTRIBUTING.md](CONTRIBUTING.md) for build and test commands.

## Workspace crates

| Crate           | Responsibility                                                               |
| --------------- | ---------------------------------------------------------------------------- |
| `fujo`          | Public API, command execution, middleware, plugins, help, and native runtime |
| `fujo-core`     | Typed input builders, parsing, resolution, provenance, and schema metadata   |
| `fujo-output`   | `Write` destinations, capture, tables, colors, and activity guards           |
| `fujo-complete` | Bash, Zsh, Fish, and PowerShell completion generation                        |
| `fujo-error`    | Shared structured errors and error categories                                |

Core receives environment and filesystem data through explicit inputs. Output
depends on the error crate and works without the parser or resolver. `fujo`
re-exports the public APIs used by applications.

Fujo ports the behavior of [DreamCLI](https://github.com/kjanat/dreamcli) to Rust.
[PORTING.md](PORTING.md) describes the compatibility tests and Rust-specific
choices.

## Cargo features

| Feature             | Default | Controls                                                                  |
| ------------------- | ------- | ------------------------------------------------------------------------- |
| `completion`        | Yes     | `fujo-complete`, `Cli::completions()`, and the `completion` re-export     |
| `terminal-prompts`  | Yes     | `TerminalPrompter` and its `rpassword` dependency                         |
| `terminal-activity` | Yes     | TTY activity rendering and spinner timer threads                          |
| `regex`             | No      | String `.pattern()` constraints and the `regex` dependency                |
| `url`               | No      | `flag::url()`, `arg::url()`, `value::url()`, and native `url::Url` values |
| `chrono`            | No      | `flag::date()`, `arg::date()`, `value::date()`, and native Chrono values  |

Disable default features for a minimal build or an application with its own
prompt implementation:

```toml
[dependencies]
fujo = { version = "0.1.0", default-features = false }
```

Add `features = ["url", "chrono"]` to enable those integrations. Cargo combines
features requested by all dependents. Internal dependencies disable defaults
and forward features explicitly. `fujo-core` has no default features.
`fujo-output` enables `terminal-activity` by default.

Minimal builds include input resolution, validation, middleware, typed and async
execution, configuration, capture, JSON, and tables. `PromptEngine` and
`TestPrompter` work without `terminal-prompts`. Without `terminal-activity`,
spinner and progress handles still record events and finish on drop, but draw
nothing. Duration and byte-size parsing work without `regex`.

Serde and `serde_json` are required for configuration, prompts, and metadata.
Definition and JSON Schema exports are available in minimal builds.

To use Fujo output with another parser, depend on `fujo-output` and pass writers
and `OutputOptions` to `Output::with_writers`. `fujo-complete` can also be used
separately and uses core metadata without the optional value integrations.

## Releases

Beepboop opens a version PR with manifest and lockfile updates. Merging it
publishes the five crates after CI passes. Beepboop records each publication
and creates a signed `v<version>` tag and a GitHub Release with generated notes.

`publish.yml` retrieves the `crates-io` credential through Beepboop. The `promote`
operation needs a runtime grant for that credential.
