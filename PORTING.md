# Port contract

Fujo is a native Rust port of DreamCLI. The
[DreamCLI TypeScript project](https://github.com/kjanat/dreamcli) in `../dreamcli`
is the behavior reference, initially inspected at commit
`f87c1672e10b27c066524a0715bfea0f714dd5bc`. The Rust implementation is organized by
crate responsibilities and uses Rust types and host interfaces.

## Preserved behavior

- CLI → stdin → env → config → prompt → default precedence, for flags and args.
- Source provenance, including CLI-selected stdin versus fallback stdin.
- Long and short aliases, inline values, short clusters, boolean negation,
  duplicate policies, counts, repeated arrays, key/value records, and case parity.
- Scalar and element constraints, collection splitting, stdin splicing, and
  validation of declared defaults.
- Required/optional inputs, variadic positionals, nested dispatch, defaults,
  propagation, middleware wrapping, conditional prompts, and typed derived state.
- Root help/version priority, JSON and quiet modes, separator handling, structured
  errors, config loading, shell completion, tables, and terminal activity lifecycle.
- In-process execution with injectable environment, stdin, prompt answers,
  filesystem capabilities, and output destinations.

The differential suite verifies 89 representative parse/resolution cases and
11 lifecycle cases, generated from the recorded TS revision. CI runs the TS
generators live with `--check` before testing Rust. Native integration tests
additionally cover plugin failures, suspension, cancellation, and host behavior. This is a
behavioral compatibility baseline; it does not certify every TypeScript test case
or identical text formatting.

## Rust API choices

| TypeScript surface                      | Rust surface                                                         |
| --------------------------------------- | -------------------------------------------------------------------- |
| Inferred object keys                    | Bound input bundles, or named typed handles with distinct surfaces   |
| `undefined`                             | `Option<T>`                                                          |
| Required/defaulted inference            | Presence marker types; `get` returns `T` or `Option<T>`              |
| `number`                                | `f64` for compatibility, or any native integer through `parse::<T>`  |
| String paths                            | `PathBuf`                                                            |
| Millisecond numbers                     | `std::time::Duration`                                                |
| Byte size numbers                       | `u64`, rounded and range checked                                     |
| JavaScript `URL` / `Date`               | `url::Url` / `chrono::DateTime<Utc>`                                 |
| Standard Schema / custom callbacks      | `FromStr`, native decoder closures, and typed validators             |
| Context object intersections            | Typed owned state transformations, or a dynamic type-indexed store   |
| Runtime-specific Node/Bun/Deno adapters | `Runtime`, `FileSystem`, `ConfigReader`, `PromptEngine`, and `Write` |
| `package.json` discovery                | Explicit metadata or `cargo_manifest!()`                             |
| Process exit calls                      | Return `std::process::ExitCode` from `main`                          |

Rust supports synchronous callbacks and native async closures. Async applications
await an async runner using their own executor. Borrowing local futures need no
Send bound; callback captures remain Send + Sync. Synchronous middleware cannot
wrap an async continuation and is rejected before execution; async wrappers can
await synchronous tails. Config, prompts, filesystem preparation, and stdio
remain synchronous. Writer-backed TTY spinners own a small standard
library timer thread that stops and joins on completion or `Drop`; no executor
is required. Progress handles render on explicit updates. A shared terminal line
keeps logs and tables readable during activities. Transient frames are excluded
from captures; logical activity events remain available in every output mode.
Terminal prompts
use line input; secret inputs use a native password reader rather than a JavaScript
terminal widget implementation.

`Command::inputs` binds registered inputs to borrowed handler parameters and
owned typed state. Its `Scope` has no arbitrary handle or type-map accessor.
Each derive explicitly transforms its previous state; tuples or structs retain
earlier state when needed. Dynamic `ctx.get()` still looks up surface/name and
checks the native type at runtime. Dynamic `ctx.state()` checks availability at
runtime and replaces same-type values on insertion. These dynamic checks are
distinct from the static guarantees of bound callbacks.

TS capture omits a finish event when framework cleanup follows an action error.
Rust's activity guard emits a failed finish on Drop. The lifecycle fixture test
explicitly asserts this stronger native cleanup behavior. TS's unanswered
`PROMPT_CANCEL` sentinel corresponds to Rust `Answer::Skip`, which allows default
fallback. Rust `Answer::Cancel` represents an abort and exits 130.

`array()` has an empty native default. An explicit `.optional()` clears the
default and represents absence as `None`. This follows Rust's presence types;
the TypeScript resolver supplies empty collections even for its optional collection
declarations.

Collections use `ValueParser<T>` elements, declared through `value::string()`,
`value::parse::<T>()`, and related factories. These parsers carry value constraints
and optional encoding, while source bindings and defaults belong to the input.
`InputSchema` exposes a read-only declaration and keeps codecs private. Collection
splitting and uniqueness methods are restricted to the appropriate native types.

Framework-generated sensitive diagnostics and debug views are redacted. Accessing
an input inside a handler returns its full native value. `Resolved::json()` redacts
sensitive values and omits opaque custom values, so its diagnostic representation
does not require application types to implement Serde.

Common `FromStr` primitive types have automatic JSON encoding and schema metadata.
Custom application types can opt in with `encode_json` and `json_schema`; opaque
types remain supported. Default descriptions control help independently of the
runtime default. JSON help uses the Rust definition format below.

Filesystem checks require an injected capability during process-free execution.
The default policy records skipped checks; `Required` fails and `Disabled`
explicitly opts out. Native execution supplies the filesystem automatically.

Filesystem capabilities use `&Path`, preserving native path names without JSON
or UTF-8 conversion. Checks apply to opaque and non-Unicode native defaults too.
A scalar positional default becomes a one-element vector when converted with
`.variadic()`, preserving the required/defaulted accessor guarantee.

## Optional integrations

The public crate defaults to `completion`, `terminal-prompts`, and
`terminal-activity`. `regex`, `url`, and `chrono` are independent opt-in features;
their built-in factories and constraints are available only when enabled. Core
has no default integrations. Output defaults to its terminal backend when used
on its own, and the public crate forwards that feature explicitly. All internal
core/output dependencies disable defaults so a minimal facade build stays minimal.

Disabling a backend preserves its abstraction: injected prompt engines still
resolve values, and activity handles still record lifecycle events and clean up
on Drop. Without `terminal-activity`, handles emit no activity text or frames and
no timer code is compiled. Duration and byte-size parsing use decimal scanners;
Chrono's ISO date parser does not depend on regex constraints. Config, provenance,
middleware, validation, typed/async execution, and schema exports remain available
in every configuration. Serde and JSON remain mandatory representation choices.

`scripts/check-features.py` verifies actual normal dependency graphs and behavior
for minimal, default, all-features, and each isolated integration. Completion
generation alone does not depend on regex, URL, or Chrono; output alone does not
depend on parsing, resolution, or completion.

## Schema documents and formats

The Rust definition document uses `format: "fujo-rust"` and `version: 1`.
The Fujo rename changes the former `dreamcli-rust` wire-format discriminator;
the format version remains `1`. Consumers matching the old discriminator must
update to `fujo-rust`.
It describes Rust input metadata and excludes executable callbacks. It is not the
TypeScript definition document wire format and does not reconstruct custom Rust
types or handlers. Input schemas use JSON Schema draft 2020-12 for the JSON
representation of supported values.

CLI arguments must be valid UTF-8; invalid OS arguments produce a structured parse
error. String patterns use Rust's `regex` syntax. Help and terminal formatting
use native renderers, so spacing, wrapping, and decorative output may differ from the
TypeScript renderers. Error categories, stable codes, exit statuses, and value
resolution are the compatibility surfaces tested here.
