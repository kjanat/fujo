use super::*;
#[cfg(feature = "chrono")]
use chrono::DateTime;
#[cfg(feature = "chrono")]
use chrono::NaiveDate;
#[cfg(feature = "chrono")]
use chrono::NaiveDateTime;
#[cfg(feature = "chrono")]
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

#[cfg(feature = "chrono")]
pub(super) fn parse_date(s: &str) -> Result<DateTime<Utc>> {
    let fail = || CliError::validation("CONSTRAINT_VIOLATED", "Expected a valid ISO-8601 date");
    if !iso_date_shape(s) {
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
    let (number, suffix) = decimal_prefix(text).ok_or_else(fail)?;
    if suffix.is_empty() {
        let n = number.parse::<f64>().map_err(|_| fail())?;
        return if n.is_finite() { Ok(n) } else { Err(fail()) };
    }
    let mut rest = text;
    let mut total = 0.0;
    while !rest.is_empty() {
        let (number, suffix) = decimal_prefix(rest).ok_or_else(fail)?;
        let (multiplier, tail) = if let Some(tail) = suffix.strip_prefix("ms") {
            (1.0, tail)
        } else if let Some(tail) = suffix.strip_prefix('s') {
            (1000.0, tail)
        } else if let Some(tail) = suffix.strip_prefix('m') {
            (60_000.0, tail)
        } else if let Some(tail) = suffix.strip_prefix('h') {
            (3_600_000.0, tail)
        } else if let Some(tail) = suffix.strip_prefix('d') {
            (86_400_000.0, tail)
        } else {
            return Err(fail());
        };
        total += number.parse::<f64>().map_err(|_| fail())? * multiplier;
        rest = tail;
    }
    if total.is_finite() {
        Ok(total)
    } else {
        Err(fail())
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
    let (number, suffix) = decimal_prefix(text).ok_or_else(fail)?;
    let power = match suffix.trim_start().to_lowercase().as_str() {
        "" | "b" => 0,
        "kb" => 1,
        "mb" => 2,
        "gb" => 3,
        "tb" => 4,
        _ => return Err(fail()),
    };
    let n = (number.parse::<f64>().map_err(|_| fail())? * 1024_f64.powi(power)).round();
    if n.is_finite() { Ok(n) } else { Err(fail()) }
}

/// Match the decimal grammar shared by durations and sizes without regex.
fn decimal_prefix(text: &str) -> Option<(&str, &str)> {
    let mut end = text.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    if text.as_bytes().get(end) == Some(&b'.') {
        let fraction = text[end + 1..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if fraction == 0 {
            return None;
        }
        end += 1 + fraction;
    }
    Some(text.split_at(end))
}

#[cfg(feature = "chrono")]
fn iso_date_shape(text: &str) -> bool {
    fn digits(text: &str, start: usize, end: usize) -> bool {
        text.get(start..end)
            .is_some_and(|part| part.bytes().all(|b| b.is_ascii_digit()))
    }
    if !text.is_ascii()
        || !digits(text, 0, 4)
        || text.as_bytes().get(4) != Some(&b'-')
        || !digits(text, 5, 7)
        || text.as_bytes().get(7) != Some(&b'-')
        || !digits(text, 8, 10)
    {
        return false;
    }
    if text.len() == 10 {
        return true;
    }
    if text.as_bytes().get(10) != Some(&b'T')
        || !digits(text, 11, 13)
        || text.as_bytes().get(13) != Some(&b':')
        || !digits(text, 14, 16)
    {
        return false;
    }
    let mut rest = &text[16..];
    if let Some(seconds) = rest.strip_prefix(':') {
        if !digits(seconds, 0, 2) {
            return false;
        }
        rest = &seconds[2..];
        if let Some(fraction) = rest.strip_prefix('.') {
            let length = fraction.bytes().take_while(u8::is_ascii_digit).count();
            if !(1..=3).contains(&length) {
                return false;
            }
            rest = &fraction[length..];
        }
    }
    rest.is_empty()
        || rest == "Z"
        || (rest.len() == 6
            && matches!(rest.as_bytes()[0], b'+' | b'-')
            && digits(rest, 1, 3)
            && rest.as_bytes()[3] == b':'
            && digits(rest, 4, 6))
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

#[cfg(all(test, feature = "regex"))]
mod migration_tests {
    use super::*;

    #[test]
    fn duration_and_size_scanners_preserve_the_previous_accepted_grammar() {
        // Retain the old regex grammar as an independent oracle for the scanner
        // migration, including whitespace, case folding, partials, and overflow.
        let duration = Regex::new(r"^(?:\d+(?:\.\d+)?|(?:\d+(?:\.\d+)?(?:ms|s|m|h|d))+)$").unwrap();
        let bytes = Regex::new(r"(?i)^(\d+(?:\.\d+)?)\s*(b|kb|mb|gb|tb)?$").unwrap();
        for number in [
            "", "0", "01", "12", "1.5", ".5", "1.", "+1", "-1", "1e3", "١",
        ] {
            for suffix in [
                "",
                "ms",
                "s",
                "m",
                "h",
                "d",
                "1ms",
                "30m",
                "30",
                "S",
                " ",
                "kb",
                "\u{2003}KB",
                " Kb",
                "mb",
                "TB",
                "b ",
                "\n",
            ] {
                let text = format!("{number}{suffix}");
                assert_eq!(
                    parse_duration(&json!(text)).is_ok(),
                    duration.is_match(&text) && text.is_ascii(),
                    "duration: {text:?}"
                );
                assert_eq!(
                    parse_bytes(&json!(text)).is_ok(),
                    bytes.is_match(&text) && number.is_ascii(),
                    "size: {text:?}"
                );
            }
        }
        let overflow = "9".repeat(400);
        assert!(parse_duration(&json!(format!("{overflow}d"))).is_err());
        assert!(parse_bytes(&json!(format!("{overflow}tb"))).is_err());
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn iso_shape_preserves_the_previous_grammar_and_handles_truncated_input() {
        let old = Regex::new(
            r"^\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}(?::\d{2}(?:\.\d{1,3})?)?(?:Z|[+-]\d{2}:\d{2})?)?$",
        )
        .unwrap();
        for date in ["2026-10-15", "2026-02-31", "2026/10/15", "２０２６-10-15"] {
            for clock in [
                "",
                "T12:30",
                "T12:30:25",
                "T12:30:25.1",
                "T12:30:25.123",
                "T12:30:25.1234",
                "T12:30.1",
                "t12:30",
            ] {
                for zone in ["", "Z", "+02:00", "-02:30", "+0200", "z", "\n"] {
                    let text = format!("{date}{clock}{zone}");
                    for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
                        let text = &text[..end];
                        assert_eq!(
                            iso_date_shape(text),
                            old.is_match(text) && text.is_ascii(),
                            "{text:?}"
                        );
                    }
                }
            }
        }
    }
}
