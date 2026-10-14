use super::*;
use chrono::DateTime;
use chrono::NaiveDate;
use chrono::NaiveDateTime;
use chrono::Utc;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

pub(super) fn typed_schema<T: Serialize + DeserializeOwned + Send + Sync + 'static>(
    mut schema: InputSchema,
) -> InputSchema {
    schema.native_decoder = Some(Arc::new(|json| {
        Ok(NativeValue {
            inner: Arc::new(serde_json::from_value::<T>(json.clone())?),
            json: Some(json.clone()),
            sensitive: false,
        })
    }));
    schema.encoder = Some(Arc::new(|native| {
        serde_json::to_value(native.inner.downcast_ref::<T>()?).ok()
    }));
    schema
}
pub(super) fn collection_schema<T: Clone + Send + Sync + 'static>(
    name: &str,
    kind: Kind,
    element: InputSchema,
) -> InputSchema {
    let mut schema = InputSchema::new(name, kind, Surface::Flag);
    schema.declaration.sensitive = element.sensitive;
    let decoder = element.native_decoder.clone();
    schema.native_decoder = Some(Arc::new(move |json| {
        let decode = |value: &Value| -> Result<T> {
            let native = decoder
                .as_ref()
                .ok_or_else(|| CliError::new("INVALID_SCHEMA", "Missing element codec"))?(
                value
            )?;

            Ok(native.downcast_ref::<T>()?.clone())
        };
        let native = if kind == Kind::KeyValue {
            let entries = json
                .as_object()
                .ok_or_else(|| CliError::validation("TYPE_MISMATCH", "Expected entries"))?;
            NativeValue::new(
                entries
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), decode(v)?)))
                    .collect::<Result<BTreeMap<String, T>>>()?,
            )
        } else {
            let values = json
                .as_array()
                .ok_or_else(|| CliError::validation("TYPE_MISMATCH", "Expected an array"))?;
            NativeValue::new(values.iter().map(decode).collect::<Result<Vec<T>>>()?)
        };
        Ok(native)
    }));
    schema.encoder = element
        .encoder
        .clone()
        .map(|encode| collection_encoder::<T>(kind, encode));
    schema.validation_encoder = element
        .validation_encoder
        .clone()
        .or_else(|| element.encoder.clone())
        .map(|encode| collection_encoder::<T>(kind, encode));
    let element_validators = element.native_validators.clone();
    schema.native_validators.push(Arc::new(move |native| {
        let values: Vec<&T> = if kind == Kind::KeyValue {
            native
                .downcast_ref::<BTreeMap<String, T>>()?
                .values()
                .collect()
        } else {
            native.downcast_ref::<Vec<T>>()?.iter().collect()
        };
        for value in values {
            let native = NativeValue::new(value.clone());
            for validate in &element_validators {
                validate(&native)?;
            }
        }
        Ok(())
    }));
    schema.declaration.element = Some(Box::new(element));
    schema
}

fn collection_encoder<T: Clone + Send + Sync + 'static>(kind: Kind, encode: Encoder) -> Encoder {
    Arc::new(move |native| {
        if kind == Kind::KeyValue {
            native
                .inner
                .downcast_ref::<BTreeMap<String, T>>()?
                .iter()
                .map(|(k, v)| encode(&NativeValue::new(v.clone())).map(|v| (k.clone(), v)))
                .collect::<Option<serde_json::Map<String, Value>>>()
                .map(Value::Object)
        } else {
            native
                .inner
                .downcast_ref::<Vec<T>>()?
                .iter()
                .map(|v| encode(&NativeValue::new(v.clone())))
                .collect::<Option<Vec<_>>>()
                .map(Value::Array)
        }
    })
}

