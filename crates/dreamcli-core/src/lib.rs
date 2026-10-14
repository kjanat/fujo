//! Runtime-independent input schemas, typed values, parsing, and resolution.
pub mod command;
pub use dreamcli_error as error;
pub mod json_schema;
pub mod parse;
pub mod prompt;
pub mod resolve;
pub mod schema;
pub mod value;
pub use error::CliError;
pub use error::Result;
pub use schema::Input;
pub use schema::Optional;
pub use schema::Required;
pub use schema::arg;
pub use schema::flag;
