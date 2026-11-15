# fujo-complete

`fujo-complete` generates Bash, Zsh, Fish, and PowerShell completion scripts
from `fujo-core` schemas. You can use it with Fujo or an existing argument parser.

## Install

```toml
[dependencies]
fujo-complete = "0.3.0"
fujo-core     = "0.3.0"
```

This crate requires Rust 1.99 or newer. It has no Cargo feature flags.

## Generate a script

```rust
use fujo_complete::{Shell, generate_with_options};
use fujo_core::command::{CommandSchema, ProgramSchema};
use fujo_core::parse::ParseOptions;
use fujo_core::{arg, flag};

let program = ProgramSchema::new("formatter")
    .completion_version("1.2.3")
    .default(CommandSchema::new("format")
        .flag(flag::enumeration("profile", ["compact", "pretty"]).into_schema())
        .arg(arg::path("files").variadic().optional().into_schema()));
program.validate()?;

let script = generate_with_options(
    &program,
    Shell::Bash,
    ParseOptions { case_parity: false },
);
assert!(script.contains("formatter"));
# Ok::<(), fujo_core::CliError>(())
```

Write the returned script to a file and load it through your shell's completion
configuration. Choose `Shell::Zsh`, `Shell::Fish`, or `Shell::PowerShell` for
those shells.

Schemas describe commands, flags, aliases, value choices, and paths.
The generator uses that metadata to complete arguments for the current command.

## Match your parser

`generate()` uses Fujo's default parsing options, which accept kebab-case and
camelCase spellings of long options. Use `generate_with_options()` to disable
case parity for a parser that only accepts declared spellings.

`ProgramSchema::new()` starts without built-ins or configuration and completion
commands. Enable those explicitly when your parser supports them.
`.completion_version()` stamps the script header. `.version()` also enables
`--version` and `-V` in the schema.

Use `generate_with_config()` and set
`CompletionOptions::abbreviate_long_options` to `true` if your parser accepts
unique prefixes of long option names. Ambiguous prefixes do not select an option.
Completions suggest full visible names.

For a Fujo application, enable the `fujo` crate's `completion` feature and call
`Cli::completions()` to register its completion command.

## Documentation

- [API reference](https://docs.rs/fujo-complete)
- [Schema builders](https://docs.rs/fujo-core)
- [Completion guide](https://github.com/kjanat/fujo#completion-only-use)

## License

MIT. See [LICENSE](https://github.com/kjanat/fujo/blob/master/LICENSE).
