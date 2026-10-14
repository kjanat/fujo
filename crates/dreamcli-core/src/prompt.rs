use crate::CliError;
use crate::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub value: Value,
    pub label: String,
}
impl Choice {
    pub fn new(value: impl Into<Value>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptKind {
    Input,
    Confirm,
    Select,
    Multiselect,
}
#[derive(Debug, Clone, Serialize)]
pub struct Prompt {
    pub kind: PromptKind,
    pub message: String,
    pub choices: Vec<Choice>,
    pub secret: bool,
}
impl Prompt {
    pub fn input(message: impl Into<String>) -> Self {
        Self {
            kind: PromptKind::Input,
            message: message.into(),
            choices: vec![],
            secret: false,
        }
    }
    pub fn confirm(message: impl Into<String>) -> Self {
        Self {
            kind: PromptKind::Confirm,
            ..Self::input(message)
        }
    }
    pub fn select(message: impl Into<String>, choices: impl IntoIterator<Item = Choice>) -> Self {
        Self {
            kind: PromptKind::Select,
            choices: choices.into_iter().collect(),
            ..Self::input(message)
        }
    }
    pub fn multiselect(
        message: impl Into<String>,
        choices: impl IntoIterator<Item = Choice>,
    ) -> Self {
        Self {
            kind: PromptKind::Multiselect,
            choices: choices.into_iter().collect(),
            ..Self::input(message)
        }
    }
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }
}
/// Hosts decide how to render and collect an answer. `None` skips this source.
pub trait PromptEngine: Send + Sync {
    fn prompt(&self, prompt: &Prompt) -> Result<Option<Value>>;
}
#[derive(Debug, Clone)]
pub enum Answer {
    Value(Value),
    Skip,
    Cancel,
}
impl<T: Into<Value>> From<T> for Answer {
    fn from(value: T) -> Self {
        Self::Value(value.into())
    }
}
#[derive(Debug, Default)]
pub struct TestPrompter {
    answers: Mutex<VecDeque<Answer>>,
    prompts: Mutex<Vec<Prompt>>,
}
impl TestPrompter {
    pub fn new(answers: impl IntoIterator<Item = Answer>) -> Self {
        Self {
            answers: Mutex::new(answers.into_iter().collect()),
            prompts: Mutex::new(vec![]),
        }
    }
    pub fn prompts(&self) -> Vec<Prompt> {
        self.prompts.lock().expect("prompt lock").clone()
    }
}
impl PromptEngine for TestPrompter {
    fn prompt(&self, prompt: &Prompt) -> Result<Option<Value>> {
        self.prompts
            .lock()
            .expect("prompt lock")
            .push(prompt.clone());
        match self.answers.lock().expect("answer lock").pop_front() {
            Some(Answer::Value(value)) => Ok(Some(value)),
            Some(Answer::Skip) => Ok(None),
            Some(Answer::Cancel) => {
                Err(CliError::new("PROMPT_CANCELLED", "Prompt cancelled").exit_code(130))
            }
            None => Err(CliError::new(
                "MISSING_PROMPT_ANSWER",
                "No prompt answer supplied",
            )),
        }
    }
}
