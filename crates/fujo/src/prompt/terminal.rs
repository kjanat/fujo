use super::*;
use crate::CliError;
use crate::Result;
use serde_json::Value;
use serde_json::json;
use std::io::Write;
use std::io::{self};

/// Line-based native prompts. Secret inputs use the terminal's password reader.
pub struct TerminalPrompter;
impl PromptEngine for TerminalPrompter {
    fn prompt(&self, prompt: &Prompt) -> Result<Option<Value>> {
        let mut stderr = io::stderr().lock();
        writeln!(stderr, "{}", prompt.message)?;
        if prompt.kind == PromptKind::Confirm {
            writeln!(
                stderr,
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
            )?;
        } else if let Some(default) = &prompt.default
            && !prompt.secret
        {
            writeln!(stderr, "(default: {})", default.as_str().unwrap_or(""))?;
        }
        for (i, choice) in prompt.choices.iter().enumerate() {
            writeln!(stderr, "  {}. {}", i + 1, choice.label)?;
        }
        write!(stderr, "> ")?;
        stderr.flush()?;
        let answer = if prompt.secret {
            rpassword::read_password()?
        } else {
            let mut answer = String::new();
            if io::stdin().read_line(&mut answer)? == 0 {
                return Err(CliError::new("PROMPT_CANCELLED", "Prompt cancelled").exit_code(130));
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
        eprintln!("{}", error.message);
        true
    }
}
