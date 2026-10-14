//! Schema-first CLI construction, typed inputs, and injectable execution.
//!
//! ```
//! use dreamcli::{cli, command, arg, flag};
//! let name = arg::string("name");
//! let loud = flag::boolean("loud").alias("l");
//! let greet = command("greet")
//!     .arg(name.clone()).flag(loud.clone())
//!     .action(move |ctx| {
//!         let message = format!("Hello, {}!", ctx.get(&name)?);
//!         ctx.out.log(if ctx.get(&loud)? { message.to_uppercase() } else { message });
//!         Ok(())
//!     });
//! let result = cli("greet").default(greet).execute(["Ada"], Default::default());
//! assert_eq!(result.stdout, "Hello, Ada!\n");
//! ```
//!
//! Required and optional inputs have different accessor return types:
//! ```compile_fail
//! use dreamcli::{Resolved, flag};
//! let optional = flag::string("name");
//! let value: String = Resolved::default().get(&optional).unwrap();
//! ```
//!
//! Flags and positional arguments also have distinct types:
//! ```compile_fail
//! use dreamcli::{Command, arg};
//! let command = Command::new("demo").flag(arg::string("name"));
//! ```
pub mod cli;
pub mod command;
pub use dreamcli_complete as completion;
pub mod config;
pub use dreamcli_core::error;
pub mod help;
pub use dreamcli_core::json_schema;
pub use dreamcli_core::parse;
pub use dreamcli_output as output;
pub mod prompt;
pub use dreamcli_core::resolve;
pub mod runtime;
pub use dreamcli_core::schema;
pub mod testkit;

pub use cli::Builtins;
pub use cli::Cli;
pub use cli::Plugin;
pub use cli::cli;
pub use command::Command;
pub use command::Context;
pub use command::Meta;
pub use command::Next;
pub use command::command;
pub use command::group;
pub use error::CliError;
pub use error::ErrorKind;
pub use error::Result;
pub use output::Output;
pub use output::OutputOptions;
pub use output::Verbosity;
pub use resolve::InputSources;
pub use resolve::Provenance;
pub use resolve::ResolveOptions;
pub use resolve::Resolved;
pub use resolve::Stage;
pub use resolve::was_explicit;
pub use runtime::RunOptions;
pub use runtime::RunResult;
pub use schema::Arg;
pub use schema::Flag;
pub use schema::Input;
pub use schema::Optional;
pub use schema::Required;
pub use schema::arg;
pub use schema::flag;
pub use serde_json::Value;
pub use serde_json::json;
