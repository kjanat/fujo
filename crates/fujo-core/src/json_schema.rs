use crate::command::CommandSchema;
use crate::command::ProgramSchema;
use crate::schema::InputSchema;
use crate::schema::Kind;
use serde_json::Value;
use serde_json::json;

pub fn input_schema(command: &CommandSchema) -> Value {
    fn surface(inputs: &[InputSchema]) -> Value {
        let properties: serde_json::Map<_, _> = inputs
            .iter()
            .map(|input| (input.name.clone(), property(input)))
            .collect();
        let required: Vec<_> = inputs
            .iter()
            .filter(|i| i.required)
            .map(|i| &i.name)
            .collect();
        json!({"type":"object", "properties": properties, "required": required, "additionalProperties":false})
    }
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema", "title":command.name, "type":"object", "properties":{"flags":surface(&command.flags),"args":surface(&command.args)}, "required":["flags","args"],"additionalProperties":false})
}
fn property(input: &InputSchema) -> Value {
    if let Some(schema) = &input.json_schema
        && schema.is_boolean()
    {
        return schema.clone();
    }
    let mut value = input.json_schema.clone().unwrap_or_else(|| match input.kind {
        Kind::String | Kind::Path => json!({"type":"string"}),
        Kind::Number | Kind::Duration | Kind::Bytes => json!({"type":"number"}),
        Kind::Count => json!({"type":"integer", "minimum":0}),
        Kind::Boolean => json!({"type":"boolean"}),
        Kind::Enum => json!({"type":"string", "enum":input.enum_values}),
        Kind::Array => {
            json!({"type":"array", "items":input.element.as_deref().map(property).unwrap_or(json!({})), "uniqueItems":input.unique})
        }
        Kind::KeyValue => {
            json!({"type":"object", "additionalProperties":input.element.as_deref().map(property).unwrap_or(json!({"type":"string"}))})
        }
        #[cfg(feature = "url")]
        Kind::Url => json!({"type":"string", "format":"uri"}),
        #[cfg(feature = "chrono")]
        Kind::Date => json!({"type":"string", "format":"date-time"}),
        Kind::Custom => json!({}),
    });
    if !value.is_object() {
        value = json!({});
    }
    let object = value.as_object_mut().expect("property object");
    if !input.description.is_empty() {
        object.insert("description".into(), json!(input.description));
    }
    if let Some(min) = input.min {
        object.insert("minimum".into(), json!(min));
    }
    if let Some(max) = input.max {
        object.insert("maximum".into(), json!(max));
    }
    if input.integer && input.kind == Kind::Number {
        object.insert("type".into(), json!("integer"));
    }
    if input.non_empty {
        object.insert(
            "minLength".into(),
            json!(input.min_length.unwrap_or(1).max(1)),
        );
    } else if let Some(min) = input.min_length {
        object.insert("minLength".into(), json!(min));
    }
    if let Some(max) = input.max_length {
        object.insert("maxLength".into(), json!(max));
    }
    if let Some(pattern) = &input.pattern {
        object.insert("pattern".into(), json!(pattern));
    }
    if input.sensitive {
        object.insert("writeOnly".into(), json!(true));
    } else if let Some(default) = input.default_json() {
        object.insert("default".into(), default.clone());
    }
    value
}
pub fn definition(program: &ProgramSchema) -> Value {
    json!({"format":"fujo", "version":1,"name":program.name,"description":program.description,"programVersion":program.version,"commands":program.commands.iter().map(command_definition).collect::<Vec<_>>(),"default":program.default.as_ref().map(command_definition),"builtins":program.builtins,"flags":program.flags.iter().map(element_definition).collect::<Vec<_>>()})
}

fn element_definition(input: &InputSchema) -> Value {
    let mut value = serde_json::to_value(input).expect("element serialization");
    if !input.sensitive
        && let Some(default) = input.default_json()
    {
        value["defaultValue"] = default.clone();
    }
    if (input.sensitive || input.prompt.as_ref().is_some_and(|p| p.secret))
        && let Some(prompt) = value.get_mut("prompt").and_then(Value::as_object_mut)
    {
        prompt.remove("default");
    }
    if let Some(element) = &input.element {
        value["element"] = element_definition(element);
    }
    value
}
pub fn command_definition(command: &CommandSchema) -> Value {
    json!({"name":command.name,"description":command.description,"aliases":command.aliases,"hidden":command.hidden,"flags":command.flags.iter().map(element_definition).collect::<Vec<_>>(),"args":command.args.iter().map(element_definition).collect::<Vec<_>>(),"commands":command.commands.iter().map(command_definition).collect::<Vec<_>>(),"examples":command.examples})
}
