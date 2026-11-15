# fujo-core

`fujo-core` defines typed inputs, parses arguments, and resolves values for Fujo.
Use it directly when you need schemas or input resolution without command execution.
For command handlers and application setup, use [fujo](https://docs.rs/fujo).

## Install

```toml
[dependencies]
fujo-core = "0.2.0"
```

This crate requires Rust 1.99 or newer. It has no default features.

## Parse and resolve inputs

```rust
use fujo_core::command::CommandSchema;
use fujo_core::parse::{ParseOptions, parse};
use fujo_core::resolve::{ResolveOptions, resolve};
use fujo_core::{arg, flag};

let name = arg::string("name");
let jobs = flag::parse::<u16>("jobs").env("BUILD_JOBS").default(4);
let command = CommandSchema::new("build")
    .arg(name.clone().into_schema())
    .flag(jobs.clone().into_schema());
let argv = vec!["demo".to_owned(), "--jobs".to_owned(), "8".to_owned()];
let parsed = parse(&command, &argv, ParseOptions::default())?;
let values = resolve(&command, &parsed, &ResolveOptions::default())?;

assert_eq!(values.get(&name)?, "demo");
assert_eq!(values.get(&jobs)?, 8);
# Ok::<(), fujo_core::CliError>(())
```

`parse()` accepts arguments without the executable name. `parse_os()` accepts
OS strings and preserves non-UTF-8 path arguments on supported platforms.

Resolution checks CLI arguments, stdin, environment, configuration, prompts,
and defaults, in that order. Pass source data through `ResolveOptions`.
The resolver reads the environment map you supply. Inject a `PromptEngine` or
`FileSystem` when resolution needs prompts or filesystem checks.

`Resolved::get()` returns owned values. `get_ref()` borrows values and supports
types without `Clone`. `Resolved::sources` records each value's source.

## Schema metadata

`CommandSchema` and `ProgramSchema` describe inputs, commands, and built-ins.
Call `ProgramSchema::validate()` to check a program before using its metadata.
`ProgramSchema::new()` starts without built-ins. Enable the ones your application
supports.

[fujo-complete](https://docs.rs/fujo-complete) generates shell completions from
these schemas. The `json_schema` module exports JSON Schema for configuration.

## Cargo features

| Feature  | Effect                                                |
| -------- | ----------------------------------------------------- |
| `regex`  | Adds string pattern constraints.                      |
| `url`    | Adds inputs that return `url::Url`.                   |
| `chrono` | Adds date inputs that return `chrono::DateTime<Utc>`. |

Duration and byte-size inputs work without optional features.

## Documentation

- [API reference](https://docs.rs/fujo-core)
- [Fujo guide](https://github.com/kjanat/fujo#readme)

## License

MIT. See [LICENSE](https://github.com/kjanat/fujo/blob/master/LICENSE).
