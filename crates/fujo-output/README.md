# fujo-output

`fujo-output` writes CLI output to supplied writers or captures it in memory.
It provides JSON output, tables, spinners, and progress bars. Use it with Fujo
or another argument parser.

## Install

```toml
[dependencies]
fujo-output = "0.3.0"
```

This crate requires Rust 1.99 or newer. The default `terminal-activity` feature
renders spinners and progress bars in a terminal.

## Capture output

```rust
use fujo_output::{Output, OutputOptions};

let out = Output::capture(OutputOptions::default());
out.log("Build complete");
out.warn("Cache is disabled");
out.finish()?;

let captured = out.captured();
assert_eq!(captured.stdout, "Build complete\n");
assert_eq!(captured.stderr, "Cache is disabled\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

For terminal or file output, pass stdout and stderr writers to
`Output::with_writers()`. It also retains captured text. Call `finish()` to check
for writer errors before reporting success.

## Output modes

`OutputOptions` controls JSON mode, verbosity, terminal rendering, and color.
In JSON mode, human-readable log messages go to stderr. `Output::json()` writes
serialized values to stdout.

`Output::table()` accepts JSON rows and `Column` definitions. Choose
`TableFormat::Pretty`, `Csv`, `Tsv`, or `Json` for the output format.

Spinner and progress handles record activity events. `events()` returns those
events, and `report()` returns both captured text and events. Handles finish when
you complete them or drop them.

## Disable terminal rendering

```toml
[dependencies]
fujo-output = { version = "0.3.0", default-features = false }
```

Spinners and progress handles still record events with `terminal-activity`
disabled. They draw no terminal output. Capture, JSON, and tables remain available.

## Documentation

- [API reference](https://docs.rs/fujo-output)
- [Fujo output guide](https://github.com/kjanat/fujo#output-help-and-completion)

## License

MIT. See [LICENSE](https://github.com/kjanat/fujo/blob/master/LICENSE).
