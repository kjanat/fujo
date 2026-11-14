# Contributing

Use the toolchain pinned in `rust-toolchain.toml`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --no-default-features
cargo test --workspace --all-features
python3 scripts/check-features.py
cargo doc --workspace --all-features --no-deps
```

The `shell-tests` feature requires Bash, Zsh, Fish, PowerShell, and Python 3 on
PATH as `bash`, `zsh`, `fish`, `pwsh`, and `python3`. Zsh line-editor tests use
Unix PTYs. Tests without `shell-tests` need no external shells. `--all-targets`
includes examples but excludes doctests, so also run ordinary `cargo test`.

CI runs core checks on Linux, macOS, and Windows. Separate Linux jobs test shell
completions and terminal frame timing, feature combinations, reference behavior,
and crate packaging. The feature checker builds nine configurations and checks
normal dependencies across all targets, excluding development dependencies. It
also tests the output and completion crates separately.

The reference job checks out the DreamCLI commit recorded in `reference.json`,
installs its locked dependencies, and checks the fixture generators. The setup
action reads the Rust version from `rust-toolchain.toml`.

## Try the examples

See the [example CLI guide](crates/fujo/examples/README.md) for commands. On Unix,
check terminal animation through a PTY:

```sh
cargo build -p fujo --example terminal --example async_build
python3 scripts/check-terminal.py target/debug/examples/terminal target/debug/examples/async_build
```

The checker verifies frame timing, logs, tables, and suppression of animation in
JSON, quiet, and piped output. It uses Python's standard library. The first
optional argument selects the terminal binary. The second selects the async
build binary and enables its checks. Pass explicit paths with a custom
`CARGO_TARGET_DIR`.

## Reference fixtures

`reference.json` contains 94 parsing and resolution cases from the recorded
DreamCLI revision. Tests compare values, sources, error codes, and exit codes.
They normalize date formatting and JSON number representations.

`lifecycle.json` contains 11 cases for middleware order, early returns,
derive and action failures, error replacement, exit codes, activity cleanup,
JSON output, and unanswered prompts. Both generators require the same reference
commit. The tests account for Rust's activity-finish event on drop. Separate
Rust tests cover cancellation, dropped futures, plugin hooks, writer failures,
and state ownership.

Check a sibling DreamCLI checkout without changing fixtures:

```sh
bun scripts/reference-fixtures.ts --check
bun scripts/lifecycle-fixtures.ts --check
cargo test -p fujo --features regex,url,chrono --test reference --test lifecycle_reference
```

To regenerate fixtures, install the dependencies in `../dreamcli`, then run:

```sh
bun scripts/reference-fixtures.ts
bun scripts/lifecycle-fixtures.ts
cargo test -p fujo --features regex,url,chrono --test reference --test lifecycle_reference
```

Generators read the sibling checkout and write Fujo's fixtures. Review changes
against the recorded source commit before accepting new expectations. Ordinary
Rust tests use the checked-in fixtures and need neither Bun nor DreamCLI.

Keep OS access in the public crate's `Runtime` implementations. Pass capabilities
into core resolution. Store native values behind typed handles and use JSON for
configuration, prompts, and schema metadata. Keep output independent of parsing
and resolution.

## Publishing to crates.io

Beepboop prepares manifest and lockfile updates in a version PR. Merging the PR
starts publication after its checks pass. `.github/workflows/publish.yml` accepts
Beepboop dispatches and publishes the five crates in dependency order. After
publication, Beepboop creates the signed `v<version>` tag and publishes the
GitHub Release with generated notes. The operation requires both the crates
and the GitHub Release to complete.

The workflow checks the release version against the workspace, each publishable
crate, and internal dependency requirements. It checks that the source belongs
to `master` and matches the commit used by CI. The full CI suite, including
packaging, must pass before publication. Publishing runs execute one at a time.

`actions/publish` retrieves the stored `crates-io` credential through Beepboop
and supplies it to Cargo. The `main` stream declares the alias in
`.github/beepboop.json`. Grant the publishing operation access after that
configuration reaches `master`:

```sh
beepboop secrets grant kjanat/fujo crates-io --stream main --operation promote --delivery runtime
```

The token needs permission to publish the selected crates, including new crates
for their first release. Beepboop's signing service supplies the signed tag object
and its tagger identity.

Release validation and its tests live in `.github/actions/check-release/`.
The composite action finds its Python files through `GITHUB_ACTION_PATH` and
validates the selected Cargo workspace.

For a release, start preparation in Beepboop, review the version PR and its CI,
then merge it. Check the **Publish crates** run and the resulting registry
versions. For local validation, run:

```sh
python3 .github/actions/check-release/check-release.py --tag vVERSION
cargo publish --workspace --dry-run --locked --all-features --registry crates-io
```

Each crate publishes separately. If a run fails, inspect Beepboop's publication
records and crates.io before retrying. Published versions are immutable. Keep
release tags on their original commits.
