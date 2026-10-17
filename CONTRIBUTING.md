# Contributing

Use the Rust 1.99 toolchain selected by `rust-toolchain.toml`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --no-default-features
cargo test --workspace --all-features
python3 scripts/check-features.py
cargo doc --workspace --all-features --no-deps
```

The `shell-tests` feature runs external completion tests. It requires Bash, Zsh,
Fish, PowerShell (`pwsh`), and Python 3 (`python3`) on PATH. The Zsh line-editor
checks use Unix PTYs through Python's standard library. Ordinary workspace tests
use no external shell dependencies. `--all-targets` also compiles and tests the examples; run the
doctests with ordinary `cargo test` because `--all-targets` excludes them.

The CI workflow runs these core gates on Linux, macOS, and Windows. A separate
Linux job installs the completion shells and verifies actual timed PTY frames.
The feature job checks minimal builds and each isolated integration. Its Python
checker compiles and tests nine feature configurations, inspects normal dependency
graphs across all target platforms, and tests standalone output and completion
crates. Dev dependencies are excluded from the dependency assertions.
The live-reference job checks out the TS commit recorded by `reference.json`,
installs its locked dependencies, and checks both fixture generators without
rewriting their files. The setup action reads `rust-toolchain.toml` so toolchain
updates do not require editing workflow versions.

## Try the examples

The [example CLI guide](crates/fujo/examples/README.md) lists the run and test
commands for every example. On Unix, verify that the terminal and async build
examples deliver frames over time through an actual PTY, that logs and tables
stay intact, and that JSON, quiet, and piped modes contain no animation controls:

```sh
cargo build -p fujo --example terminal --example async_build
python3 scripts/check-terminal.py target/debug/examples/terminal target/debug/examples/async_build
```

The checker uses only Python's standard library. Its optional first argument
selects the terminal binary; its optional second argument enables checks for
the async build binary. Explicit paths also support `CARGO_TARGET_DIR`.

## Reference fixtures

The checked-in `reference.json` records 94 parse/resolution cases generated from
the sibling TypeScript checkout at the commit stored in the fixture. Tests compare
values, provenance, error codes, and exit codes. Date formatting and JSON numeric
representations are normalized while preserving their values.

`lifecycle.json` records 11 orchestration cases: middleware ordering, short
circuits, derive/action failures, error transformation, explicit exit codes,
activity cleanup, JSON output, and unanswered prompts. Its generator requires
the same reference commit. Rust's Drop finish event is explicitly covered as a
native difference. Separate native tests cover aborts and dropped futures,
plugin hooks, writer failures, and typed state ownership.

Verify against a live sibling checkout without changing fixtures:

```sh
bun scripts/reference-fixtures.ts --check
bun scripts/lifecycle-fixtures.ts --check
cargo test -p fujo --features regex,url,chrono --test reference --test lifecycle_reference
```

Regenerate with the TypeScript checkout at `../dreamcli` and its dependencies
installed:

```sh
bun scripts/reference-fixtures.ts
bun scripts/lifecycle-fixtures.ts
cargo test -p fujo --features regex,url,chrono --test reference --test lifecycle_reference
```

The generator reads the sibling checkout and writes only this repository's
fixture. Review differences against the recorded source commit before accepting
new expectations. Running the normal Rust test suite does not require Bun or the
TypeScript checkout.

Keep OS access in the public crate's `Runtime` implementations. Core resolution
receives explicit capabilities. Native values live behind typed handles; JSON is
used for config, prompt interchange, and schema metadata. Keep output independent
of core parsing and resolution.
