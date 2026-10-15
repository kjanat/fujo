//! In-process harness uses exactly the same executor as native CLI runs.
use crate::Builtins;
use crate::Cli;
use crate::Command;
use crate::RunOptions;
use crate::RunResult;
use crate::cli;
pub use crate::output::Capture;
pub use crate::output::Output;
pub use crate::prompt::Answer;
pub use crate::prompt::TestPrompter;

pub fn run_command(
    command: Command,
    argv: impl IntoIterator<Item = impl Into<String>>,
    options: RunOptions,
) -> RunResult {
    run_command_with_builtins(command, argv, options, Builtins::default())
}
pub fn run_command_with_builtins(
    command: Command,
    argv: impl IntoIterator<Item = impl Into<String>>,
    options: RunOptions,
    builtins: Builtins,
) -> RunResult {
    cli(command.schema().name.as_str())
        .builtins(builtins)
        .default(command)
        .execute(argv, options)
}
/// Test the complete configured application, including plugins and builtins.
pub fn run_cli(
    app: &Cli,
    argv: impl IntoIterator<Item = impl Into<String>>,
    options: RunOptions,
) -> RunResult {
    app.execute(argv, options)
}
