# fujo-error

`fujo-error` defines the structured errors shared by Fujo's crates.
`CliError` carries an error category, code, message, exit code, optional
suggestion, and optional JSON details.

## Install

```toml
[dependencies]
fujo-error = "0.2.0"
```

This crate requires Rust 1.99 or newer. It has no Cargo feature flags.

## Create an error

```rust
use fujo_error::{CliError, ErrorKind};

let error = CliError::validation("INVALID_PORT", "Port must be greater than zero")
    .suggest("Choose a port between 1 and 65535");

assert_eq!(error.name, ErrorKind::Validation);
assert_eq!(error.exit_code, 2);
assert_eq!(error.to_string(), "Port must be greater than zero");
```

Use `CliError::new()` for a general CLI error with exit code 1.
`CliError::parse()` and `CliError::validation()` use exit code 2.
`.exit_code()` overrides the exit code. `.details()` attaches a `serde_json::Value`.

`CliError::aggregate()` combines multiple errors into one validation error with
nested details. A single error passes through unchanged.

## Error handling

`Result<T>` is an alias for `std::result::Result<T, CliError>`.
`CliError` implements `Display` and `std::error::Error`.
Conversions from `std::io::Error` and `serde_json::Error` use the codes
`IO_ERROR` and `INVALID_JSON`.

`CliError` and `ErrorKind` support Serde serialization and deserialization.
Serialized errors use camelCase field names. Error categories serialize as
`CLIError`, `ParseError`, and `ValidationError`.

## Documentation

- [API reference](https://docs.rs/fujo-error)
- [Fujo guide](https://github.com/kjanat/fujo#readme)

## License

MIT. See [LICENSE](https://github.com/kjanat/fujo/blob/master/LICENSE).
