use super::codecs::collection_schema;
use super::codecs::typed_schema;
use super::*;
use chrono::DateTime;
use chrono::Utc;
use std::collections::BTreeMap;
use std::marker::PhantomData;

/// Input surfaces are distinct types; a positional cannot be registered as a flag.
#[derive(Debug, Clone, Copy)]
pub struct FlagMarker;
#[derive(Debug, Clone, Copy)]
pub struct ArgMarker;
pub type Flag<T, P = Optional> = Input<T, P, FlagMarker>;
pub type Arg<T, P = Required> = Input<T, P, ArgMarker>;

/// Optional/required presence is part of the Rust type, including defaults.
#[derive(Debug, Clone, Copy)]
pub struct Optional;
#[derive(Debug, Clone, Copy)]
pub struct Required;
pub trait Presence<T> {
    type Output;
    type Ref<'a>
    where
        T: 'a;
    fn read(value: Option<T>, name: &str) -> Result<Self::Output>;
    fn read_ref<'a>(value: Option<&'a T>, name: &str) -> Result<Self::Ref<'a>>
    where
        T: 'a;
}
impl<T> Presence<T> for Optional {
    type Output = Option<T>;
    type Ref<'a>
        = Option<&'a T>
    where
        T: 'a;
    fn read(value: Option<T>, _: &str) -> Result<Self::Output> {
        Ok(value)
    }
    fn read_ref<'a>(value: Option<&'a T>, _: &str) -> Result<Self::Ref<'a>>
    where
        T: 'a,
    {
        Ok(value)
    }
}
impl<T> Presence<T> for Required {
    type Output = T;
    type Ref<'a>
        = &'a T
    where
        T: 'a;
    fn read(value: Option<T>, name: &str) -> Result<T> {
        value.ok_or_else(|| {
            CliError::new(
                "MISSING_RESOLVED_INPUT",
                format!("Input {name} has not been resolved"),
            )
        })
    }
    fn read_ref<'a>(value: Option<&'a T>, name: &str) -> Result<Self::Ref<'a>>
    where
        T: 'a,
    {
        value.ok_or_else(|| {
            CliError::new(
                "MISSING_RESOLVED_INPUT",
                format!("Input {name} has not been resolved"),
            )
        })
    }
}