pub(super) fn parse_date(s: &str) -> Result<DateTime<Utc>> {
    let pattern = Regex::new(
        r"^\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}(?::\d{2}(?:\.\d{1,3})?)?(?:Z|[+-]\d{2}:\d{2})?)?$",
    )
    .expect("constant regex");
    let fail = || CliError::validation("CONSTRAINT_VIOLATED", "Expected a valid ISO-8601 date");
    if !pattern.is_match(s) {
        return Err(fail());
    }
    if s.len() == 10 {
        return Ok(NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| fail())?
            .and_hms_opt(0, 0, 0)
            .ok_or_else(fail)?
            .and_utc());
    }
    // Chrono accepts leap seconds; JavaScript's strict codec rejects them.
    if s.get(17..19) == Some("60") {
        return Err(fail());
    }
    let mut normalized = s.to_owned();
    if s.len() >= 16 && (s.len() == 16 || s.as_bytes().get(16) != Some(&b':')) {
        normalized.insert_str(16, ":00");
    }
    if normalized.ends_with('Z')
        || normalized
            .get(19..)
            .is_some_and(|tail| tail.contains('+') || tail.contains('-'))
    {
        DateTime::parse_from_rfc3339(&normalized)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|_| fail())
    } else {
        NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f")
            .map(|d| d.and_utc())
            .map_err(|_| fail())
    }
}
pub(super) fn parse_duration(raw: &Value) -> Result<f64> {
    let fail = || {
        CliError::validation(
            "CONSTRAINT_VIOLATED",
            "Expected a non-negative duration, e.g. 1h30m or 250ms",
        )
    };
    if let Some(n) = raw.as_f64() {
        return if n >= 0.0 && n.is_finite() {
            Ok(n)
        } else {
            Err(fail())
        };
    }
    let text = raw.as_str().ok_or_else(fail)?;
    let bare = Regex::new(r"^\d+(?:\.\d+)?$").expect("constant regex");
    if bare.is_match(text) {
        let n = text.parse::<f64>().map_err(|_| fail())?;
        return if n.is_finite() { Ok(n) } else { Err(fail()) };
    }
    let regex = Regex::new(r"(\d+(?:\.\d+)?)(ms|s|m|h|d)").expect("constant regex");
    let mut consumed = 0;
    let mut total = 0.0;
    for capture in regex.captures_iter(text) {
        let matched = capture.get(0).expect("full match");
        if matched.start() != consumed {
            return Err(fail());
        }
        consumed = matched.end();
        let multiplier = match &capture[2] {
            "ms" => 1.0,
            "s" => 1000.0,
            "m" => 60_000.0,
            "h" => 3_600_000.0,
            _ => 86_400_000.0,
        };
        total += capture[1].parse::<f64>().map_err(|_| fail())? * multiplier;
    }
    if consumed != text.len() || consumed == 0 || !total.is_finite() {
        Err(fail())
    } else {
        Ok(total)
    }
}
pub(super) fn parse_bytes(raw: &Value) -> Result<f64> {
    let fail = || {
        CliError::validation(
            "CONSTRAINT_VIOLATED",
            "Expected a non-negative size, e.g. 512mb or 1.5gb",
        )
    };
    if let Some(n) = raw.as_f64() {
        return if n >= 0.0 && n.is_finite() {
            Ok(n)
        } else {
            Err(fail())
        };
    }
    let text = raw.as_str().ok_or_else(fail)?;
    let regex = Regex::new(r"(?i)^(\d+(?:\.\d+)?)\s*(b|kb|mb|gb|tb)?$").expect("constant regex");
    let capture = regex.captures(text).ok_or_else(fail)?;
    let power = match capture.get(2).map(|m| m.as_str().to_lowercase()).as_deref() {
        Some("kb") => 1,
        Some("mb") => 2,
        Some("gb") => 3,
        Some("tb") => 4,
        _ => 0,
    };
    let n = (capture[1].parse::<f64>().map_err(|_| fail())? * 1024_f64.powi(power)).round();
    if n.is_finite() { Ok(n) } else { Err(fail()) }
}

pub(super) fn parse_number(text: &str) -> std::result::Result<f64, ()> {
    let radix = if text.starts_with("0x") || text.starts_with("0X") {
        Some(16)
    } else if text.starts_with("0o") || text.starts_with("0O") {
        Some(8)
    } else if text.starts_with("0b") || text.starts_with("0B") {
        Some(2)
    } else {
        None
    };
    if let Some(radix) = radix {
        if text.len() == 2 {
            return Err(());
        }
        text[2..].chars().try_fold(0.0, |value, c| {
            c.to_digit(radix)
                .map(|digit| value * f64::from(radix) + f64::from(digit))
                .ok_or(())
        })
    } else {
        text.parse().map_err(|_| ())
    }
}
