use crate::CliError;
use crate::Result;
use crate::command::CommandSchema;
use crate::resolve::Stage;
use crate::schema::DuplicatePolicy;
use crate::schema::InputSchema;
use crate::schema::Kind;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    LongFlag { name: String, value: Option<String> },
    ShortFlags(String),
    Positional(String),
    Separator,
}
pub fn tokenize(argv: &[String]) -> Vec<Token> {
    let mut past = false;
    argv.iter()
        .map(|raw| {
            if past {
                Token::Positional(raw.clone())
            } else if raw == "--" {
                past = true;
                Token::Separator
            } else if let Some(raw) = raw.strip_prefix("--") {
                let (name, value) = raw
                    .split_once('=')
                    .map(|(n, v)| (n, Some(v.to_string())))
                    .unwrap_or((raw, None));
                Token::LongFlag {
                    name: name.into(),
                    value,
                }
            } else if raw.starts_with('-') && raw != "-" {
                Token::ShortFlags(raw[1..].into())
            } else {
                Token::Positional(raw.clone())
            }
        })
        .collect()
}
pub fn includes_before_separator(argv: &[String], token: &str) -> bool {
    argv.iter().take_while(|s| *s != "--").any(|s| s == token)
}
pub fn strip_before_separator(argv: &[String], token: &str) -> Vec<String> {
    let mut past = false;
    argv.iter()
        .filter_map(|s| {
            if s == "--" {
                past = true;
            }
            if !past && s == token {
                None
            } else {
                Some(s.clone())
            }
        })
        .collect()
}
#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
    pub case_parity: bool,
}
impl Default for ParseOptions {
    fn default() -> Self {
        Self { case_parity: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Occurrence {
    Value(Value),
    Stdin,
    Increment,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Parsed {
    pub flags: BTreeMap<String, Value>,
    pub args: BTreeMap<String, Value>,
    pub flag_occurrences: BTreeMap<String, Vec<Occurrence>>,
    pub arg_occurrences: BTreeMap<String, Vec<Occurrence>>,
}
pub(crate) fn kebab(name: &str) -> String {
    let mut output = String::new();
    let mut prev = None;
    for c in name.chars() {
        if c.is_ascii_uppercase()
            && prev.is_some_and(|p: char| p.is_ascii_lowercase() || p.is_ascii_digit())
        {
            output.push('-');
        }
        output.extend(c.to_lowercase());
        prev = Some(c);
    }
    output
}
fn counterpart(name: &str) -> Option<String> {
    let other = if name.contains('-') {
        let mut s = String::new();
        let mut upper = false;
        for c in name.chars() {
            if c == '-' {
                upper = true;
            } else {
                if upper {
                    s.extend(c.to_uppercase());
                } else {
                    s.push(c);
                }
                upper = false;
            }
        }
        s
    } else {
        kebab(name)
    };
    (other != name).then_some(other)
}
pub fn flag_lookup(
    flags: &[InputSchema],
    options: ParseOptions,
) -> BTreeMap<String, (&InputSchema, bool)> {
    let mut lookup = BTreeMap::new();
    for flag in flags {
        lookup.insert(flag.name.clone(), (flag, false));
        for alias in &flag.aliases {
            lookup.insert(alias.name.clone(), (flag, false));
        }
        if let Some(negated) = &flag.negated_name {
            lookup.insert(negated.clone(), (flag, true));
        }
    }
    if options.case_parity {
        let explicit = lookup.clone();
        for (name, entry) in &explicit {
            if let Some(other) = counterpart(name)
                && !explicit.contains_key(&other)
            {
                lookup.entry(other).or_insert(*entry);
            }
        }
    }
    lookup
}

pub fn parse(schema: &CommandSchema, argv: &[String], options: ParseOptions) -> Result<Parsed> {
    let tokens = tokenize(argv);
    let lookup = flag_lookup(&schema.flags, options);
    let mut parsed = Parsed::default();
    let mut positionals = vec![];
    let mut i = 0;
    while let Some(token) = tokens.get(i) {
        match token {
            Token::Positional(s) => {
                positionals.push(s.clone());
                i += 1;
            }
            Token::Separator => {
                i += 1;
            }
            Token::LongFlag { name, value } => {
                let (flag, negated) = lookup
                    .get(name)
                    .copied()
                    .ok_or_else(|| unknown_flag(name, &schema.flags, true))?;
                let occurrences = if negated {
                    if value.is_some() {
                        return Err(CliError::parse(
                            "INVALID_VALUE",
                            format!("Flag --{name} does not take a value"),
                        ));
                    }
                    vec![Occurrence::Value(json!(false))]
                } else if !flag.expects_value() {
                    if let Some(value) = value {
                        decode_token(flag, value)?
                    } else if flag.kind == Kind::Count {
                        vec![Occurrence::Increment]
                    } else {
                        vec![Occurrence::Value(json!(true))]
                    }
                } else {
                    let value = if let Some(value) = value {
                        value
                    } else {
                        i += 1;
                        match tokens.get(i) {
                            Some(Token::Positional(s)) => s,
                            _ => return Err(missing(flag)),
                        }
                    };
                    decode_token(flag, value)?
                };
                record(&mut parsed.flag_occurrences, flag, occurrences)?;
                i += 1;
            }
            Token::ShortFlags(chars) => {
                let iter = chars.char_indices().peekable();
                for (offset, c) in iter {
                    let (flag, _) = lookup
                        .get(&c.to_string())
                        .copied()
                        .ok_or_else(|| unknown_flag(&c.to_string(), &schema.flags, false))?;
                    let occurrences = if !flag.expects_value() {
                        if flag.kind == Kind::Count {
                            vec![Occurrence::Increment]
                        } else {
                            vec![Occurrence::Value(json!(true))]
                        }
                    } else {
                        let tail = &chars[offset + c.len_utf8()..];
                        let value = if !tail.is_empty() {
                            tail
                        } else {
                            i += 1;
                            match tokens.get(i) {
                                Some(Token::Positional(s)) => s,
                                _ => return Err(missing(flag)),
                            }
                        };
                        let values = decode_token(flag, value)?;
                        record(&mut parsed.flag_occurrences, flag, values)?;
                        break;
                    };
                    record(&mut parsed.flag_occurrences, flag, occurrences)?;
                }
                i += 1;
            }
        }
    }
    let mut cursor = 0;
    for arg in &schema.args {
        let mut occurrences = vec![];
        if arg.variadic {
            for raw in &positionals[cursor..] {
                occurrences.extend(decode_token(arg, raw)?);
            }
            cursor = positionals.len();
        } else if let Some(raw) = positionals.get(cursor) {
            occurrences.extend(decode_token(arg, raw)?);
            cursor += 1;
        }
        if !occurrences.is_empty() {
            parsed.arg_occurrences.insert(arg.name.clone(), occurrences);
        }
    }
    if cursor < positionals.len() {
        return Err(CliError::parse(
            "UNEXPECTED_POSITIONAL",
            "Unexpected positional argument",
        ));
    }
    for flag in &schema.flags {
        if let Some(occurrences) = parsed.flag_occurrences.get(&flag.name) {
            parsed
                .flags
                .insert(flag.name.clone(), project(flag, occurrences)?);
        }
    }
    for arg in &schema.args {
        if let Some(occurrences) = parsed.arg_occurrences.get(&arg.name) {
            parsed
                .args
                .insert(arg.name.clone(), project(arg, occurrences)?);
        }
    }
    Ok(parsed)
}
fn missing(flag: &InputSchema) -> CliError {
    CliError::parse(
        "MISSING_VALUE",
        format!("Flag {} requires a value", flag.display_name()),
    )
}
fn record(
    target: &mut BTreeMap<String, Vec<Occurrence>>,
    schema: &InputSchema,
    occurrences: Vec<Occurrence>,
) -> Result<()> {
    let entry = target.entry(schema.name.clone()).or_default();
    if !entry.is_empty() && !schema.collection() && schema.kind != Kind::Count {
        match schema.duplicate {
            DuplicatePolicy::Error => {
                return Err(CliError::parse(
                    "DUPLICATE_FLAG",
                    format!("Flag {} was supplied more than once", schema.display_name()),
                ));
            }
            DuplicatePolicy::First => return Ok(()),
            DuplicatePolicy::Last => entry.clear(),
        }
    }
    entry.extend(occurrences);
    Ok(())
}
pub(crate) fn decode_token(schema: &InputSchema, raw: &str) -> Result<Vec<Occurrence>> {
    if raw == "-" && schema.stdin.is_some_and(|s| s.reads_dash()) {
        return Ok(vec![Occurrence::Stdin]);
    }
    if schema.collection() {
        schema
            .cli_split
            .values(raw)?
            .into_iter()
            .map(|v| decode_element(schema, v, true).map(Occurrence::Value))
            .collect()
    } else {
        Ok(vec![Occurrence::Value(
            schema.decode_scalar(json!(raw), Stage::Cli)?,
        )])
    }
}
pub(crate) fn decode_element(schema: &InputSchema, value: Value, cli: bool) -> Result<Value> {
    let element = schema.element.as_deref().unwrap_or(schema);
    if schema.kind == Kind::KeyValue {
        let (key, raw) = value
            .as_str()
            .and_then(|s| s.split_once('='))
            .filter(|(k, _)| !k.is_empty())
            .ok_or_else(|| {
                schema.failure(
                    "TYPE_MISMATCH",
                    "Expected KEY=VALUE with a non-empty key",
                    cli,
                )
            })?;
        let decoded = element
            .decode_scalar(json!(raw), if cli { Stage::Cli } else { Stage::Env })
            .map_err(|e| schema.failure(&e.code, &e.message, cli))?;
        Ok(json!([key, decoded]))
    } else {
        element
            .decode_scalar(value, if cli { Stage::Cli } else { Stage::Env })
            .map_err(|e| schema.failure(&e.code, &e.message, cli))
    }
}
pub(crate) fn project(schema: &InputSchema, occurrences: &[Occurrence]) -> Result<Value> {
    let values: Vec<_> = occurrences
        .iter()
        .filter_map(|o| match o {
            Occurrence::Value(v) => Some(v.clone()),
            Occurrence::Stdin => Some(json!("-")),
            Occurrence::Increment => None,
        })
        .collect();
    if schema.kind == Kind::Count {
        let mut count = 0_u64;
        for o in occurrences {
            match o {
                Occurrence::Increment => {
                    count = count.checked_add(1).ok_or_else(|| {
                        schema.failure("CONSTRAINT_VIOLATED", "Count overflow", true)
                    })?
                }
                Occurrence::Value(v) => {
                    count = v
                        .as_u64()
                        .ok_or_else(|| schema.failure("TYPE_MISMATCH", "Invalid count", true))?
                }
                _ => {}
            }
        }
        Ok(json!(count))
    } else if schema.kind == Kind::KeyValue {
        let mut entries = serde_json::Map::new();
        for pair in values {
            if pair == json!("-") {
                continue;
            }
            let pair = pair
                .as_array()
                .ok_or_else(|| schema.failure("TYPE_MISMATCH", "Expected entry pair", false))?;
            let key = pair
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| schema.failure("TYPE_MISMATCH", "Expected entry key", false))?;
            let value = pair
                .get(1)
                .ok_or_else(|| schema.failure("TYPE_MISMATCH", "Expected entry value", false))?;
            if entries.contains_key(key) {
                match schema.duplicate_keys {
                    DuplicatePolicy::Error => {
                        return Err(schema.failure(
                            "CONSTRAINT_VIOLATED",
                            "Duplicate entry key",
                            false,
                        ));
                    }
                    DuplicatePolicy::First => continue,
                    DuplicatePolicy::Last => {}
                }
            }
            entries.insert(key.into(), value.clone());
        }
        Ok(Value::Object(entries))
    } else if schema.kind == Kind::Array || schema.variadic {
        let mut result = vec![];
        for value in values {
            if !schema.unique || !result.contains(&value) {
                result.push(value);
            }
        }
        Ok(Value::Array(result))
    } else {
        Ok(values.last().cloned().unwrap_or(Value::Null))
    }
}
fn unknown_flag(name: &str, flags: &[InputSchema], long: bool) -> CliError {
    let display = if long {
        format!("--{name}")
    } else {
        format!("-{name}")
    };
    let mut error = CliError::parse("UNKNOWN_FLAG", format!("Unknown flag {display}"));
    if long {
        let candidates: Vec<_> = flags
            .iter()
            .filter(|f| !f.hidden)
            .flat_map(|f| {
                std::iter::once(f.name.as_str())
                    .chain(
                        f.aliases
                            .iter()
                            .filter(|a| !a.hidden && a.name.len() > 1)
                            .map(|a| a.name.as_str()),
                    )
                    .chain(f.negated_name.as_deref())
            })
            .collect();
        if let Some(suggest) = suggest_command(name, candidates.into_iter()) {
            error.suggest = Some(format!("--{suggest}"));
        }
    }
    error
}
pub fn suggest_command<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<String> {
    candidates
        .map(|candidate| (distance(name, candidate), candidate))
        .filter(|(d, _)| *d <= 3)
        .min_by_key(|(d, _)| *d)
        .map(|(_, s)| s.into())
}
fn distance(a: &str, b: &str) -> usize {
    let mut previous: Vec<_> = (0..=b.chars().count()).collect();
    for (i, ac) in a.chars().enumerate() {
        let mut current = vec![i + 1];
        for (j, bc) in b.chars().enumerate() {
            current.push(
                (previous[j] + usize::from(ac != bc))
                    .min(previous[j + 1] + 1)
                    .min(current[j] + 1),
            );
        }
        previous = current;
    }
    *previous.last().expect("distance buffer")
}
