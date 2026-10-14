//! In-process harness uses exactly the same executor as native CLI runs.
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
    cli(command.schema().name.as_str())
        .default(command)
        .execute(argv, options)
}