#[derive(Debug)]
pub struct Input<T, P = Optional, S = FlagMarker> {
    pub(crate) schema: InputSchema,
    marker: PhantomData<InputType<T, P, S>>,
}
type InputType<T, P, S> = fn() -> (T, P, S);
impl<T, P, S> Clone for Input<T, P, S> {
    fn clone(&self) -> Self {
        Self {
            schema: self.schema.clone(),
            marker: PhantomData,
        }
    }
}
impl<T, P, S> Input<T, P, S> {
    pub(super) fn from_schema(schema: InputSchema) -> Self {
        Self {
            schema,
            marker: PhantomData,
        }
    }
    pub fn schema(&self) -> &InputSchema {
        &self.schema
    }
    pub fn into_schema(self) -> InputSchema {
        self.schema
    }
    pub fn name(&self) -> &str {
        &self.schema.name
    }
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.schema.declaration.description = description.into();
        self
    }

    pub fn env(mut self, name: impl Into<String>) -> Self {
        self.schema.declaration.env_vars.push(name.into());
        self
    }
    pub fn config(mut self, path: impl Into<String>) -> Self {
        self.schema.declaration.config_path = Some(path.into());
        self
    }
    pub fn stdin(self) -> Self {
        self.stdin_with(StdinBinding::default())
    }
    pub fn stdin_with(mut self, binding: StdinBinding) -> Self {
        self.schema.declaration.stdin = Some(binding);
        self
    }
    pub fn prompt(mut self, prompt: Prompt) -> Self {
        self.schema.declaration.prompt = Some(prompt);
        self
    }
    pub fn hidden(mut self) -> Self {
        self.schema.declaration.hidden = true;
        self
    }
    pub fn sensitive(mut self) -> Self {
        self.schema.declaration.sensitive = true;
        self
    }
    pub fn deprecated(mut self, message: impl Into<String>) -> Self {
        self.schema.declaration.deprecated = Some(message.into());
        self
    }
    pub fn value_hint(mut self, hint: impl Into<String>) -> Self {
        self.schema.declaration.value_hint = Some(hint.into());
        self
    }
    /// Supply the JSON representation without requiring Serde on the native type.
    pub fn encode_json(mut self, encode: impl Fn(&T) -> Value + Send + Sync + 'static) -> Self
    where
        T: Send + Sync + 'static,
    {
        if self.schema.validation_encoder.is_none() {
            self.schema.validation_encoder = self.schema.encoder.clone();
        }
        self.schema.encoder = Some(Arc::new(move |native| {
            native.inner.downcast_ref::<T>().map(&encode)
        }));
        if let Some(default) = &mut self.schema.native_default {
            default.json = self
                .schema
                .encoder
                .as_ref()
                .and_then(|encode| encode(default));
            self.schema.declaration.default_value = default.json.clone();
        }
        self
    }
    pub fn json_schema(mut self, schema: Value) -> Self {
        self.schema.declaration.json_schema = Some(schema);
        self
    }
    pub fn default_description(mut self, description: impl Into<DefaultDescription>) -> Self {
        self.schema.declaration.default_description = Some(description.into());
        self
    }
    pub fn hide_default(self) -> Self {
        self.default_description(DefaultDescription::Hidden)
    }
    pub fn choices(mut self, choices: impl IntoIterator<Item = impl Into<String>>) -> Self {
        if self.schema.kind == Kind::String {
            self.schema.declaration.kind = Kind::Enum;
        }
        self.schema.declaration.enum_values = choices.into_iter().map(Into::into).collect();
        self
    }

    pub fn required(mut self) -> Input<T, Required, S> {
        self.schema.declaration.default_description = None;
        self.schema.declaration.required = true;
        self.schema.native_default = None;
        self.schema.declaration.default_value = None;
        Input::from_schema(self.schema)
    }
    pub fn optional(mut self) -> Input<T, Optional, S> {
        self.schema.declaration.default_description = None;
        self.schema.declaration.required = false;
        self.schema.declaration.default_value = None;
        self.schema.native_default = None;
        Input::from_schema(self.schema)
    }
    pub fn default(mut self, value: T) -> Input<T, Required, S>
    where
        T: Send + Sync + 'static,
    {
        self.schema.declaration.required = false;
        self.schema.declaration.default_description = None;
        let mut native = NativeValue::new(value);
        native.json = self
            .schema
            .encoder
            .as_ref()
            .and_then(|encode| encode(&native));
        self.schema.declaration.default_value = native.json.clone();
        self.schema.native_default = Some(native);
        Input::from_schema(self.schema)
    }
    pub fn validate(mut self, validator: impl Fn(&T) -> Result<()> + Send + Sync + 'static) -> Self
    where
        T: Send + Sync + 'static,
    {
        self.schema
            .native_validators
            .push(Arc::new(move |v| validator(v.downcast_ref::<T>()?)));
        self
    }
}

impl<T, P> Input<T, P, FlagMarker> {
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.schema.declaration.aliases.push(Alias {
            name: alias.into(),
            hidden: false,
        });
        self
    }
    pub fn hidden_alias(mut self, alias: impl Into<String>) -> Self {
        self.schema.declaration.aliases.push(Alias {
            name: alias.into(),
            hidden: true,
        });
        self
    }
    pub fn duplicate(mut self, policy: DuplicatePolicy) -> Self {
        self.schema.declaration.duplicate = policy;
        self
    }
    pub fn propagate(mut self) -> Self {
        self.schema.declaration.propagate = true;
        self
    }
}

