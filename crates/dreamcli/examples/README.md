# Example CLIs

Each example keeps input declarations, validation, orchestration, and output
selection together in one `app() -> Cli` function. Supporting domain types stay
in the same file. `main()` calls `app().run()` or awaits `app().run_async()` using
the application's executor. The integration tests import
these exact definitions and execute them with injected inputs.

| Example                          | CLI logic                                 | What it demonstrates                                                                                                                                                                        |
| -------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [release.rs](release.rs)         | `app()` near the top; version types below | Native `Version` and `Bump`, checked version arithmetic, config/env/CLI precedence, conditional secret prompts, typed derived plans, middleware, JSON encoding without Serde on input types |
| [batch.rs](batch.rs)             | `app()`; job/result types below           | Variadic checked paths, nonzero worker counts, typed priority maps, validated/deduplicated arrays, durations, bytes, global flags, collision policy, provenance, tables, and progress       |
| [settings.rs](settings.rs)       | `app()` with a local command factory      | Nested commands, inherited profile, native URL/port values, production HTTPS policy, explicit stdin credentials, redaction, effective settings, provenance, and JSON Schema                 |
| [terminal.rs](terminal.rs)       | `app()` with a single action              | Automatic spinner animation, label changes, live progress bars, status messages during progress, verbose diagnostics, failure cleanup, tables, and JSON/quiet output                        |
| [async_build.rs](async_build.rs) | `app()` with bound inputs and state       | Command-bound native values, explicit state construction, shared globals, async derives/actions/middleware, application-owned Tokio, live spinner/progress, JSON and quiet output           |

These examples build release and batch plans, inspect settings, and simulate work. Running them
does not upload artifacts, copy/move/link files, or write configuration.

The release and settings examples require the opt-in `url` integration. Cargo
records required features for each example. The commands below retain the
default completion and terminal features.

## Typed async build

```sh
cargo run -p dreamcli --example async_build
cargo run -p dreamcli --example async_build -- --profile=prod --jobs=20 --delay=150ms
cargo run -p dreamcli --example async_build -- --delay=0ms --jobs=2 --json
```

The demo binds a shared profile, a native nonzero job count, and a native duration
to its callbacks. An async derive constructs `Build`; the action receives that
state directly. `tokio::time::sleep` yields while the output spinner ticks and
the action advances its progress bar. Tokio is a dev dependency used by this
example; DreamCLI itself selects no executor. `greet.rs` also uses bound inputs.

The release, batch, settings, and terminal examples retain the dynamic handle
API. Their compile-time checks establish value and presence types, while handle
registration and type-map state availability are checked at runtime. Bound
callbacks additionally establish their declared input bundle and state shape.

## Live terminal showcase

Run this in an interactive terminal without `--json`:

```sh
cargo run -p dreamcli --example terminal
```

Over about five seconds, the demo discovers inputs with an animated spinner,
updates its label while checking dependencies, builds 24 simulated items with a
live bar and percentage, and animates verification before printing a result table.
A status message appears halfway through without overwriting the bar. Spinners
tick every 80 ms even while the action is blocked in `thread::sleep`; the action
does not manually draw frames.

```sh
cargo run -p dreamcli --example terminal -- --steps=40 --delay=150ms --verbose
cargo run -p dreamcli --example terminal -- --fail-at=8
cargo run -p dreamcli --example terminal -- --delay=0ms --json
cargo run -p dreamcli --example terminal -- --quiet
```

`--steps` is a native `NonZeroU16` constrained to 1–100; `--delay` is a native
`Duration` capped at one second per item. `--fail-at` must identify an existing
item; a simulated failure clears the bar, reports `DEMO_FAILURE`, exits 2, and
skips the success table. `--verbose` adds diagnostics between progress updates;
`--quiet` suppresses them along with status and animation. JSON produces a result
array on stdout and keeps human messages on stderr. Piping suppresses animation.

The release and batch planners finish immediately, so they usually cannot display
multiple frames. This showcase intentionally paces simulated work. Captures omit
transient frames and retain logical activity events; the Unix PTY check in
`scripts/check-terminal.py` verifies real frame timing and terminal rendering.

## Release planner

Use an existing artifact. For a quick local demonstration, the workspace
`Cargo.toml` can serve as the checked input file:

```sh
cargo run -p dreamcli --features url --example release -- plan Cargo.toml --from=1.4.2 --bump=minor --json
```

The result contains `current: "1.4.2"` and `next: "1.5.0"`. `Version` and `Bump`
reach the derivation as native Rust values; neither input type implements Serde.
The plan has its own serializable representation.

