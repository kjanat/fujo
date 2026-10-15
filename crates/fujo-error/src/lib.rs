use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

pub type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    #[serde(rename = "CLIError")]
    Cli,
    #[serde(rename = "ParseError")]
    Parse,
    #[serde(rename = "ValidationError")]
    Validation,
}

/// Stable machine code, human message, process exit code, and optional details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliError {
    pub name: ErrorKind,
    pub code: String,
    pub message: String,
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl CliError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: ErrorKind::Cli,
            code: code.into(),
            message: message.into(),
            exit_code: 1,
            suggest: None,
            details: None,
        }
    }
    pub fn parse(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: ErrorKind::Parse,
            exit_code: 2,
            ..Self::new(code, message)
        }
    }
    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: ErrorKind::Validation,
            exit_code: 2,
            ..Self::new(code, message)
        }
    }
    pub fn suggest(mut self, text: impl Into<String>) -> Self {
        self.suggest = Some(text.into());
        self
    }
    pub fn details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
    pub fn exit_code(mut self, code: i32) -> Self {
        self.exit_code = code;
        self
    }
    /// Aggregate a non-empty set of validation errors, retaining a lone error unchanged.
    pub fn aggregate(first: Self, remaining: impl IntoIterator<Item = Self>) -> Self {
        let mut errors: Vec<_> = std::iter::once(first).chain(remaining).collect();
        if errors.len() == 1 {
            return errors.remove(0);
        }
        let message = format!(
            "Multiple validation errors ({}):\n  - {}",
            errors.len(),
            errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("\n  - ")
        );
        Self::validation(&errors[0].code, message)
            .details(serde_json::json!({ "count": errors.len(), "errors": errors }))
            .suggest("Fix the listed validation errors and retry")
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for CliError {}
impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::new("IO_ERROR", error.to_string())
    }
}
impl From<serde_json::Error> for CliError {
    fn from(error: serde_json::Error) -> Self {
        Self::new("INVALID_JSON", error.to_string())
    }
}