mod sealed {
    pub trait Collection {}
    impl<T> Collection for Vec<T> {}
    impl<T> Collection for std::collections::BTreeMap<String, T> {}
}
/// Collection values support delimiter and source-specific splitting.
pub trait Collection: sealed::Collection {}
impl<T> Collection for Vec<T> {}
impl<T> Collection for BTreeMap<String, T> {}
impl<T: Collection, P, S> Input<T, P, S> {
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.schema.declaration.cli_split = Split::Delimiter(separator.into());
        self
    }
    pub fn split_cli(mut self, split: Split) -> Self {
        self.schema.declaration.cli_split = split;
        self
    }
    pub fn split_env(mut self, split: Split) -> Self {
        self.schema.declaration.env_split = split;
        self
    }
    pub fn split_stdin(mut self, split: Split) -> Self {
        self.schema.declaration.stdin_split = split;
        self
    }
}
impl<T, P, S> Input<Vec<T>, P, S> {
    pub fn unique(mut self) -> Self {
        self.schema.declaration.unique = true;
        self
    }
}
impl<T, P, S> Input<BTreeMap<String, T>, P, S> {
    pub fn duplicate_keys(mut self, policy: DuplicatePolicy) -> Self {
        self.schema.declaration.duplicate_keys = policy;
        self
    }
}
impl<T, P> Input<T, P, ArgMarker> {
    pub fn variadic(self) -> Input<Vec<T>, P, ArgMarker>
    where
        T: Clone + Send + Sync + 'static,
    {
        let original = self.schema;
        let mut schema = collection_schema::<T>(&original.name, Kind::Array, original.clone());
        schema.declaration.surface = Surface::Arg;
        let original = original.declaration;
        schema.declaration.required = original.required;
        schema.declaration.description = original.description;
        schema.declaration.env_vars = original.env_vars;
        schema.declaration.config_path = original.config_path;
        schema.declaration.stdin = original.stdin;
        schema.declaration.prompt = original.prompt;
        schema.declaration.cli_split = original.cli_split;
        schema.declaration.env_split = original.env_split;
        schema.declaration.stdin_split = original.stdin_split;
        schema.declaration.variadic = true;
        schema.declaration.sensitive = original.sensitive;
        Input::from_schema(schema)
    }
}

impl<P, S> Input<String, P, S> {
    pub fn non_empty(mut self) -> Self {
        self.schema.declaration.non_empty = true;
        self
    }
    pub fn min_length(mut self, min: usize) -> Self {
        self.schema.declaration.min_length = Some(min);
        self
    }
    pub fn max_length(mut self, max: usize) -> Self {
        self.schema.declaration.max_length = Some(max);
        self
    }
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.schema.declaration.pattern = Some(pattern.into());
        self
    }
}
impl<P, S> Input<std::path::PathBuf, P, S> {
    pub fn path_checks(mut self, checks: PathChecks) -> Self {
        self.schema.declaration.path_checks = Some(checks);
        self
    }
}
impl<P, S> Input<f64, P, S> {
    pub fn min(mut self, min: f64) -> Self {
        self.schema.declaration.min = Some(min);
        self
    }
    pub fn max(mut self, max: f64) -> Self {
        self.schema.declaration.max = Some(max);
        self
    }
    pub fn integer(mut self) -> Self {
        self.schema.declaration.integer = true;
        self
    }
}
impl<P> Input<bool, P, FlagMarker> {
    pub fn negatable(mut self) -> Self {
        self.schema.declaration.negated_name = Some(format!("no-{}", self.schema.name));
        self
    }
    pub fn negated_name(mut self, name: impl Into<String>) -> Self {
        self.schema.declaration.negated_name = Some(name.into());
        self
    }
}
impl<P, S> Input<url::Url, P, S> {
    pub fn protocols(mut self, protocols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.schema.declaration.protocols = protocols.into_iter().map(Into::into).collect();
        self
    }
}

