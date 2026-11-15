# Fujo

Fujo builds command-line applications in Rust. Declare typed inputs, then use
their resolved values in a command handler.

## Install

```toml
[dependencies]
fujo = "0.2.0"
```

Fujo requires Rust 1.99 or newer.

## Define a command

```rust,no_run
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

Run the application with `Ada --loud` to print `HELLO, ADA!`.
The handler receives `name` as `&String` and `loud` as `&bool`.
Optional inputs become `Option<&T>`.

Inputs can read CLI arguments, stdin, environment variables, configuration,
prompts, and defaults, in that order. Enable sources on each input declaration.
`ctx.sources()` records where each value came from.

Use `.derive()` to pass typed state between stages and `.middleware()` to wrap
later stages. Async handlers use `.action_async()` and the application's executor.
For tests, `Cli::execute()` accepts arguments and returns captured output and
an exit code.

## Cargo features

The default build enables `completion`, `terminal-prompts`, and
`terminal-activity`. Disable them with `default-features = false`.
Input resolution, typed handlers, async execution, configuration, JSON output,
tables, and output capture remain available.

| Feature             | Effect                                                 |
| ------------------- | ------------------------------------------------------ |
| `completion`        | Generates Bash, Zsh, Fish, and PowerShell completions. |
| `terminal-prompts`  | Adds terminal prompts and password input.              |
| `terminal-activity` | Draws spinners and progress bars in a terminal.        |
| `regex`             | Adds string pattern constraints.                       |
| `url`               | Adds inputs that return `url::Url`.                    |
| `chrono`            | Adds date inputs that return `chrono::DateTime<Utc>`.  |

## Documentation

- [API reference](https://docs.rs/fujo)
- [Full guide](https://github.com/kjanat/fujo#readme)
- [Example applications](https://github.com/kjanat/fujo/tree/master/crates/fujo/examples)

Fujo re-exports input builders from `fujo-core`, output from `fujo-output`,
and errors from `fujo-error`. The `completion` feature re-exports `fujo-complete`.

## License

MIT. See [LICENSE](https://github.com/kjanat/fujo/blob/master/LICENSE).
