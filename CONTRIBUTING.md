# Contributing

Use the Rust 1.99 toolchain selected by `rust-toolchain.toml`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

The `shell-tests` feature runs external completion tests. It requires Bash, Zsh,
Fish, and PowerShell (`pwsh`) on PATH. Ordinary workspace tests use no external
shell dependencies. `--all-targets` also compiles and tests the examples; run the
doctests with ordinary `cargo test` because `--all-targets` excludes them.

## Try the examples

```sh
cargo run -p dreamcli --example greet -- Ada --loud --times 2
cargo run -p dreamcli --example greet -- --help
cargo run -p dreamcli --example greet -- completions bash
cargo run -p dreamcli --example deploy -- db seed --json
```

The deploy example uses `DEPLOY_REGION`, `DEPLOY_TOKEN`, and optional JSON config.
It demonstrates native enums, global flags, typed derived state, and nested
commands. It simulates deployment and does not contact an external service.

## Reference fixtures

The checked-in `reference.json` records 89 parse/resolution cases generated from
the sibling TypeScript checkout at the commit stored in the fixture. Tests compare
values, provenance, error codes, and exit codes. Date formatting and JSON numeric
representations are normalized while preserving their values.

Regenerate with the TypeScript checkout at `../dreamcli` and its dependencies
installed:

```sh
bun scripts/reference-fixtures.ts
cargo test -p dreamcli --test reference
```

The generator reads the sibling checkout and writes only this repository's
fixture. Review differences against the recorded source commit before accepting
new expectations. Running the normal Rust test suite does not require Bun or the
TypeScript checkout.

Keep OS access in the public crate's `Runtime` implementations. Core resolution
receives explicit capabilities. Native values live behind typed handles; JSON is
used for config, prompt interchange, and schema metadata. Keep output independent
of core parsing and resolution.
