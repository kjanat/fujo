use crate::CliError;
use crate::Result;
use crate::prompt::Prompt;
use crate::resolve::Stage;
use crate::value::NativeValue;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    String,
    Number,
    Boolean,
    Enum,
    Array,
    Count,
    KeyValue,
    Custom,
    Url,
    Path,
    Date,
    Duration,
    Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Flag,
    Arg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StdinWhen {
    Dash,
    Missing,
    DashOrMissing,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StdinConsume {
    Exclusive,
    Broadcast,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct StdinBinding {
    pub when: StdinWhen,
    pub consume: StdinConsume,
    pub trim: bool,
}
impl Default for StdinBinding {
    fn default() -> Self {
        Self {
            when: StdinWhen::DashOrMissing,
            consume: StdinConsume::Exclusive,
            trim: false,
        }
    }
}
impl StdinBinding {
    pub fn reads_dash(self) -> bool {
        self.when != StdinWhen::Missing
    }
    pub fn reads_missing(self) -> bool {
        self.when != StdinWhen::Dash
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "format", content = "delimiter", rename_all = "lowercase")]
pub enum Split {
    Whole,
    Delimiter(String),
    Lines,
    Whitespace,
    Json,
}
impl Split {
    pub fn values(&self, text: &str) -> Result<Vec<Value>> {
        Ok(match self {
            Self::Whole => vec![json!(text)],
            Self::Delimiter(separator) if separator.is_empty() => {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "Split delimiter cannot be empty",
                ));
            }
            Self::Delimiter(separator) => text
                .split(separator)
                .filter(|s| !s.is_empty())
                .map(|s| json!(s))
                .collect(),
            Self::Lines => {
                if text.is_empty() {
                    return Ok(vec![]);
                }
                let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                let mut parts: Vec<_> = normalized.split('\n').map(|s| json!(s)).collect();
                if normalized.ends_with('\n') {
                    parts.pop();
                }
                parts
            }
            Self::Whitespace => text.split_whitespace().map(|s| json!(s)).collect(),
            Self::Json => serde_json::from_str::<Value>(text)
                .map_err(|_| CliError::validation("TYPE_MISMATCH", "Expected valid JSON"))?
                .as_array()
                .cloned()
                .ok_or_else(|| CliError::validation("TYPE_MISMATCH", "Expected a JSON array"))?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DuplicatePolicy {
    Last,
    First,
    Error,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PathType {
    File,
    Directory,
}
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathChecks {
    pub must_exist: bool,
    pub path_type: Option<PathType>,
    pub create: bool,
}
impl PathChecks {
    pub fn file() -> Self {
        Self {
            must_exist: true,
            path_type: Some(PathType::File),
            create: false,
        }
    }
    pub fn directory() -> Self {
        Self {
            must_exist: true,
            path_type: Some(PathType::Directory),
            create: false,
        }
    }
    pub fn allow_missing(mut self) -> Self {
        self.must_exist = false;
        self
    }
    pub fn create(mut self) -> Self {
        self.create = true;
        self
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Alias {
    pub name: String,
    pub hidden: bool,
}

pub(crate) type NativeDecoder = Arc<dyn Fn(&Value) -> Result<NativeValue> + Send + Sync>;
pub(crate) type NativeValidator = Arc<dyn Fn(&NativeValue) -> Result<()> + Send + Sync>;
pub(crate) type Encoder = Arc<dyn Fn(&NativeValue) -> Option<Value> + Send + Sync>;
pub(crate) type Validator = Arc<dyn Fn(&Value) -> Result<()> + Send + Sync>;

/// Serializable input declaration. Custom code is retained separately from metadata.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputSchema {
    pub name: String,
    pub surface: Surface,
    pub kind: Kind,
    pub description: String,
    pub aliases: Vec<Alias>,
    pub required: bool,
    pub hidden: bool,
    pub sensitive: bool,
    pub deprecated: Option<String>,
    pub negated_name: Option<String>,
    pub duplicate: DuplicatePolicy,
    pub duplicate_keys: DuplicatePolicy,
    pub env_vars: Vec<String>,
    pub config_path: Option<String>,
    pub stdin: Option<StdinBinding>,
    pub prompt: Option<Prompt>,
    #[serde(skip)]
    pub default_value: Option<Value>,
    pub enum_values: Vec<String>,
    pub cli_split: Split,
    pub env_split: Split,
    pub stdin_split: Split,
    pub unique: bool,
    pub variadic: bool,
    pub propagate: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub integer: bool,
    pub non_empty: bool,
    pub(crate) strict_string: bool,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub pattern: Option<String>,
    pub protocols: Vec<String>,
    pub path_checks: Option<PathChecks>,
    pub value_hint: Option<String>,
    pub element: Option<Box<InputSchema>>,
    #[serde(skip)]
    pub(crate) native_decoder: Option<NativeDecoder>,
    #[serde(skip)]
    pub(crate) native_default: Option<NativeValue>,
    #[serde(skip)]
    pub(crate) native_validators: Vec<NativeValidator>,
    #[serde(skip)]
    pub(crate) encoder: Option<Encoder>,
    #[serde(skip)]
    pub(crate) validators: Vec<Validator>,
}

impl std::fmt::Debug for InputSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputSchema")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl InputSchema {
    fn new(name: &str, kind: Kind, surface: Surface) -> Self {
        Self {
            name: name.into(),
            surface,
            kind,
            description: String::new(),
            aliases: vec![],
            required: surface == Surface::Arg,
            hidden: false,
            sensitive: false,
            deprecated: None,
            negated_name: None,
            duplicate: DuplicatePolicy::Last,
            duplicate_keys: DuplicatePolicy::Last,
            env_vars: vec![],
            config_path: None,
            stdin: None,
            prompt: None,
            default_value: None,
            enum_values: vec![],
            cli_split: Split::Whole,
            env_split: Split::Delimiter(",".into()),
            stdin_split: Split::Lines,
            unique: false,
            variadic: false,
            propagate: false,
            min: None,
            max: None,
            integer: false,
            non_empty: false,
            strict_string: false,
            min_length: None,
            max_length: None,
            pattern: None,
            protocols: vec![],
            path_checks: None,
            value_hint: None,
            element: None,
            native_decoder: None,
            native_default: None,
            native_validators: vec![],
            encoder: None,
            validators: vec![],
        }
    }
    pub fn expects_value(&self) -> bool {
        !matches!(self.kind, Kind::Boolean | Kind::Count)
    }
    pub fn collection(&self) -> bool {
        self.variadic || matches!(self.kind, Kind::Array | Kind::KeyValue)
    }
    pub fn display_name(&self) -> String {
        match self.surface {
            Surface::Flag => format!("--{}", self.name),
            Surface::Arg => format!("<{}>", self.name),
        }
    }
    pub(crate) fn failure(&self, code: &str, reason: &str, cli: bool) -> CliError {
        let message = format!(
            "Invalid value for {}: {}",
            self.display_name(),
            if self.sensitive { "[redacted]" } else { reason }
        );
        let mut error = if cli {
            CliError::parse("INVALID_VALUE", message)
        } else {
            CliError::validation(code, message)
        };
        error.details = Some(
            json!({match self.surface { Surface::Flag => "flag", Surface::Arg => "arg" }: self.name}),
        );
        error
    }
    pub(crate) fn decode_scalar(&self, raw: Value, source: Stage) -> Result<Value> {
        let cli = source == Stage::Cli;
        let result = (|| {
            let value = match self.kind {
                Kind::String | Kind::Path => {
                    if let Some(text) = raw.as_str() {
                        json!(text)
                    } else if !self.strict_string
                        && ((source == Stage::Config && (raw.is_number() || raw.is_boolean()))
                            || source == Stage::Prompt)
                    {
                        json!(raw.to_string())
                    } else {
                        return Err(CliError::validation("TYPE_MISMATCH", "Expected a string"));
                    }
                }
                Kind::Number | Kind::Count => {
                    let n = if let Some(n) = raw.as_f64() {
                        n
                    } else if let Some(s) = raw.as_str() {
                        if s.trim().is_empty() {
                            return Err(CliError::validation("TYPE_MISMATCH", "Expected a number"));
                        }
                        parse_number(s.trim()).map_err(|_| {
                            CliError::validation("TYPE_MISMATCH", "Expected a finite number")
                        })?
                    } else {
                        return Err(CliError::validation("TYPE_MISMATCH", "Expected a number"));
                    };
                    if !n.is_finite() {
                        return Err(CliError::validation(
                            "TYPE_MISMATCH",
                            "Expected a finite number",
                        ));
                    }
                    if self.kind == Kind::Count {
                        if n < 0.0 || n.fract() != 0.0 || n >= u64::MAX as f64 {
                            return Err(CliError::validation(
                                "CONSTRAINT_VIOLATED",
                                "Count must be a non-negative integer",
                            ));
                        }
                        json!(n as u64)
                    } else {
                        json!(n)
                    }
                }
                Kind::Boolean => {
                    if let Some(b) = raw.as_bool() {
                        json!(b)
                    } else {
                        let s = raw.as_str().map(str::to_lowercase).unwrap_or_default();
                        match s.as_str() {
                            "true" | "1" => json!(true),
                            "false" | "0" => json!(false),
                            "yes" | "y" if !cli => json!(true),
                            "no" | "n" | "" if !cli && raw.is_string() => json!(false),
                            _ => {
                                return Err(CliError::validation(
                                    "TYPE_MISMATCH",
                                    "Expected a boolean",
                                ));
                            }
                        }
                    }
                }
                Kind::Enum => {
                    let s = raw.as_str().ok_or_else(|| {
                        CliError::validation("TYPE_MISMATCH", "Expected an enum string")
                    })?;
                    if !self.enum_values.iter().any(|v| v == s) {
                        return Err(CliError::validation(
                            "INVALID_ENUM",
                            format!("Expected one of {}", self.enum_values.join(", ")),
                        ));
                    }
                    json!(s)
                }
                Kind::Url => {
                    let url = url::Url::parse(raw.as_str().ok_or_else(|| {
                        CliError::validation("TYPE_MISMATCH", "Expected a URL string")
                    })?)
                    .map_err(|_| CliError::validation("CONSTRAINT_VIOLATED", "Invalid URL"))?;
                    if !self.protocols.is_empty()
                        && !self.protocols.iter().any(|p| p == url.scheme())
                    {
                        return Err(CliError::validation(
                            "CONSTRAINT_VIOLATED",
                            "URL protocol is not allowed",
                        ));
                    }
                    json!(url.as_str())
                }
                Kind::Date => json!(parse_date(raw.as_str().ok_or_else(|| {
                    CliError::validation("TYPE_MISMATCH", "Expected an ISO date")
                })?)?),
                Kind::Duration => json!(parse_duration(&raw)?),
                Kind::Bytes => json!(parse_bytes(&raw)?),
                Kind::Custom => {
                    if !self.enum_values.is_empty()
                        && !raw
                            .as_str()
                            .is_some_and(|s| self.enum_values.iter().any(|v| v == s))
                    {
                        return Err(CliError::validation(
                            "INVALID_ENUM",
                            "Value is not an available choice",
                        ));
                    }
                    raw
                }
                Kind::Array | Kind::KeyValue => {
                    return Err(CliError::new(
                        "INVALID_SCHEMA",
                        "Collection used as an element",
                    ));
                }
            };
            self.validate_scalar(&value)?;
            Ok(value)
        })();
        result.map_err(|e| self.failure(&e.code, &e.message, cli))
    }
    pub(crate) fn validate_scalar(&self, value: &Value) -> Result<()> {
        let fail = |reason| CliError::validation("CONSTRAINT_VIOLATED", reason);
        if let Some(n) = value.as_f64() {
            if self.min.is_some_and(|min| n < min) {
                return Err(fail("Value is below the minimum"));
            }
            if self.max.is_some_and(|max| n > max) {
                return Err(fail("Value is above the maximum"));
            }
            if self.integer && n.fract() != 0.0 {
                return Err(fail("Value must be an integer"));
            }
        }
        if let Some(s) = value.as_str() {
            let len = s.encode_utf16().count();
            if self.non_empty && s.is_empty() {
                return Err(fail("Value must not be empty"));
            }
            if self.min_length.is_some_and(|min| len < min) {
                return Err(fail("Value is too short"));
            }
            if self.max_length.is_some_and(|max| len > max) {
                return Err(fail("Value is too long"));
            }
            if let Some(pattern) = &self.pattern {
                let regex = Regex::new(pattern)
                    .map_err(|_| CliError::new("INVALID_SCHEMA", "Invalid regular expression"))?;
                if !regex.is_match(s) {
                    return Err(fail("Value does not match the pattern"));
                }
            }
        }
        for validator in &self.validators {
            validator(value)?;
        }
        Ok(())
    }
    pub(crate) fn validate_definition(&self) -> Result<()> {
        if self.name.is_empty()
            || self.name.starts_with('-')
            || self.name.chars().any(|c| c.is_whitespace() || c == '=')
        {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Input names must be non-empty names without whitespace, leading dashes, or '='",
            ));
        }
        if self.negated_name.is_some() && self.kind != Kind::Boolean {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Only boolean flags can be negated",
            ));
        }
        if self.kind == Kind::Enum && self.enum_values.is_empty() {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "An enum needs at least one choice",
            ));
        }
        if self.min.zip(self.max).is_some_and(|(min, max)| min > max)
            || self
                .min_length
                .zip(self.max_length)
                .is_some_and(|(min, max)| min > max)
        {
            return Err(CliError::new("INVALID_SCHEMA", "Minimum exceeds maximum"));
        }
        if let Some(pattern) = &self.pattern {
            Regex::new(pattern)
                .map_err(|_| CliError::new("INVALID_SCHEMA", "Invalid regular expression"))?;
        }
        if self
            .path_checks
            .as_ref()
            .is_some_and(|p| p.create && p.path_type != Some(PathType::Directory))
        {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Only directory paths can be created",
            ));
        }
        for split in [&self.cli_split, &self.env_split, &self.stdin_split] {
            if matches!(split, Split::Delimiter(s) if s.is_empty()) {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "Split delimiter cannot be empty",
                ));
            }
        }
        if let Some(element) = &self.element {
            if element.collection() || matches!(element.kind, Kind::Count) {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "Collections require a scalar element",
                ));
            }
            element.validate_definition()?;
        }
        if let Some(default) = &self.native_default {
            if let Some(json) = default.json() {
                crate::resolve::validate_default(self, json)
                    .map_err(|e| CliError::new("INVALID_SCHEMA", e.message))?;
            }
            for validate in &self.native_validators {
                validate(default).map_err(|e| self.failure("INVALID_SCHEMA", &e.message, false))?;
            }
        }
        Ok(())
    }
}

mod builder;
mod codecs;
pub use builder::Arg;
pub use builder::ArgMarker;
pub use builder::Flag;
pub use builder::FlagMarker;
pub use builder::Input;
pub use builder::Optional;
pub use builder::Presence;
pub use builder::Required;
pub use builder::arg;
pub use builder::flag;
use codecs::parse_bytes;
use codecs::parse_date;
use codecs::parse_duration;
use codecs::parse_number;
