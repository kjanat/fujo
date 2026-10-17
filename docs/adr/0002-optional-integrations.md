# Optional integrations within the existing crates

Status: Implemented for review.

## Decision

Keep the five-crate workspace and the integrated resolution contract. The facade
defaults to `completion`, `terminal-prompts`, and `terminal-activity`, with
`regex`, `url`, and `chrono` as independent opt-in integrations. Core has no
default features; standalone output defaults to its terminal backend. Internal
core/output dependencies disable defaults and forward features explicitly.
Features only add capabilities, as required by Cargo feature unification.

Gate specialized factories, constraints, and the `Url` and `Date` kinds
together with their dependencies. A build without a feature cannot declare its
inputs, so every declared constraint is enforced.
Use small decimal scanners for duration/size values and a separate ISO shape
check for Chrono, so neither integration needs the regex feature.

Move terminal prompts and terminal activities into feature-gated implementation
modules. Keep `PromptEngine`, scripted answers, spinner/progress handles, logical
events, and Drop cleanup available in every build. Disabling terminal activities
removes the timer and renderer, including final activity text. Other output,
including logs and tables, remains usable. Disabling native prompts leaves the
resolver's injected prompt stage intact.

Keep Serde and JSON mandatory: config, prompt interchange, codecs, and metadata
currently share those representations. Schema exports stay available. Do not
introduce flags for individual resolution stages or a metadata-only crate until
a consumer demonstrates that need. Output remains independently adoptable without
the parser; completion's core dependency brings no specialized integrations by
default. No executor or additional runtime dependency is introduced.

## Verification

The feature checker compiles minimal, default, all-features, and each of the six
isolated integrations, runs behavior contracts, and asserts normal dependency
graphs for all targets. It also tests standalone minimal output and completion
crates. Workspace gates cover default/minimal/all-features tests, strict Clippy,
docs, live TS fixtures, external completion shells, and real PTY animation.

## Consequences

Applications using `.pattern()`, URL factories, or date factories must enable
the corresponding integration. Release/settings examples require `url`; Cargo
example metadata makes this explicit. Features are unified by Cargo, so defaults
disabled by one application dependency may still be enabled by another. Minimal
builds retain the same input precedence, validation, provenance, middleware,
typed state, async execution, injected capabilities, and capture semantics.
