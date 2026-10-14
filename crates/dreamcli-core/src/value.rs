use crate::CliError;
use crate::Result;
use serde_json::Value;
use std::any::Any;
use std::sync::Arc;

/// Native Rust value; JSON is optional metadata, never the handler's storage model.
#[derive(Clone)]
pub struct NativeValue {
    pub(crate) inner: Arc<dyn Any + Send + Sync>,
    pub(crate) json: Option<Value>,
    pub(crate) sensitive: bool,
}
impl std::fmt::Debug for NativeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeValue")
            .field(
                "json",
                &if self.sensitive {
                    Some(Value::String("[redacted]".into()))
                } else {
                    self.json.clone()
                },
            )
            .finish_non_exhaustive()
    }
}
impl NativeValue {
    pub fn new<T: Send + Sync + 'static>(value: T) -> Self {
        Self {
            inner: Arc::new(value),
            json: None,
            sensitive: false,
        }
    }
    pub fn json(&self) -> Option<&Value> {
        self.json.as_ref()
    }
    pub fn downcast_ref<T: 'static>(&self) -> Result<&T> {
        self.inner.downcast_ref().ok_or_else(|| {
            CliError::new(
                "TYPE_MISMATCH",
                format!("Input is not {}", std::any::type_name::<T>()),
            )
        })
    }
}
