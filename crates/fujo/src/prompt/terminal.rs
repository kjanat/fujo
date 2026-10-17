use super::*;
use crate::CliError;
use crate::Output;
use crate::Result;
use serde_json::Value;
use serde_json::json;
use std::fmt::Write;
use std::io::{self};

#[derive(Debug, Clone)]
pub struct TerminalPrompter {
    output: Output,
}
impl TerminalPrompter {
    pub fn new(output: Output) -> Self {
        Self { output }
    }
}
fn cancelled() -> CliError {
    CliError::new("PROMPT_CANCELLED", "Prompt cancelled").exit_code(130)
}
fn secret_answer(read: io::Result<String>) -> Result<String> {
    match read {
        Ok(answer) => Ok(answer),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(cancelled()),
        Err(error) => Err(error.into()),
    }
}
fn render(prompt: &Prompt) -> String {
    let mut text = format!("{}\n", prompt.message);
    if prompt.kind == PromptKind::Confirm {
        let _ = writeln!(
            text,
            "{}",
            if prompt
                .default
                .as_ref()
                .and_then(Value::as_bool)
                .unwrap_or(true)
            {
                "(Y/n)"
            } else {
                "(y/N)"
            }
        );
    } else if let Some(default) = &prompt.default
        && !prompt.secret
    {
        let _ = writeln!(text, "(default: {})", default.as_str().unwrap_or(""));
    }
    for (i, choice) in prompt.choices.iter().enumerate() {
        let _ = writeln!(text, "  {}. {}", i + 1, choice.label);
    }
    text.push_str("> ");
    text
}
impl PromptEngine for TerminalPrompter {
    fn prompt(&self, prompt: &Prompt) -> Result<Option<Value>> {
        self.output.try_raw_stderr(&render(prompt))?;
        let answer = if prompt.secret {
            secret_answer(rpassword::read_password())?
        } else {
            let mut answer = String::new();
            if io::stdin().read_line(&mut answer)? == 0 {
                return Err(cancelled());
            }
            crate::resolve::strip_terminator(&answer).to_owned()
        };
        let choice = |answer: &str| -> Result<Value> {
            if let Ok(index) = answer.parse::<usize>()
                && let Some(choice) = index
                    .checked_sub(1)
                    .and_then(|index| prompt.choices.get(index))
            {
                return Ok(choice.value.clone());
            }
            prompt
                .choices
                .iter()
                .find(|choice| choice.label == answer || choice.value.as_str() == Some(answer))
                .map(|choice| choice.value.clone())
                .ok_or_else(|| {
                    CliError::validation("CONSTRAINT_VIOLATED", "Choose an available option")
                })
        };
        match prompt.kind {
            PromptKind::Input => Ok(Some(json!(answer))),
            PromptKind::Confirm => match answer.to_lowercase().as_str() {
                "y" | "yes" | "true" | "1" => Ok(Some(json!(true))),
                "n" | "no" | "false" | "0" => Ok(Some(json!(false))),
                "" => Ok(Some(json!(""))),
                _ => Err(CliError::validation(
                    "CONSTRAINT_VIOLATED",
                    "Answer yes or no",
                )),
            },
            PromptKind::Select => choice(&answer).map(Some),
            PromptKind::Multiselect => answer
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| choice(s.trim()))
                .collect::<Result<Vec<_>>>()
                .map(|v| Some(json!(v))),
        }
    }
    fn retry(&self, error: &CliError) -> bool {
        self.output
            .try_raw_stderr(&format!("{}\n", error.message))
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::Choice;

    #[test]
    fn secret_eof_is_a_cancelled_prompt() {
        let error = secret_answer(Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "unexpected end of file",
        )))
        .unwrap_err();
        assert_eq!(error.code, "PROMPT_CANCELLED");
        assert_eq!(error.exit_code, 130);
        assert_eq!(secret_answer(Ok("hunter2".into())).unwrap(), "hunter2");
        assert_eq!(
            secret_answer(Err(io::Error::other("tty gone")))
                .unwrap_err()
                .code,
            "IO_ERROR"
        );
    }

    #[test]
    fn a_failed_prompt_write_stops_before_reading() {
        struct Closed;
        impl io::Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("stderr closed"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let output = Output::with_writers(io::sink(), Closed, Default::default());
        let prompter = TerminalPrompter::new(output);
        assert_eq!(
            prompter.prompt(&Prompt::input("Name?")).unwrap_err().code,
            "IO_ERROR"
        );
        assert!(!prompter.retry(&CliError::validation("BAD", "Answer yes or no")));
    }

    #[test]
    fn prompts_render_through_the_shared_output() {
        assert_eq!(render(&Prompt::input("Name?")), "Name?\n> ");
        assert_eq!(
            render(&Prompt::input("Name?").default("Ada")),
            "Name?\n(default: Ada)\n> "
        );
        assert_eq!(
            render(&Prompt::input("Token?").default("x").secret()),
            "Token?\n> "
        );
        assert_eq!(render(&Prompt::confirm("Go?")), "Go?\n(Y/n)\n> ");
        assert_eq!(
            render(&Prompt::confirm("Go?").default(false)),
            "Go?\n(y/N)\n> "
        );
        assert_eq!(
            render(&Prompt::select(
                "Pick",
                [Choice::new("a", "Alpha"), Choice::new("b", "Beta")]
            )),
            "Pick\n  1. Alpha\n  2. Beta\n> "
        );
        let output = Output::capture(Default::default());
        let prompter = TerminalPrompter::new(output.clone());
        assert!(prompter.retry(&CliError::validation("BAD", "Answer yes or no")));
        assert_eq!(output.captured().stderr, "Answer yes or no\n");
    }
}
