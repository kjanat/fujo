use crate::CliError;
use crate::Result;
use crate::command::CommandSchema;
use crate::parse::Occurrence;
use crate::parse::Parsed;
use crate::parse::decode_element;
use crate::parse::project;
use crate::prompt::PromptEngine;
use crate::prompt::PromptKind;
use crate::schema::Input;
use crate::schema::InputSchema;
use crate::schema::Kind;
use crate::schema::PathType;
use crate::schema::Presence;
use crate::schema::Split;
use crate::schema::Surface;
use crate::value::NativeValue;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Cli,
    Stdin,
    Env,
    Config,
    Prompt,
    Default,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub stage: Stage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}
impl Provenance {
    fn at(stage: Stage) -> Self {
        Self {
            stage,
            via: None,
            trigger: None,
            env_var: None,
            config_path: None,
        }
    }
}
pub fn was_explicit(source: Option<&Provenance>) -> bool {
    source.is_some_and(|s| s.stage != Stage::Default)
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct InputSources {
    pub flags: BTreeMap<String, Provenance>,
    pub args: BTreeMap<String, Provenance>,
}

/// Filesystem checks are explicit capabilities, shared by native and test adapters.
pub trait FileSystem: Send + Sync {
    fn stat(&self, path: &str) -> Result<Option<PathType>>;
    fn mkdir(&self, path: &str) -> Result<()>;
}
pub type Interactive =
    Arc<dyn Fn(&Resolved) -> Result<BTreeMap<String, Option<crate::prompt::Prompt>>> + Send + Sync>;
#[derive(Clone, Default)]
pub struct ResolveOptions {
    pub env: BTreeMap<String, String>,
    pub config: Value,
    pub stdin: Option<String>,
    pub prompter: Option<Arc<dyn PromptEngine>>,
    pub filesystem: Option<Arc<dyn FileSystem>>,
    pub interactive: Option<Interactive>,
}
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    pub flags: BTreeMap<String, NativeValue>,
    pub args: BTreeMap<String, NativeValue>,
    pub sources: InputSources,
    pub deprecations: Vec<String>,
}
impl Resolved {
    pub fn get<T: Clone + Send + Sync + 'static, P: Presence<T>, S>(
        &self,
        input: &Input<T, P, S>,
    ) -> Result<P::Output> {
        let value = match input.schema().surface {
            Surface::Flag => &self.flags,
            Surface::Arg => &self.args,
        }
        .get(input.name());
        P::read(
            value.map(|v| v.downcast_ref::<T>().cloned()).transpose()?,
            input.name(),
        )
    }
    pub fn get_ref<T: 'static, P: Presence<T>, S>(
        &self,
        input: &Input<T, P, S>,
    ) -> Result<P::Ref<'_>> {
        P::read_ref(self.peek(input)?, input.name())
    }
    /// Read a possibly unresolved handle, useful inside conditional prompt selection.
    pub fn peek<T: 'static, P, S>(&self, input: &Input<T, P, S>) -> Result<Option<&T>> {
        match input.schema().surface {
            Surface::Flag => &self.flags,
            Surface::Arg => &self.args,
        }
        .get(input.name())
        .map(|v| v.downcast_ref())
        .transpose()
    }
    /// Only values with a JSON representation are exported. Native custom values remain opaque.
    pub fn json(&self) -> Value {
        let surface = |values: &BTreeMap<String, NativeValue>| {
            Value::Object(
                values
                    .iter()
                    .filter_map(|(k, v)| {
                        if v.sensitive {
                            Some((k.clone(), json!("[redacted]")))
                        } else {
                            v.json.clone().map(|v| (k.clone(), v))
                        }
                    })
                    .collect(),
            )
        };
        json!({ "flags": surface(&self.flags), "args": surface(&self.args) })
    }
}

