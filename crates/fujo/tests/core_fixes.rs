use fujo::CliError;
use fujo::ErrorKind;
use fujo::Result;
use fujo::RunOptions;
use fujo::RunResult;
use fujo::arg;
use fujo::cli;
use fujo::command;
use fujo::config::ConfigReader;
use fujo::config::ConfigSettings;
use fujo::config::discover;
use fujo::flag;
use fujo::json;
use fujo::parse::ParseOptions;
use fujo::parse::canonical_flag_name_with;
use fujo::parse::flag_lookup;
use fujo::resolve::FileSystem;
use fujo::resolve::was_explicit;
use fujo::schema::DuplicatePolicy;
use fujo::schema::Kind;
use fujo::schema::PathChecks;
use fujo::schema::PathType;
use fujo::schema::Split;
use fujo::schema::Surface;
use fujo::value;
use fujo_core::command::with_flags;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

fn run(command: fujo::Command, argv: &[&str], options: RunOptions) -> RunResult {
    cli("app")
        .default(command.action(|_| Ok(())))
        .execute(argv.iter().copied(), options)
}
fn with_env(name: &str, value: &str) -> RunOptions {
    let mut options = RunOptions::default();
    options.resolve.env.insert(name.into(), value.into());
    options
}

#[test]
fn negatable_camel_case_flags_register_a_kebab_negation() {
    let dry_run = flag::boolean("dryRun").negatable();
    assert_eq!(dry_run.schema().negated_name.as_deref(), Some("no-dry-run"));
    for argv in [["--no-dry-run"], ["--noDryRun"]] {
        let resolved = run(
            command("app").flag(dry_run.clone()),
            &argv,
            RunOptions::default(),
        )
        .resolved
        .unwrap();
        assert!(!resolved.get(&dry_run).unwrap());
        assert!(was_explicit(resolved.sources.flags.get("dryRun")));
    }
    let resolved = run(
        command("app").flag(dry_run.clone()),
        &["--dry-run"],
        RunOptions::default(),
    )
    .resolved
    .unwrap();
    assert!(resolved.get(&dry_run).unwrap());
}

#[test]
fn short_negated_flags_set_false() {
    let color = flag::boolean("color").alias("c").negated_name("N");
    for (argv, expected) in [("-N", false), ("-cN", false), ("-Nc", true)] {
        let resolved = run(
            command("app").flag(color.clone()),
            &[argv],
            RunOptions::default(),
        )
        .resolved
        .unwrap();
        assert_eq!(resolved.get(&color).unwrap(), expected, "{argv}");
        assert!(was_explicit(resolved.sources.flags.get("color")));
    }
}

#[test]
fn parsed_booleans_accept_their_negation() {
    let cache = flag::parse::<bool>("cache").negatable();
    for (argv, expected) in [("--no-cache", Some(false)), ("--cache=true", Some(true))] {
        let result = run(
            command("app").flag(cache.clone()),
            &[argv],
            RunOptions::default(),
        );
        assert!(result.success(), "{argv}: {:?}", result.error);
        assert_eq!(
            result.resolved.unwrap().get(&cache).unwrap(),
            expected,
            "{argv}"
        );
    }
}

#[test]
fn collection_element_errors_name_the_collection_once() {
    let ports = flag::array("ports", value::number().min(1.0)).env("PORTS");
    let error = run(
        command("app").flag(ports.clone()),
        &["--ports=0"],
        RunOptions::default(),
    )
    .error
    .unwrap();
    assert_eq!(error.code, "INVALID_VALUE");
    assert_eq!(
        error.message,
        "Invalid value for --ports: Value is below the minimum"
    );
    let error = run(command("app").flag(ports), &[], with_env("PORTS", "0"))
        .error
        .unwrap();
    assert_eq!(error.code, "CONSTRAINT_VIOLATED");
    assert_eq!(
        error.message,
        "Invalid value for --ports: Value is below the minimum"
    );
    let limits = flag::key_value_of("limits", value::number().min(1.0)).config("limits");
    let mut options = RunOptions::default();
    options.resolve.config = json!({"limits": {"cpu": 0}});
    let error = run(command("app").flag(limits), &[], options)
        .error
        .unwrap();
    assert_eq!(
        error.message,
        "Invalid value for --limits: Value is below the minimum"
    );
    let error = run(
        command("app").arg(arg::number("value").variadic()),
        &["1", "x"],
        RunOptions::default(),
    )
    .error
    .unwrap();
    assert_eq!(
        error.message,
        "Invalid value for <value>: Expected a finite number"
    );
}