pub mod flag {
    use super::*;
    pub fn string(name: &str) -> Input<String> {
        Input::from_schema(typed_schema::<String>(InputSchema::new(
            name,
            Kind::String,
            Surface::Flag,
        )))
    }
    pub fn number(name: &str) -> Input<f64> {
        Input::from_schema(typed_schema::<f64>(InputSchema::new(
            name,
            Kind::Number,
            Surface::Flag,
        )))
    }
    pub fn boolean(name: &str) -> Input<bool, Required> {
        Input::<bool>::from_schema(typed_schema::<bool>(InputSchema::new(
            name,
            Kind::Boolean,
            Surface::Flag,
        )))
        .default(false)
    }
    pub fn count(name: &str) -> Input<u64, Required> {
        Input::<u64>::from_schema(typed_schema::<u64>(InputSchema::new(
            name,
            Kind::Count,
            Surface::Flag,
        )))
        .default(0)
    }
    pub fn enumeration(
        name: &str,
        choices: impl IntoIterator<Item = impl Into<String>>,
    ) -> Input<String> {
        let mut schema = InputSchema::new(name, Kind::Enum, Surface::Flag);
        schema.declaration.enum_values = choices.into_iter().map(Into::into).collect();
        Input::from_schema(typed_schema::<String>(schema))
    }
    pub fn array<T: Clone + Send + Sync + 'static>(
        name: &str,
        element: super::value::ValueParser<T>,
    ) -> Input<Vec<T>, Required> {
        Input::<Vec<T>>::from_schema(collection_schema::<T>(name, Kind::Array, element.schema))
            .default(vec![])
    }
    pub fn key_value(name: &str) -> Input<BTreeMap<String, String>, Required> {
        let mut element = super::value::string();
        element.schema.declaration.strict_string = true;
        key_value_of(name, element).default(BTreeMap::new())
    }
    pub fn key_value_of<T: Clone + Send + Sync + 'static>(
        name: &str,
        element: super::value::ValueParser<T>,
    ) -> Input<BTreeMap<String, T>> {
        Input::from_schema(collection_schema::<T>(name, Kind::KeyValue, element.schema))
    }
    pub fn custom<T: Send + Sync + 'static>(
        name: &str,
        decode: impl Fn(Value) -> Result<T> + Send + Sync + 'static,
    ) -> Input<T> {
        let mut schema = InputSchema::new(name, Kind::Custom, Surface::Flag);
        schema.native_decoder = Some(Arc::new(move |raw| {
            Ok(NativeValue::new(decode(raw.clone())?))
        }));
        Input::from_schema(schema)
    }
    /// Parse any native `FromStr` type, including application enums and identifiers.
    pub fn parse<T>(name: &str) -> Input<T>
    where
        T: std::str::FromStr + Send + Sync + 'static,
        T::Err: std::fmt::Display,
    {
        let mut input = custom(name, |raw| {
            let text = if let Some(text) = raw.as_str() {
                text.to_owned()
            } else if raw.is_number() || raw.is_boolean() {
                raw.to_string()
            } else {
                return Err(CliError::validation(
                    "TYPE_MISMATCH",
                    "Expected a scalar value",
                ));
            };
            text.parse::<T>()
                .map_err(|e| CliError::validation("CONSTRAINT_VIOLATED", e.to_string()))
        });
        input.schema.encoder = Some(Arc::new(|native| {
            crate::value::primitive_json(native.inner.as_ref())
        }));
        input.schema.declaration.json_schema = crate::value::primitive_schema::<T>();
        input
    }
    pub fn url(name: &str) -> Input<url::Url> {
        Input::from_schema(typed_schema::<url::Url>(InputSchema::new(
            name,
            Kind::Url,
            Surface::Flag,
        )))
    }
    pub fn path(name: &str) -> Input<std::path::PathBuf> {
        Input::from_schema(typed_schema::<std::path::PathBuf>(InputSchema::new(
            name,
            Kind::Path,
            Surface::Flag,
        )))
    }
    pub fn date(name: &str) -> Input<DateTime<Utc>> {
        Input::from_schema(typed_schema::<DateTime<Utc>>(InputSchema::new(
            name,
            Kind::Date,
            Surface::Flag,
        )))
    }
    pub fn duration(name: &str) -> Input<std::time::Duration> {
        let mut schema = InputSchema::new(name, Kind::Duration, Surface::Flag);
        schema.native_decoder = Some(Arc::new(|json| {
            let millis = json
                .as_f64()
                .ok_or_else(|| CliError::validation("TYPE_MISMATCH", "Expected milliseconds"))?;
            let value = std::time::Duration::try_from_secs_f64(millis / 1000.0).map_err(|_| {
                CliError::validation("CONSTRAINT_VIOLATED", "Duration out of range")
            })?;
            Ok(NativeValue {
                inner: Arc::new(value),
                json: Some(json.clone()),
                sensitive: false,
            })
        }));
        schema.encoder = Some(Arc::new(|native| {
            Some(json!(
                native
                    .inner
                    .downcast_ref::<std::time::Duration>()?
                    .as_secs_f64()
                    * 1000.0
            ))
        }));
        Input::from_schema(schema)
    }
    pub fn bytes(name: &str) -> Input<u64> {
        let mut schema = InputSchema::new(name, Kind::Bytes, Surface::Flag);
        schema.native_decoder = Some(Arc::new(|json| {
            let bytes = json
                .as_f64()
                .ok_or_else(|| CliError::validation("TYPE_MISMATCH", "Expected bytes"))?;
            if bytes < 0.0 || bytes >= u64::MAX as f64 {
                return Err(CliError::validation(
                    "CONSTRAINT_VIOLATED",
                    "Byte size out of range",
                ));
            }
            Ok(NativeValue {
                inner: Arc::new(bytes.round() as u64),
                json: Some(json.clone()),
                sensitive: false,
            })
        }));
        schema.encoder = Some(Arc::new(|native| {
            Some(json!(native.inner.downcast_ref::<u64>()?))
        }));
        Input::from_schema(schema)
    }
}