Add `--publish` to include an authenticated publish step in the plan. Supply
`SHIPYARD_TOKEN`, `--token`, or answer the secret prompt in a terminal. An absent
credential in a noninteractive invocation produces `AUTH_REQUIRED`. Tokens never
appear in the release plan.

The default configuration discovery uses the `shipyard` name. Example contents:

```json
{
  "release": {
    "from": "1.4.2",
    "bump": "minor",
    "registry": "https://registry.example.invalid"
  }
}
```

`SHIPYARD_VERSION` overrides `release.from`; `--from` overrides both. All registry
URLs must use HTTPS. The version parser accepts three decimal `u32` components
without leading zeros; it is intentionally a small A.B.C release model, with
checked increments.

## Batch planner

```sh
cargo run -p dreamcli --example batch -- plan Cargo.toml README.md \
  --output planned-output --workers 8 --timeout 1m30s --budget 2mb \
  --tag fast,local,fast --priority README.md=20 --explain --json
```

The result contains one job per input, a deduplicated tag list, numeric
priorities, an 8-worker limit, a 90,000ms timeout, and a 2,097,152-byte budget.
`--explain` includes winning-source provenance. Omit `--json` for a table.

Workers are `NonZeroU16`, constrained to 1–64. Priority values are `u16`,
constrained to 0–100. Duplicate priority keys fail. Input files must exist;
the output directory may be new. Inputs producing the same target name fail
with `TARGET_COLLISION`. Path collision policy is lexical; this planning example
does not establish file identity through symlinks or filesystem aliases.

`BATCH_LABEL`, `BATCH_WORKERS`, and `BATCH_TAGS` provide environment defaults.
The `batch` configuration supports `mode` and `workers`.

## Nested settings inspector

```sh
cargo run -p dreamcli --features url --example settings -- config show --profile prod --port 9443 --json
cargo run -p dreamcli --features url --example settings -- config validate --endpoint http://localhost
cargo run -p dreamcli --features url --example settings -- config schema
printf '%s\n' 'example-token' | cargo run -p dreamcli --features url --example settings -- config show --token=- --json
```

The first command reports effective values and their sources. The second uses
the default development profile; adding `--profile prod` rejects HTTP with
`HTTPS_REQUIRED`. The schema includes inherited global inputs.

The explicit `--token=-` binding reads stdin once. JSON and table output show
`[redacted]`, while derived state can check whether authentication is configured.
Supported environment names are `SETTINGS_PROFILE`, `SETTINGS_PORT`,
`SETTINGS_ENDPOINT`, `SETTINGS_FEATURES`, and `SETTINGS_TOKEN`.

Example `settings` configuration:

```json
{
  "profile": "prod",
  "server": { "port": 9443, "endpoint": "https://service.example.invalid" },
  "features": ["cache", "metrics"],
  "limits": { "requests": 50 }
}
```

Use `--config PATH` to select a file explicitly. Without it, each example uses
the project/user/system discovery described in the main README.

## Type and behavior checks

```sh
cargo test -p dreamcli --features url --test example_clis
cargo test -p dreamcli --test typed --test async_execution --test lifecycle_reference
cargo test -p dreamcli --doc
cargo check -p dreamcli --features url --examples
```

The handlers contain explicit native type annotations: `Version`, `Bump`,
`NonZeroU16`, `PathBuf`, `Duration`, `Vec<PathBuf>`, and
`BTreeMap<String, u16>`. The compiler verifies these against their input handles.
Compile-fail doctests also reject treating native ports or paths as strings,
assigning `Vec<u16>` to `Vec<String>`, supplying a string default to `u16`,
mixing positional inputs with flags, and attaching binding settings to collection
element parsers.

Runtime tests cover precedence, authenticated and unauthenticated release plans,
prompt answers, credential redaction, invalid/overflowing versions, nonzero/range
checks, file existence, target collisions, typed collections, progress events,
nested dispatch, schema output, JSON help, and all four completion generators.
They verify that the original input files are unchanged and the planned output
directory is not created. Cross-input policies and filesystem facts are checked
at runtime; the compiler enforces input value types and presence accessors.

Bound-input compile-fail checks also reject unrelated handle reads, undeclared
bundle elements, wrong native value types, missing state, and positional inputs
in root globals. Async continuations cannot be invoked twice. Runtime tests
exercise borrowed non-Clone inputs, local state across awaits, globals at nested
command positions, failed derives, middleware wrapping, and dropped invocations.

Every example supports `--help`, `--help --json`, and
`completions bash|zsh|fish|powershell`. For example:

```sh
cargo run -p dreamcli --example batch -- plan --help
cargo run -p dreamcli --features url --example release -- plan --help --json
cargo run -p dreamcli --features url --example settings -- completions fish
```