#[test]
fn duplicate_entry_keys_report_the_source_kind() {
    let env = flag::key_value("env")
        .duplicate_keys(DuplicatePolicy::Error)
        .env("APP_ENV");
    let error = run(
        command("app").flag(env.clone()),
        &["--env=a=1", "--env=a=2"],
        RunOptions::default(),
    )
    .error
    .unwrap();
    assert_eq!(error.name, ErrorKind::Parse);
    assert_eq!(error.code, "INVALID_VALUE");
    assert_eq!(
        error.message,
        "Invalid value for --env: Duplicate entry key"
    );
    let error = run(
        command("app").flag(env),
        &[],
        with_env("APP_ENV", "a=1,a=2"),
    )
    .error
    .unwrap();
    assert_eq!(error.name, ErrorKind::Validation);
    assert_eq!(error.code, "CONSTRAINT_VIOLATED");
    assert_eq!(
        error.message,
        "Invalid value for --env: Duplicate entry key"
    );
}

#[test]
fn flag_spelling_collisions_and_invalid_spellings_are_schema_errors() {
    let error = command("app")
        .flag(flag::boolean("a").alias("a"))
        .validate()
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SCHEMA");
    assert_eq!(error.message, "Colliding flag spelling 'a'");
    let error = command("app")
        .flag(flag::boolean("a").alias("-x"))
        .validate()
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SCHEMA");
    assert!(error.message.contains("'-x'"), "{}", error.message);
    let error = command("app")
        .flag(flag::boolean("a").negated_name("no a"))
        .validate()
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SCHEMA");
    assert!(error.message.contains("'no a'"), "{}", error.message);
}

struct Reader(BTreeMap<PathBuf, String>);
impl ConfigReader for Reader {
    fn read(&self, path: &Path) -> Result<Option<String>> {
        Ok(self.0.get(path).cloned())
    }
}
#[test]
fn config_errors_keep_loader_text_and_name_the_file() {
    let settings = ConfigSettings::new("app").loader(["txt"], |_: &str| {
        Err(CliError::new("BAD_PORT", "Invalid port"))
    });
    let reader = Reader(BTreeMap::from([
        (PathBuf::from("a.txt"), "x".into()),
        (PathBuf::from("list.json"), "[]".into()),
    ]));
    let error = discover(&settings, &reader, &[], Some(Path::new("a.txt"))).unwrap_err();
    assert_eq!(error.code, "CONFIG_PARSE_ERROR");
    assert_eq!(
        error.message,
        "Cannot parse config file a.txt: Invalid port"
    );
    let error = discover(&settings, &reader, &[], Some(Path::new("list.json"))).unwrap_err();
    assert_eq!(error.code, "CONFIG_PARSE_ERROR");
    assert_eq!(error.message, "Config file list.json must be an object");
}

struct StatFailsAfterMkdir(Mutex<bool>);
impl FileSystem for StatFailsAfterMkdir {
    fn stat(&self, _: &Path) -> Result<Option<PathType>> {
        if *self.0.lock().unwrap() {
            Err(CliError::new("IO_ERROR", "stat failed after mkdir"))
        } else {
            Ok(None)
        }
    }
    fn mkdir(&self, _: &Path) -> Result<()> {
        *self.0.lock().unwrap() = true;
        Ok(())
    }
}
#[test]
fn stat_failures_after_creating_a_directory_are_input_failures() {
    let out = flag::path("out").path_checks(PathChecks::directory().create());
    let mut options = RunOptions::default();
    options.resolve.filesystem = Some(Arc::new(StatFailsAfterMkdir(Mutex::new(false))));
    let error = run(command("app").flag(out), &["--out=build"], options)
        .error
        .unwrap();
    assert_eq!(error.code, "CONSTRAINT_VIOLATED");
    assert_eq!(
        error.message,
        "Invalid value for --out: stat failed after mkdir"
    );
}

