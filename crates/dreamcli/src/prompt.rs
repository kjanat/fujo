//! Injectable prompts are always available; native terminal UI is optional.
pub use dreamcli_core::prompt::*;
#[cfg(feature = "terminal-prompts")]
mod terminal;
#[cfg(feature = "terminal-prompts")]
pub use terminal::TerminalPrompter;