pub fn resolve(
    schema: &CommandSchema,
    parsed: &Parsed,
    options: &ResolveOptions,
) -> Result<Resolved> {
    let mut resolved = Resolved::default();
    let mut errors = vec![];
    let mut hard_errors = std::collections::BTreeSet::new();
    // Resolve explicit sources for the whole command before deciding which prompts to ask.
    for input in schema.flags.iter().chain(&schema.args) {
        let occurrences = match input.surface {
            Surface::Flag => &parsed.flag_occurrences,
            Surface::Arg => &parsed.arg_occurrences,
        }
        .get(&input.name);
        match resolve_input(input, occurrences.map(Vec::as_slice), options, false) {
            Ok(Some((value, source))) => {
                record(&mut resolved, input, value, source);
            }
            Ok(None) => {}
            Err(error) => {
                if error.name == crate::error::ErrorKind::Cli {
                    return Err(error);
                }
                hard_errors.insert((input.surface as u8, input.name.clone()));
                errors.push(error);
            }
        }
    }
    let interactive = options
        .interactive
        .as_ref()
        .map(|interactive| interactive(&resolved))
        .transpose()?
        .unwrap_or_default();
    for input in schema.flags.iter().chain(&schema.args) {
        let values = if input.surface == Surface::Flag {
            &resolved.flags
        } else {
            &resolved.args
        };
        if values.contains_key(&input.name)
            || hard_errors.contains(&(input.surface as u8, input.name.clone()))
        {
            continue;
        }
        let mut input = input.clone();
        if input.surface == Surface::Flag
            && let Some(prompt) = interactive.get(&input.name)
        {
            input.prompt = prompt.clone();
        }
        match resolve_input(&input, None, options, true) {
            Ok(Some((value, source))) => record(&mut resolved, &input, value, source),
            Ok(None) => {}
            Err(error) => {
                if error.name == crate::error::ErrorKind::Cli {
                    return Err(error);
                }
                errors.push(error);
            }
        }
    }
    if errors.is_empty() {
        Ok(resolved)
    } else {
        let mut errors = errors.into_iter();
        Err(CliError::aggregate(
            errors.next().expect("non-empty error list"),
            errors,
        ))
    }
}
fn record(
    resolved: &mut Resolved,
    input: &InputSchema,
    mut value: NativeValue,
    source: Provenance,
) {
    value.sensitive = input.sensitive;
    if source.stage != Stage::Default
        && let Some(message) = &input.deprecated
    {
        resolved
            .deprecations
            .push(format!("{} is deprecated: {message}", input.display_name()));
    }
    match input.surface {
        Surface::Flag => {
            resolved.flags.insert(input.name.clone(), value);
            resolved.sources.flags.insert(input.name.clone(), source);
        }
        Surface::Arg => {
            resolved.args.insert(input.name.clone(), value);
            resolved.sources.args.insert(input.name.clone(), source);
        }
    }
}
fn resolve_input(
    input: &InputSchema,
    occurrences: Option<&[Occurrence]>,
    options: &ResolveOptions,
    later: bool,
) -> Result<Option<(NativeValue, Provenance)>> {
    let mut selected = None;
    if let Some(occurrences) = occurrences {
        let mut expanded = vec![];
        let mut stdin = false;
        for occurrence in occurrences {
            if matches!(occurrence, Occurrence::Stdin) {
                stdin = true;
                if let Some(buffer) = options.stdin.as_ref() {
                    expanded.extend(stdin_occurrences(input, buffer)?);
                } else if input.collection() {
                    return Err(CliError::validation(
                        "MISSING_STDIN",
                        format!("{} requires stdin", input.display_name()),
                    ));
                }
            } else {
                expanded.push(occurrence.clone());
            }
        }
        let mut source = Provenance::at(Stage::Cli);
        if stdin {
            source.via = Some("stdin".into());
            source.trigger = Some("dash".into());
        }
        if !expanded.is_empty() || input.collection() {
            selected = Some((project(input, &expanded)?, source));
        }
    }
    if selected.is_none()
        && input.stdin.is_some_and(|s| s.reads_missing())
        && let Some(buffer) = &options.stdin
    {
        let source = Provenance {
            via: Some("stdin".into()),
            trigger: Some("fallback".into()),
            ..Provenance::at(Stage::Stdin)
        };
        selected = Some((project(input, &stdin_occurrences(input, buffer)?)?, source));
    }
    if selected.is_none() {
        for env in &input.env_vars {
            if let Some(value) = options.env.get(env) {
                selected = Some((
                    coerce(input, json!(value), &input.env_split, Stage::Env)?,
                    Provenance {
                        env_var: Some(env.clone()),
                        ..Provenance::at(Stage::Env)
                    },
                ));
                break;
            }
        }
    }
    if selected.is_none()
        && let Some(path) = &input.config_path
        && let Some(value) = config_value(&options.config, path).filter(|v| !v.is_null())
    {
        selected = Some((
            coerce(input, value.clone(), &input.env_split, Stage::Config)?,
            Provenance {
                config_path: Some(path.clone()),
                ..Provenance::at(Stage::Config)
            },
        ));
    }
    if selected.is_none() && !later {
        return Ok(None);
    }
    if selected.is_none()
        && let (Some(prompt), Some(engine)) = (&input.prompt, &options.prompter)
    {
        let mut prompt = prompt.clone();
        let compatible = match input.kind {
            Kind::Count | Kind::KeyValue => false,
            Kind::Boolean => prompt.kind == PromptKind::Confirm,
            Kind::Array => prompt.kind == PromptKind::Multiselect,
            Kind::Custom => true,
            Kind::Enum | Kind::String | Kind::Path => {
                matches!(prompt.kind, PromptKind::Input | PromptKind::Select)
            }
            _ => prompt.kind == PromptKind::Input,
        };
        if !compatible {
            return Err(input.failure(
                "CONSTRAINT_VIOLATED",
                "Prompt kind is incompatible with the input",
                false,
            ));
        }
        if prompt.choices.is_empty() && input.kind == Kind::Enum {
            prompt.choices = input
                .enum_values
                .iter()
                .map(|v| crate::prompt::Choice::new(v.as_str(), v))
                .collect();
        }
        if let Some(value) = engine.prompt(&prompt)? {
            if !prompt.choices.is_empty() {
                let values = if prompt.kind == PromptKind::Multiselect {
                    value
                        .as_array()
                        .cloned()
                        .unwrap_or_else(|| vec![value.clone()])
                } else {
                    vec![value.clone()]
                };
                if values
                    .iter()
                    .any(|v| !prompt.choices.iter().any(|c| c.value == *v))
                {
                    return Err(input.failure(
                        "CONSTRAINT_VIOLATED",
                        "Answer is not a prompt choice",
                        false,
                    ));
                }
            }
            selected = Some((
                coerce(input, value, &input.env_split, Stage::Prompt)?,
                Provenance::at(Stage::Prompt),
            ));
        }
    }
    let (native, source) = if let Some((value, source)) = selected {
        validate_paths(input, &value, options)?;
        let native = input
            .native_decoder
            .as_ref()
            .ok_or_else(|| CliError::new("INVALID_SCHEMA", "Missing native codec"))?(
            &value
        )
        .map_err(|e| input.failure(&e.code, &e.message, source.stage == Stage::Cli))?;
        (native, source)
    } else if let Some(default) = &input.native_default {
        if let Some(value) = default.json() {
            validate_default(input, value)?;
            validate_paths(input, value, options)?;
        }
        (default.clone(), Provenance::at(Stage::Default))
    } else if input.required {
        return Err(CliError::validation(
            if input.surface == Surface::Flag {
                "REQUIRED_FLAG"
            } else {
                "REQUIRED_ARG"
            },
            format!("Required {} is missing", input.display_name()),
        ));
    } else {
        return Ok(None);
    };
    for validate in &input.native_validators {
        validate(&native).map_err(|e| input.failure(&e.code, &e.message, false))?;
    }
    Ok(Some((native, source)))
}
pub(crate) fn validate_default(input: &InputSchema, value: &Value) -> Result<()> {
    if input.kind == Kind::Array {
        let array = value
            .as_array()
            .ok_or_else(|| input.failure("TYPE_MISMATCH", "Expected an array default", false))?;
        for value in array {
            input
                .element
                .as_deref()
                .expect("array element")
                .decode_scalar(value.clone(), Stage::Default)?;
        }
        input.validate_scalar(value)
    } else if input.kind == Kind::KeyValue {
        let values = value
            .as_object()
            .ok_or_else(|| input.failure("TYPE_MISMATCH", "Expected an entry default", false))?;
        for value in values.values() {
            input
                .element
                .as_deref()
                .expect("entry element")
                .decode_scalar(value.clone(), Stage::Default)?;
        }
        input.validate_scalar(value)
    } else {
        input
            .decode_scalar(value.clone(), Stage::Default)
            .map(|_| ())
    }
}
fn stdin_occurrences(input: &InputSchema, buffer: &str) -> Result<Vec<Occurrence>> {
    if input.collection() {
        collection_text(input, buffer, &input.stdin_split, Stage::Stdin)
    } else {
        let raw = if input.stdin.is_some_and(|s| s.trim)
            || !matches!(input.kind, Kind::String | Kind::Path | Kind::Custom)
        {
            strip_terminator(buffer)
        } else {
            buffer
        };
        Ok(vec![Occurrence::Value(
            input.decode_scalar(json!(raw), Stage::Stdin)?,
        )])
    }
}
pub fn strip_terminator(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .or_else(|| text.strip_suffix('\r'))
        .unwrap_or(text)
}
pub fn config_value<'a>(config: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(config, |value, key| value.as_object()?.get(key))
}
fn coerce(input: &InputSchema, value: Value, split: &Split, source: Stage) -> Result<Value> {
    if !input.collection() {
        return input.decode_scalar(value, source);
    }
    let values = match &value {
        Value::Array(values) if input.kind != Kind::KeyValue => values.clone(),
        Value::Array(values) => {
            let element = input.element.as_deref().expect("entry element");
            return project(
                input,
                &values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        element
                            .decode_scalar(value.clone(), source)
                            .map(|value| Occurrence::Value(json!([index.to_string(), value])))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        Value::Object(entries) if input.kind == Kind::KeyValue => {
            let element = input.element.as_deref().expect("entry element");
            return project(
                input,
                &entries
                    .iter()
                    .map(|(k, v)| {
                        element
                            .decode_scalar(v.clone(), source)
                            .map(|v| Occurrence::Value(json!([k, v])))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        Value::String(text) => {
            return project(input, &collection_text(input, text, split, source)?);
        }
        _ => {
            return Err(input.failure(
                "TYPE_MISMATCH",
                "Expected an array or collection text",
                false,
            ));
        }
    };
    project(
        input,
        &values
            .into_iter()
            .map(|v| decode_collection_element(input, v, source).map(Occurrence::Value))
            .collect::<Result<Vec<_>>>()?,
    )
}
fn decode_collection_element(input: &InputSchema, value: Value, source: Stage) -> Result<Value> {
    if input.kind == Kind::KeyValue {
        // Text entries carry string values; decoding config JSON objects happens separately.
        decode_element(input, value, false)
    } else {
        input
            .element
            .as_deref()
            .unwrap_or(input)
            .decode_scalar(value, source)
            .map_err(|e| input.failure(&e.code, &e.message, false))
    }
}
fn collection_text(
    input: &InputSchema,
    text: &str,
    split: &Split,
    source: Stage,
) -> Result<Vec<Occurrence>> {
    if input.kind == Kind::KeyValue && *split == Split::Json {
        let value: Value = serde_json::from_str(text)
            .map_err(|_| input.failure("TYPE_MISMATCH", "Expected valid JSON", false))?;
        let entries = value
            .as_object()
            .ok_or_else(|| input.failure("TYPE_MISMATCH", "Expected a JSON object", false))?;
        return entries
            .iter()
            .map(|(key, value)| {
                input
                    .element
                    .as_deref()
                    .expect("entry element")
                    .decode_scalar(value.clone(), source)
                    .map(|value| Occurrence::Value(json!([key, value])))
            })
            .collect();
    }
    if text.is_empty() && *split == Split::Whole {
        return Ok(vec![]);
    }
    split
        .values(text)
        .map_err(|e| input.failure(&e.code, &e.message, false))?
        .into_iter()
        .map(|v| decode_collection_element(input, v, source).map(Occurrence::Value))
        .collect()
}

fn validate_paths(input: &InputSchema, value: &Value, options: &ResolveOptions) -> Result<()> {
    let element = input.element.as_deref().unwrap_or(input);
    let (Some(checks), Some(fs)) = (&element.path_checks, &options.filesystem) else {
        return Ok(());
    };
    let values = match value {
        Value::Array(values) => values.iter().collect::<Vec<_>>(),
        Value::Object(values) => values.values().collect(),
        _ => vec![value],
    };
    for value in values {
        let Some(path) = value.as_str() else {
            continue;
        };
        let mut actual = fs
            .stat(path)
            .map_err(|e| input.failure("CONSTRAINT_VIOLATED", &e.message, false))?;
        if actual.is_none() && checks.create {
            fs.mkdir(path)
                .map_err(|e| input.failure("CONSTRAINT_VIOLATED", &e.message, false))?;
            actual = fs.stat(path)?;
        }
        if actual.is_none() && checks.must_exist {
            return Err(input.failure("CONSTRAINT_VIOLATED", "Path does not exist", false));
        }
        if actual.is_some() && checks.path_type.is_some() && actual != checks.path_type {
            return Err(input.failure("CONSTRAINT_VIOLATED", "Path has the wrong type", false));
        }
    }
    Ok(())
}

/// Testable flag-only resolution; no process-global reads or output.
pub fn read_flags(
    schema: &[InputSchema],
    argv: &[String],
    options: &ResolveOptions,
) -> Result<Resolved> {
    let command = CommandSchema {
        name: "flags".into(),
        description: String::new(),
        aliases: vec![],
        hidden: false,
        flags: schema.to_vec(),
        args: vec![],
        commands: vec![],
        examples: vec![],
    };
    command.validate()?;
    resolve(
        &command,
        &crate::parse::parse(&command, argv, Default::default())?,
        options,
    )
}