#[cfg(feature = "regex")]
#[test]
fn patterns_compile_at_build_time_and_reject_mismatches() {
    let error = command("app")
        .flag(flag::string("name").pattern("["))
        .validate()
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SCHEMA");
    assert_eq!(error.message, "Invalid regular expression");
    let name = flag::string("name").pattern("^[a-z]+$");
    let error = run(
        command("app").flag(name.clone()),
        &["--name=ABC"],
        RunOptions::default(),
    )
    .error
    .unwrap();
    assert_eq!(
        error.message,
        "Invalid value for --name: Value does not match the pattern"
    );
    assert!(
        run(
            command("app").flag(name),
            &["--name=abc"],
            RunOptions::default()
        )
        .success()
    );
}

#[test]
fn choices_are_checked_once_with_one_message() {
    let region = flag::enumeration("region", ["us", "eu"]).env("REGION");
    let error = run(
        command("app").flag(region.clone()),
        &["--region=ap"],
        RunOptions::default(),
    )
    .error
    .unwrap();
    assert_eq!(error.code, "INVALID_VALUE");
    assert_eq!(
        error.message,
        "Invalid value for --region: Expected one of us, eu"
    );
    let error = run(command("app").flag(region), &[], with_env("REGION", "ap"))
        .error
        .unwrap();
    assert_eq!(error.code, "INVALID_ENUM");
    assert_eq!(
        error.message,
        "Invalid value for --region: Expected one of us, eu"
    );
    let port = flag::parse::<u16>("port")
        .choices(["80", "443"])
        .config("port");
    assert!(
        command("app")
            .flag(port.clone().default(80))
            .validate()
            .is_ok()
    );
    assert_eq!(
        command("app")
            .flag(port.clone().default(8080))
            .validate()
            .unwrap_err()
            .code,
        "INVALID_SCHEMA"
    );
    let mut options = RunOptions::default();
    options.resolve.config = json!({"port": 80});
    let resolved = run(command("app").flag(port.clone()), &[], options)
        .resolved
        .unwrap();
    assert_eq!(resolved.get(&port).unwrap(), Some(80));
    let mut options = RunOptions::default();
    options.resolve.config = json!({"port": 8080});
    let error = run(command("app").flag(port), &[], options).error.unwrap();
    assert_eq!(error.code, "INVALID_ENUM");
    assert_eq!(
        error.message,
        "Invalid value for --port: Expected one of 80, 443"
    );
}

#[test]
fn default_json_mirrors_the_native_default() {
    let port = flag::number("port").default(8080.0);
    assert_eq!(port.schema().default_json(), Some(&json!(8080.0)));
    assert_eq!(port.clone().required().schema().default_json(), None);
    assert_eq!(port.clone().optional().schema().default_json(), None);
    assert_eq!(flag::number("port").schema().default_json(), None);
    let schema = fujo::json_schema::input_schema(command("app").flag(port).schema());
    assert_eq!(
        schema["properties"]["flags"]["properties"]["port"]["default"],
        json!(8080.0)
    );
    let variadic = arg::string("item").default("one".into()).variadic();
    assert_eq!(variadic.schema().default_json(), Some(&json!(["one"])));
}

#[test]
fn strict_string_stays_out_of_definitions() {
    let app = cli("app").default(
        command("app")
            .flag(flag::key_value("define"))
            .action(|_| Ok(())),
    );
    let definition = fujo::json_schema::definition(app.schema());
    let define = &definition["default"]["flags"][0];
    assert_eq!(define["name"], json!("define"));
    assert_eq!(define["element"]["kind"], json!("string"));
    assert!(define["element"].get("strictString").is_none());
    assert!(define.get("strictString").is_none());
}

