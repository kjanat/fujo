//! Schema-first CLI construction, typed inputs, and injectable execution.
//!
//! Defaults enable `completion`, `terminal-prompts`, and `terminal-activity`.
//! Specialized `regex`, `url`, and `chrono` integrations are opt-in. Disabling
//! native prompt/activity backends preserves injected prompts and logical events.
//!
//! ```
//! use fujo::{cli, command, arg, flag};
//! let name = arg::string("name");
//! let loud = flag::boolean("loud").alias("l");
//! let greet = command("greet")
//!     .inputs((name, loud))
//!     .action(|ctx, (name, loud), ()| {
//!         let message = format!("Hello, {name}!");
//!         ctx.out.log(if *loud { message.to_uppercase() } else { message });
//!         Ok(())
//!     });
//! let result = cli("greet").default(greet).execute(["Ada"], Default::default());
//! assert_eq!(result.stdout, "Hello, Ada!\n");
//! ```
//!
//! Required and optional inputs have different accessor return types:
//! ```compile_fail
//! use fujo::{Resolved, flag};
//! let optional = flag::string("name");
//! let value: String = Resolved::default().get(&optional).unwrap();
//! ```
//!
//! Flags and positional arguments also have distinct types:
//! ```compile_fail
//! use fujo::{Command, arg};
//! let command = Command::new("demo").flag(arg::string("name"));
//! ```
//!
//! The richer `release`, `batch`, and `settings` examples retain native types in
//! handlers. Incorrect assumptions are rejected by the compiler:
//!
//! ```compile_fail
//! use fujo::{Command, flag};
//! use std::num::NonZeroU16;
//! let workers = flag::parse::<NonZeroU16>("workers").required();
//! let command = Command::new("batch").flag(workers.clone()).action(move |ctx| {
//!     let workers: String = ctx.get(&workers)?;
//!     Ok(())
//! });
//! ```
//!
//! ```compile_fail
//! use fujo::{Command, arg};
//! let artifact = arg::path("artifact");
//! let command = Command::new("release").arg(artifact.clone()).action(move |ctx| {
//!     let artifact: String = ctx.get(&artifact)?;
//!     Ok(())
//! });
//! ```
//!
//! ```compile_fail
//! use fujo::{Command, flag, value};
//! let ports = flag::array("ports", value::parse::<u16>());
//! let command = Command::new("settings").flag(ports.clone()).action(move |ctx| {
//!     let ports: Vec<String> = ctx.get(&ports)?;
//!     Ok(())
//! });
//! ```
//!
//! ```compile_fail
//! use fujo::flag;
//! let port = flag::parse::<u16>("port").default("8080");
//! ```
pub mod cli;
pub mod command;
mod execution;
#[cfg(feature = "completion")]
pub use fujo_complete as completion;
pub mod config;
pub use fujo_core::error;
pub mod help;
pub use fujo_core::json_schema;
pub use fujo_core::parse;
pub use fujo_output as output;
pub mod prompt;
pub use fujo_core::resolve;
pub mod runtime;
pub use fujo_core::schema;
pub mod testkit;
pub mod typed;
pub use typed::AsyncDerive;
pub use typed::AsyncMiddleware;
pub use typed::Chain;
pub use typed::Derive;
pub use typed::InputSet;
pub use typed::Middleware;
pub use typed::Pipeline;
pub use typed::Scope;
pub use typed::StageInfo;
pub use typed::StageKind;
pub use typed::TypedCommand;
pub use typed::pipeline;

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
pub use execution::AsyncNext;
pub use execution::LocalFuture;
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
pub use schema::value;
pub use schema::value::ValueParser;
pub use serde_json::Value;
pub use serde_json::json;
