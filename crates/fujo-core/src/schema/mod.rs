use crate::CliError;
use crate::Result;
use crate::prompt::Prompt;
use crate::resolve::Stage;
use crate::value::NativeValue;
#[cfg(feature = "regex")]
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
    pub fn validate(&self) -> Result<()> {
        if matches!(self, Self::Delimiter(separator) if separator.is_empty()) {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Split delimiter cannot be empty",
            ));
        }
        Ok(())
    }
    pub fn values(&self, text: &str) -> Result<Vec<Value>> {
        self.validate()?;
        Ok(match self {
            Self::Whole => vec![json!(text)],
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

/// ```compile_fail
/// use fujo_core::schema::{flag, Kind};
/// let mut schema = flag::string("name").into_schema();
/// schema.kind = Kind::Number;
/// ```
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputSchema {
    #[serde(flatten)]
    pub(crate) declaration: InputDeclaration,
    #[serde(skip)]
    pub(crate) native_type: Option<std::any::TypeId>,
    #[serde(skip)]
    pub(crate) native_decoder: Option<NativeDecoder>,
    #[serde(skip)]
    pub(crate) native_default: Option<NativeValue>,
    #[serde(skip)]
    pub(crate) native_validators: Vec<NativeValidator>,
    #[serde(skip)]
    pub(crate) encoder: Option<Encoder>,
    #[serde(skip)]
    pub(crate) validation_encoder: Option<Encoder>,
    #[serde(skip)]
    pub(crate) validators: Vec<Validator>,
    #[cfg(feature = "regex")]
    #[serde(skip)]
    pub(crate) compiled_pattern: Option<Regex>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDeclaration {
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
    #[serde(skip)]
    pub(crate) strict_string: bool,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub pattern: Option<String>,
    pub protocols: Vec<String>,
    pub path_checks: Option<PathChecks>,
    pub value_hint: Option<String>,
    pub element: Option<Box<InputSchema>>,
    pub default_description: Option<DefaultDescription>,
    pub json_schema: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum DefaultDescription {
    Text(String),
    Hidden,
}
impl From<&str> for DefaultDescription {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<String> for DefaultDescription {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl Serialize for DefaultDescription {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Text(text) => serializer.serialize_str(text),
            Self::Hidden => serializer.serialize_bool(false),
        }
    }
}
impl std::ops::Deref for InputSchema {
    type Target = InputDeclaration;
    fn deref(&self) -> &Self::Target {
        &self.declaration
    }
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
    pub fn propagated(mut self) -> Self {
        self.declaration.propagate = true;
        self
    }
    pub fn conflicts_with(&self, other: &InputSchema) -> bool {
        self.name == other.name
            && (self.surface != other.surface
                || matches!((self.native_type, other.native_type), (Some(a), Some(b)) if a != b))
    }
    pub fn has_default(&self) -> bool {
        self.native_default.is_some()
    }
    pub fn default_json(&self) -> Option<&Value> {
        self.native_default.as_ref().and_then(|d| d.json.as_ref())
    }
    fn new(name: &str, kind: Kind, surface: Surface) -> Self {
        Self {
            declaration: InputDeclaration {
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
                default_description: None,
                json_schema: None,
            },
            native_type: None,
            native_decoder: None,
            native_default: None,
            native_validators: vec![],
            encoder: None,
            validation_encoder: None,
            validators: vec![],
            #[cfg(feature = "regex")]
            compiled_pattern: None,
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
        self.decode_raw(raw, source)
            .map_err(|e| self.failure(&e.code, &e.message, source == Stage::Cli))
    }
    pub(crate) fn decode_raw(&self, raw: Value, source: Stage) -> Result<Value> {
        let cli = source == Stage::Cli;
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
                json!(s)
            }
            #[cfg(feature = "url")]
            Kind::Url => {
                let url = url::Url::parse(raw.as_str().ok_or_else(|| {
                    CliError::validation("TYPE_MISMATCH", "Expected a URL string")
                })?)
                .map_err(|_| CliError::validation("CONSTRAINT_VIOLATED", "Invalid URL"))?;
                if !self.protocols.is_empty() && !self.protocols.iter().any(|p| p == url.scheme()) {
                    return Err(CliError::validation(
                        "CONSTRAINT_VIOLATED",
                        "URL protocol is not allowed",
                    ));
                }
                json!(url.as_str())
            }
            #[cfg(feature = "chrono")]
            Kind::Date => json!(parse_date(raw.as_str().ok_or_else(|| {
                CliError::validation("TYPE_MISMATCH", "Expected an ISO date")
            })?)?),
            #[cfg(not(feature = "url"))]
            Kind::Url => return Err(disabled_feature("url")),
            #[cfg(not(feature = "chrono"))]
            Kind::Date => return Err(disabled_feature("chrono")),
            Kind::Duration => json!(parse_duration(&raw)?),
            Kind::Bytes => json!(parse_bytes(&raw)?),
            Kind::Custom => raw,
            Kind::Array | Kind::KeyValue => {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "Collection used as an element",
                ));
            }
        };
        self.validate_scalar(&value)?;
        Ok(value)
    }
    pub(crate) fn validate_scalar(&self, value: &Value) -> Result<()> {
        let fail = |reason| CliError::validation("CONSTRAINT_VIOLATED", reason);
        if !self.enum_values.is_empty() {
            let text = match value {
                Value::String(s) => Some(s.clone()),
                Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
                _ => None,
            };
            if !text.is_some_and(|s| self.enum_values.contains(&s)) {
                return Err(CliError::validation(
                    "INVALID_ENUM",
                    format!("Expected one of {}", self.enum_values.join(", ")),
                ));
            }
        }
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
            #[cfg(feature = "regex")]
            if self
                .compiled_pattern
                .as_ref()
                .is_some_and(|r| !r.is_match(s))
            {
                return Err(fail("Value does not match the pattern"));
            }
        }
        for validator in &self.validators {
            validator(value)?;
        }
        Ok(())
    }
    pub(crate) fn validate_definition(&self) -> Result<()> {
        #[cfg(not(feature = "regex"))]
        if self.pattern.is_some() {
            return Err(disabled_feature("regex"));
        }
        #[cfg(not(feature = "url"))]
        if self.kind == Kind::Url {
            return Err(disabled_feature("url"));
        }
        #[cfg(not(feature = "chrono"))]
        if self.kind == Kind::Date {
            return Err(disabled_feature("chrono"));
        }
        if let Some(prompt) = &self.prompt {
            prompt.validate_definition()?;
        }
        if self
            .json_schema
            .as_ref()
            .is_some_and(|schema| !schema.is_object() && !schema.is_boolean())
        {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "JSON Schema must be an object or boolean",
            ));
        }
        if !self.enum_values.is_empty()
            && !matches!(self.kind, Kind::String | Kind::Enum | Kind::Custom)
        {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Text choices require a string, enum, or custom parser",
            ));
        }
        for spelling in self.spellings() {
            if spelling.is_empty()
                || spelling.starts_with('-')
                || spelling.chars().any(|c| c.is_whitespace() || c == '=')
            {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    format!(
                        "Input spelling '{spelling}' must be non-empty without whitespace, leading dashes, or '='"
                    ),
                ));
            }
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
        #[cfg(feature = "regex")]
        if self.pattern.is_some() && self.compiled_pattern.is_none() {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Invalid regular expression",
            ));
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
            split.validate()?;
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
            if let Some(json) = self.default_validation_json(default) {
                crate::resolve::validate_default(self, &json)
                    .map_err(|e| CliError::new("INVALID_SCHEMA", e.message))?;
            }
            for validate in &self.native_validators {
                validate(default).map_err(|e| self.failure("INVALID_SCHEMA", &e.message, false))?;
            }
        }
        Ok(())
    }
    pub(crate) fn spellings(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str())
            .chain(self.aliases.iter().map(|a| a.name.as_str()))
            .chain(self.negated_name.as_deref())
    }
    pub(crate) fn default_validation_json(&self, default: &NativeValue) -> Option<Value> {
        match &self.validation_encoder {
            Some(encode) => encode(default),
            None => default.json.clone(),
        }
    }
}

mod builder;
mod codecs;
pub mod value;
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
#[cfg(feature = "chrono")]
use codecs::parse_date;
use codecs::parse_duration;
use codecs::parse_number;

#[cfg(any(not(feature = "regex"), not(feature = "url"), not(feature = "chrono")))]
fn disabled_feature(feature: &str) -> CliError {
    CliError::new(
        "FEATURE_DISABLED",
        format!("Enable the '{feature}' Cargo feature for this input"),
    )
}