#[test]
fn inherited_flags_precede_local_flags_without_shadowing() {
    let schema = with_flags(
        command("child")
            .flag(flag::boolean("verbose"))
            .flag(flag::string("name"))
            .schema()
            .clone(),
        &[
            flag::boolean("verbose").into_schema(),
            flag::string("region").into_schema(),
        ],
    );
    let names: Vec<_> = schema.flags.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["region", "verbose", "name"]);
    assert!(schema.validate().is_ok());
    let error = command("root")
        .flag(flag::string("region").alias("r").propagate())
        .command(command("child").flag(flag::string("other").alias("r")))
        .validate()
        .unwrap_err();
    assert_eq!(error.code, "INVALID_SCHEMA");
    assert_eq!(error.message, "Colliding flag spelling 'r'");
    assert!(
        command("root")
            .flag(flag::string("region").alias("r").propagate())
            .command(command("child").flag(flag::string("region").alias("r")))
            .validate()
            .is_ok()
    );
}

#[test]
fn canonical_names_resolve_against_a_shared_lookup() {
    let dry_run = flag::boolean("dryRun").into_schema();
    let flags = [dry_run.clone()];
    let lookup = flag_lookup(&flags, ParseOptions::default());
    assert_eq!(canonical_flag_name_with(&lookup, &dry_run), "dry-run");
    let lookup = flag_lookup(&flags, ParseOptions { case_parity: false });
    assert_eq!(canonical_flag_name_with(&lookup, &dry_run), "dryRun");
    let flags = [dry_run.clone(), flag::boolean("dry-run").into_schema()];
    let lookup = flag_lookup(&flags, ParseOptions::default());
    assert_eq!(canonical_flag_name_with(&lookup, &dry_run), "dryRun");
}

#[test]
fn positional_booleans_are_required_without_defaults() {
    let enabled = arg::boolean("enabled");
    let schema = enabled.schema();
    assert_eq!(schema.surface, Surface::Arg);
    assert_eq!(schema.kind, Kind::Boolean);
    assert!(schema.required);
    assert!(!schema.has_default());
    let resolved = run(
        command("app").arg(enabled.clone()),
        &["true"],
        RunOptions::default(),
    )
    .resolved
    .unwrap();
    assert!(resolved.get(&enabled).unwrap());
    let error = run(command("app").arg(enabled), &[], RunOptions::default())
        .error
        .unwrap();
    assert_eq!(error.code, "REQUIRED_ARG");
}

#[test]
fn byte_sizes_trim_whitespace_around_the_unit() {
    let size = flag::bytes("size");
    for (raw, expected) in [
        ("12 ", 12),
        ("1kb ", 1024),
        ("1 kb ", 1024),
        ("1 kb\n", 1024),
    ] {
        let argv = format!("--size={raw}");
        let result = run(
            command("app").flag(size.clone()),
            &[argv.as_str()],
            RunOptions::default(),
        );
        assert_eq!(
            result.resolved.unwrap().get(&size).unwrap(),
            Some(expected),
            "{raw:?}"
        );
    }
    for raw in ["1 k b", " 1kb"] {
        let argv = format!("--size={raw}");
        assert!(
            run(
                command("app").flag(size.clone()),
                &[argv.as_str()],
                RunOptions::default()
            )
            .error
            .is_some(),
            "{raw:?}"
        );
    }
}

#[test]
fn delimiters_split_on_one_character_and_drop_empty_parts() {
    assert_eq!(
        Split::Delimiter(';').values(";a;;b;").unwrap(),
        [json!("a"), json!("b")]
    );
    let tags = flag::array("tags", value::string()).separator(';');
    let result = run(
        command("app").flag(tags.clone()),
        &["--tags=x;y"],
        RunOptions::default(),
    );
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.resolved.unwrap().get(&tags).unwrap(), ["x", "y"]);
}
