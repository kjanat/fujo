//! Nested settings inspection with source provenance, explicit stdin, and redacted credentials.
use dreamcli::Cli;
use dreamcli::CliError;
use dreamcli::Command;
use dreamcli::config::ConfigSettings;
use dreamcli::flag;
use dreamcli::group;
use dreamcli::json;
use dreamcli::output::Column;
use dreamcli::output::TableFormat;
use dreamcli::schema::StdinBinding;
use dreamcli::schema::StdinWhen;
use dreamcli::value;
use std::collections::BTreeMap;
use std::num::NonZeroU16;

pub fn app() -> Cli {
    let profile = flag::enumeration("profile", ["dev", "prod"])
        .env("SETTINGS_PROFILE")
        .config("profile")
        .default("dev".into());
    let settings = |name: &str| {
        let port = flag::parse::<NonZeroU16>("port")
            .env("SETTINGS_PORT")
            .config("server.port")
            .default(NonZeroU16::new(8080).expect("nonzero constant"))
            .encode_json(|v| json!(v.get()))
            .json_schema(json!({"type":"integer","minimum":1,"maximum":65535}));
        let endpoint = flag::url("endpoint")
            .protocols(["http", "https"])
            .env("SETTINGS_ENDPOINT")
            .config("server.endpoint")
            .default("https://localhost".parse().expect("constant URL"));
        let token = flag::string("token")
            .non_empty()
            .sensitive()
            .env("SETTINGS_TOKEN")
            .config("auth.token")
            .stdin_with(StdinBinding {
                when: StdinWhen::Dash,
                ..Default::default()
            });
        let features = flag::array("feature", value::enumeration(["cache", "metrics", "trace"]))
            .separator(",")
            .unique()
            .env("SETTINGS_FEATURES")
            .config("features");
        let limits = flag::key_value_of("limit", value::parse::<u16>())
            .config("limits")
            .default(BTreeMap::new());
        let profile = profile.clone();
        Command::new(name)
            .describe("Resolve, validate, and explain effective settings")
            .flag(port.clone())
            .flag(endpoint.clone())
            .flag(token.clone())
            .flag(features)
            .flag(limits)
            .derive(move |ctx| {
                let port: NonZeroU16 = ctx.get(&port)?;
                let endpoint = ctx.get(&endpoint)?;
                if ctx.get(&profile)? == "prod" && endpoint.scheme() != "https" {
                    return Err(CliError::validation(
                        "HTTPS_REQUIRED",
                        "Production endpoints must use HTTPS",
                    ));
                }
                Ok(Effective {
                    address: format!("{}:{}", endpoint.host_str().unwrap_or("localhost"), port),
                    authenticated: ctx.get_ref(&token)?.is_some(),
                })
            })
    };
    let show = settings("show").action(|ctx| {
        let effective = ctx.state::<Effective>()?;
        if ctx.out.options().json {
            ctx.out.json(
                &json!({"address":effective.address,"authenticated":effective.authenticated,
                "values":ctx.inputs.json()["flags"],"sources":ctx.sources()}),
            )
        } else {
            let flags = ctx.inputs.json();
            let rows = flags["flags"]
                .as_object()
                .expect("resolved flags")
                .iter()
                .map(|(key, value)| json!({"name":key,"value":value}))
                .collect::<Vec<_>>();
            ctx.out.table(
                &rows,
                &[
                    Column::new("name", "Setting"),
                    Column::new("value", "Value"),
                ],
                TableFormat::Pretty,
            )
        }
    });
    let validate = settings("validate").action(|ctx| {
        ctx.out
            .json(&json!({"valid":true,"address":ctx.state::<Effective>()?.address}))
    });
    let mut declaration = show.schema().clone();
    declaration.flags.push(profile.schema().clone());
    let schema = dreamcli::json_schema::input_schema(&declaration);
    Cli::new("settings")
        .version(env!("CARGO_PKG_VERSION"))
        .flag(profile)
        .config(ConfigSettings::new("settings"))
        .command(
            group("config")
                .describe("Inspect configuration without changing files")
                .command(show)
                .command(validate)
                .command(
                    Command::new("schema")
                        .describe("Export the settings input JSON Schema")
                        .action(move |ctx| ctx.out.json(&schema)),
                ),
        )
        .completions()
}

struct Effective {
    address: String,
    authenticated: bool,
}
fn main() -> std::process::ExitCode {
    app().run()
}