pub mod arg {
    use super::*;
    fn positional<T, P>(mut flag: Input<T, P>) -> Input<T, Required, ArgMarker> {
        flag.schema.declaration.surface = Surface::Arg;
        flag.schema.declaration.required = true;
        flag.schema.native_default = None;
        flag.schema.declaration.default_value = None;
        Input::from_schema(flag.schema)
    }
    pub fn string(name: &str) -> Input<String, Required, ArgMarker> {
        positional(flag::string(name))
    }
    pub fn number(name: &str) -> Input<f64, Required, ArgMarker> {
        positional(flag::number(name))
    }
    pub fn boolean(name: &str) -> Input<bool, Required, ArgMarker> {
        let mut schema = InputSchema::new(name, Kind::Boolean, Surface::Arg);
        schema.declaration.required = true;
        Input::from_schema(typed_schema::<bool>(schema))
    }
    pub fn enumeration(
        name: &str,
        choices: impl IntoIterator<Item = impl Into<String>>,
    ) -> Input<String, Required, ArgMarker> {
        positional(flag::enumeration(name, choices))
    }
    pub fn custom<T: Send + Sync + 'static>(
        name: &str,
        decode: impl Fn(Value) -> Result<T> + Send + Sync + 'static,
    ) -> Input<T, Required, ArgMarker> {
        positional(flag::custom(name, decode))
    }
    pub fn parse<T>(name: &str) -> Input<T, Required, ArgMarker>
    where
        T: std::str::FromStr + Send + Sync + 'static,
        T::Err: std::fmt::Display,
    {
        positional(flag::parse(name))
    }
    pub fn key_value(name: &str) -> Input<BTreeMap<String, String>, Required, ArgMarker> {
        positional(flag::key_value(name))
    }
    pub fn url(name: &str) -> Input<url::Url, Required, ArgMarker> {
        positional(flag::url(name))
    }
    pub fn path(name: &str) -> Input<std::path::PathBuf, Required, ArgMarker> {
        positional(flag::path(name))
    }
    pub fn date(name: &str) -> Input<DateTime<Utc>, Required, ArgMarker> {
        positional(flag::date(name))
    }
    pub fn duration(name: &str) -> Input<std::time::Duration, Required, ArgMarker> {
        positional(flag::duration(name))
    }
    pub fn bytes(name: &str) -> Input<u64, Required, ArgMarker> {
        positional(flag::bytes(name))
    }
}

impl<T, P> Input<BTreeMap<String, T>, P, ArgMarker> {
    /// Consume the positional tail as entries, folding it into one native map.
    pub fn variadic_entries(mut self) -> Self {
        self.schema.declaration.variadic = true;
        self
    }
}
