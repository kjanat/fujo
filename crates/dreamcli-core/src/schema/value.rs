//! Reusable scalar parsers without input bindings or defaults.
//!
//! ```compile_fail
//! use dreamcli_core::schema::{flag, value};
//! let tags = flag::array("tags", value::string().env("TAGS"));
//! ```
//! ```compile_fail
//! use dreamcli_core::schema::flag;
//! let tags = flag::array("tags", flag::string("tag").env("TAGS"));
//! ```
//! ```compile_fail
//! use dreamcli_core::schema::flag;
//! let name = flag::string("name").separator(",");
//! ```
//! ```compile_fail
//! use dreamcli_core::schema::flag;
//! let name = flag::string("name").unique();
//! ```
use super::*;
use std::marker::PhantomData;

#[derive(Debug)]
pub struct ValueParser<T> {
    pub(super) schema: InputSchema,
    marker: PhantomData<fn() -> T>,
}
impl<T> Clone for ValueParser<T> {
    fn clone(&self) -> Self {
        Self {
            schema: self.schema.clone(),
            marker: PhantomData,
        }
    }
}
impl<T> ValueParser<T> {
    fn from_input<P>(input: Input<T, P>) -> Self {
        let mut schema = input.into_schema();
        schema.declaration.required = false;
        schema.declaration.default_value = None;
        schema.native_default = None;
        Self {
            schema,
            marker: PhantomData,
        }
    }
    pub fn flag(self, name: &str) -> Flag<T> {
        let mut schema = self.schema;
        schema.declaration.name = name.into();
        Input::from_schema(schema)
    }
    pub fn arg(self, name: &str) -> Arg<T> {
        let mut schema = self.schema;
        schema.declaration.name = name.into();
        schema.declaration.surface = Surface::Arg;
        schema.declaration.required = true;
        Input::from_schema(schema)
    }
    pub fn validate(self, validator: impl Fn(&T) -> Result<()> + Send + Sync + 'static) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::from_input(self.flag("value").validate(validator))
    }
    pub fn choices(self, choices: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::from_input(self.flag("value").choices(choices))
    }
    pub fn encode_json(self, encoder: impl Fn(&T) -> Value + Send + Sync + 'static) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::from_input(self.flag("value").encode_json(encoder))
    }
    pub fn json_schema(self, schema: Value) -> Self {
        Self::from_input(self.flag("value").json_schema(schema))
    }
}
impl ValueParser<String> {
    pub fn non_empty(self) -> Self {
        Self::from_input(self.flag("value").non_empty())
    }
    pub fn min_length(self, min: usize) -> Self {
        Self::from_input(self.flag("value").min_length(min))
    }
    pub fn max_length(self, max: usize) -> Self {
        Self::from_input(self.flag("value").max_length(max))
    }
    pub fn pattern(self, pattern: impl Into<String>) -> Self {
        Self::from_input(self.flag("value").pattern(pattern))
    }
}
impl ValueParser<f64> {
    pub fn min(self, min: f64) -> Self {
        Self::from_input(self.flag("value").min(min))
    }
    pub fn max(self, max: f64) -> Self {
        Self::from_input(self.flag("value").max(max))
    }
    pub fn integer(self) -> Self {
        Self::from_input(self.flag("value").integer())
    }
}
impl ValueParser<url::Url> {
    pub fn protocols(self, protocols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::from_input(self.flag("value").protocols(protocols))
    }
}
pub fn string() -> ValueParser<String> {
    ValueParser::from_input(flag::string("value"))
}
pub fn number() -> ValueParser<f64> {
    ValueParser::from_input(flag::number("value"))
}
pub fn boolean() -> ValueParser<bool> {
    ValueParser::from_input(flag::boolean("value"))
}
pub fn enumeration(choices: impl IntoIterator<Item = impl Into<String>>) -> ValueParser<String> {
    ValueParser::from_input(flag::enumeration("value", choices))
}
pub fn url() -> ValueParser<url::Url> {
    ValueParser::from_input(flag::url("value"))
}
pub fn path() -> ValueParser<std::path::PathBuf> {
    ValueParser::from_input(flag::path("value"))
}
pub fn date() -> ValueParser<chrono::DateTime<chrono::Utc>> {
    ValueParser::from_input(flag::date("value"))
}
pub fn duration() -> ValueParser<std::time::Duration> {
    ValueParser::from_input(flag::duration("value"))
}
pub fn bytes() -> ValueParser<u64> {
    ValueParser::from_input(flag::bytes("value"))
}
pub fn custom<T: Send + Sync + 'static>(
    decode: impl Fn(Value) -> Result<T> + Send + Sync + 'static,
) -> ValueParser<T> {
    ValueParser::from_input(flag::custom("value", decode))
}
pub fn parse<T>() -> ValueParser<T>
where
    T: std::str::FromStr + Send + Sync + 'static,
    T::Err: std::fmt::Display,
{
    ValueParser::from_input(flag::parse("value"))
}
