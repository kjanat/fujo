use crate::CliError;
use crate::Result;
use serde_json::Value;
use std::any::Any;
use std::any::TypeId;
use std::sync::Arc;

pub(crate) fn primitive_json(value: &dyn Any) -> Option<Value> {
    if value
        .downcast_ref::<f32>()
        .is_some_and(|value| !value.is_finite())
        || value
            .downcast_ref::<f64>()
            .is_some_and(|value| !value.is_finite())
    {
        return None;
    }
    macro_rules! primitives {
        ($($ty:ty),*) => { $(if let Some(value) = value.downcast_ref::<$ty>() {
            return serde_json::to_value(value).ok();
        })* };
    }
    primitives!(
        String, bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64
    );
    None
}
pub(crate) fn primitive_schema<T: 'static>() -> Option<Value> {
    macro_rules! integers {
        ($($ty:ty),*) => { $(if TypeId::of::<T>() == TypeId::of::<$ty>() {
            return Some(serde_json::json!({"type":"integer", "minimum": <$ty>::MIN, "maximum": <$ty>::MAX}));
        })* };
    }
    integers!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);
    let kind = if TypeId::of::<T>() == TypeId::of::<String>() {
        "string"
    } else if TypeId::of::<T>() == TypeId::of::<bool>() {
        "boolean"
    } else if TypeId::of::<T>() == TypeId::of::<f32>() || TypeId::of::<T>() == TypeId::of::<f64>() {
        "number"
    } else {
        return None;
    };
    Some(serde_json::json!({"type": kind}))
}

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
