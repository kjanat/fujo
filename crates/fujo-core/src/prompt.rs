use crate::CliError;
use crate::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
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
#[derive(Clone, Serialize)]
pub struct Prompt {
    pub kind: PromptKind,
    pub message: String,
    pub choices: Vec<Choice>,
    pub secret: bool,
    pub default: Option<Value>,
    #[serde(skip)]
    validator: Option<PromptValidator>,
}
type PromptValidator = Arc<dyn Fn(&str) -> Result<()> + Send + Sync>;
impl std::fmt::Debug for Prompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prompt")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .field("secret", &self.secret)
            .finish_non_exhaustive()
    }
}
impl Prompt {
    pub fn input(message: impl Into<String>) -> Self {
        Self {
            kind: PromptKind::Input,
            message: message.into(),
            choices: vec![],
            secret: false,
            default: None,
            validator: None,
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
    pub fn default(mut self, value: impl Into<Value>) -> Self {
        self.default = Some(value.into());
        self
    }
    pub fn validate(
        mut self,
        validate: impl Fn(&str) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.validator = Some(Arc::new(validate));
        self
    }
    pub fn validate_definition(&self) -> Result<()> {
        let valid = self.default.as_ref().is_none_or(|value| match self.kind {
            PromptKind::Input => value.is_string(),
            PromptKind::Confirm => value.is_boolean(),
            _ => false,
        });
        if !valid || (self.validator.is_some() && self.kind != PromptKind::Input) {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Prompt default or validator is incompatible with its kind",
            ));
        }
        Ok(())
    }
    /// Apply blank-answer defaults and validation consistently for every host.
    pub fn normalize_answer(&self, answer: Option<Value>) -> Result<Option<Value>> {
        self.validate_definition()?;
        let Some(answer) = answer else {
            return Ok(None);
        };
        if answer.as_str().is_some_and(|s| s.trim().is_empty()) {
            return match self.kind {
                PromptKind::Input => Ok(self.default.clone()),
                PromptKind::Confirm => Ok(Some(self.default.clone().unwrap_or(Value::Bool(true)))),
                _ => Ok(Some(answer)),
            };
        }
        if let Some(validate) = &self.validator {
            validate(
                answer
                    .as_str()
                    .ok_or_else(|| CliError::validation("TYPE_MISMATCH", "Expected text input"))?,
            )?;
        }
        if !self.choices.is_empty() {
            let values = if self.kind == PromptKind::Multiselect {
                answer
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![answer.clone()])
            } else {
                vec![answer.clone()]
            };
            if values
                .iter()
                .any(|v| !self.choices.iter().any(|choice| choice.value == *v))
            {
                return Err(CliError::validation(
                    "CONSTRAINT_VIOLATED",
                    "Choose an available option",
                ));
            }
        }
        Ok(Some(answer))
    }
}
/// Hosts decide how to render and collect an answer. `None` skips this source.
pub trait PromptEngine: Send + Sync {
    fn prompt(&self, prompt: &Prompt) -> Result<Option<Value>>;
    /// Return true when the engine can collect another answer after rejection.
    fn retry(&self, _error: &CliError) -> bool {
        false
    }
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
    fn retry(&self, _error: &CliError) -> bool {
        !self.answers.lock().expect("answer lock").is_empty()
    }
}
