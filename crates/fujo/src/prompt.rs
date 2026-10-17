pub use fujo_core::prompt::*;
#[cfg(feature = "terminal-prompts")]
mod terminal;
#[cfg(feature = "terminal-prompts")]
pub use terminal::TerminalPrompter;
