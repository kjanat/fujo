use dreamcli::Value;
use dreamcli::arg;
use dreamcli::command;
use dreamcli::flag;
use dreamcli::parse::parse;
use dreamcli::resolve::ResolveOptions;
use dreamcli::resolve::resolve;
use dreamcli::schema::Kind;
use dreamcli::schema::StdinBinding;
use dreamcli::value;
use serde_json::json;

fn schema(name: &str) -> dreamcli::Command {
    match name {
        "basic" => command("basic")
            .flag(
                flag::string("name")
                    .alias("n")
                    .env("NAME")
                    .config("app.name")
                    .default("default".into()),
            )
            .flag(
                flag::number("port")
                    .alias("p")
                    .env("PORT")
                    .config("app.port"),
            )
            .flag(flag::count("verbose").alias("v"))
            .flag(flag::boolean("force").alias("f").negatable().env("FORCE"))
            .flag(
                flag::enumeration("region", ["us", "eu"])
                    .env("REGION")
                    .config("app.region"),
            )
            .flag(
                flag::array("tag", value::string())
                    .separator(",")
                    .unique()
                    .env("TAGS"),
            )
            .flag(flag::key_value("define").alias("D").env("DEFINES"))
            .flag(flag::duration("timeout").env("TIMEOUT"))
            .flag(flag::bytes("size"))
            .flag(flag::date("date"))
            .flag(flag::url("url"))
            .flag(flag::string("outputPath"))
            .arg(arg::string("target").optional()),
        "required" => command("required")
            .flag(flag::string("token").required())
            .arg(arg::string("target")),
        "stdin" => command("stdin").flag(
            flag::string("text")
                .stdin()
                .env("TEXT")
                .default("default".into()),
        ),
        "list" => command("list").flag(flag::array("tag", value::string()).stdin().env("TAGS")),
        "trim" => command("trim").arg(arg::string("path").stdin_with(StdinBinding {
            trim: true,
            ..Default::default()
        })),
        "arraysjson" => command("arraysjson").flag(
            flag::array("value", value::number())
                .env("VALUE")
                .config("value")
                .stdin()
                .split_env(dreamcli::schema::Split::Json)
                .split_stdin(dreamcli::schema::Split::Json),
        ),
        "entriesjson" => command("entriesjson").flag(
            flag::key_value_of("value", value::number())
                .default(Default::default())
                .env("VALUE")
                .config("value")
                .stdin()
                .split_env(dreamcli::schema::Split::Json)
                .split_stdin(dreamcli::schema::Split::Json),
        ),
        "bounds" => command("bounds")
            .flag(
                flag::number("port")
                    .integer()
                    .min(1.0)
                    .max(10.0)
                    .env("PORT"),
            )
            .flag(
                flag::string("name")
                    .non_empty()
                    .min_length(2)
                    .max_length(5)
                    .pattern("^[a-z]+$")
                    .env("NAME"),
            ),
        "rest" => command("rest").arg(arg::number("value").variadic()),
        "entriesrest" => command("entriesrest").arg(arg::key_value("value").variadic_entries()),
        _ => panic!("Unknown reference schema"),
    }
}
fn normalize(value: &mut Value, schema: &dreamcli::command::CommandSchema) {
    // DateTime's serde format uses +00:00; JavaScript serializes the same instant as Z.
    for input in schema.flags.iter().chain(&schema.args) {
        if input.kind == Kind::Date {
            let surface = if input.surface == dreamcli::schema::Surface::Flag {
                "flags"
            } else {
                "args"
            };
            if let Some(text) = value[surface][&input.name].as_str() {
                let text = text.replace("+00:00", "Z");
                value[surface][&input.name] =
                    json!(if text.ends_with('Z') && !text.contains('.') {
                        text.replace('Z', ".000Z")
                    } else {
                        text
                    });
            }
        }
    }
}
fn numeric_representation(value: &mut Value) {
    match value {
        Value::Number(number) => *value = json!(number.as_f64().unwrap()),
        Value::Array(values) => values.iter_mut().for_each(numeric_representation),
        Value::Object(values) => values.values_mut().for_each(numeric_representation),
        _ => {}
    }
}

#[test]
fn matches_typescript_resolution_and_error_contracts() {
    let document: Value = serde_json::from_str(include_str!("fixtures/reference.json")).unwrap();
    let mut failures = vec![];
    for (index, case) in document["fixtures"].as_array().unwrap().iter().enumerate() {
        let command = schema(case["schema"].as_str().unwrap());
        let argv: Vec<_> = case["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        let options = ResolveOptions {
            env: case["options"]["env"]
                .as_object()
                .map(|env| {
                    env.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            config: case["options"]["config"].clone(),
            stdin: case["options"]["stdinData"].as_str().map(str::to_owned),
            ..Default::default()
        };
        let result = parse(command.schema(), &argv, Default::default())
            .and_then(|parsed| resolve(command.schema(), &parsed, &options));
        let mut actual = match result {
            Ok(result) => {
                let mut values = result.json();
                normalize(&mut values, command.schema());
                values["sources"] = serde_json::to_value(&result.sources).unwrap();
                json!({"expected":values})
            }
            Err(error) => json!({"error":{"code":error.code,"exitCode":error.exit_code}}),
        };
        let mut expected = if case.get("error").is_some() {
            json!({"error":case["error"]})
        } else {
            json!({"expected":case["expected"]})
        };
        numeric_representation(&mut actual);
        numeric_representation(&mut expected);
        if actual != expected {
            failures.push(format!(
                "case {index}: {} {:?}\nexpected {expected}\nactual   {actual}",
                case["schema"], argv
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
