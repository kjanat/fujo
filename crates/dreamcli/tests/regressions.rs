use dreamcli::Builtins;
use dreamcli::Cli;
use dreamcli::CliError;
use dreamcli::Command;
use dreamcli::RunOptions;
use dreamcli::Value;
use dreamcli::arg;
use dreamcli::flag;
use dreamcli::json;
use dreamcli::prompt::Answer;
use dreamcli::prompt::Prompt;
use dreamcli::prompt::TestPrompter;
use dreamcli::resolve::CheckPolicy;
use dreamcli::resolve::FileSystem;
use dreamcli::resolve::SkipReason;
use dreamcli::resolve::Stage;
use dreamcli::schema::PathChecks;
use dreamcli::schema::PathType;
use dreamcli::schema::Split;
use dreamcli::value;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

fn run(command: Command, argv: &[&str], options: RunOptions) -> dreamcli::RunResult {
    Cli::new("app")
        .default(command.action(|_| Ok(())))
        .execute(argv.iter().copied(), options)
}
fn prompted(answers: impl IntoIterator<Item = Answer>) -> RunOptions {
    let mut options = RunOptions::default();
    options.resolve.prompter = Some(Arc::new(TestPrompter::new(answers)));
    options
}

#[test]
fn string_choices_are_constraints_and_invalid_defaults_are_rejected() {
    let command = Command::new("app").flag(flag::string("color").choices(["red"]));
    let result = run(command.clone(), &["--color=blue"], RunOptions::default());
    assert_eq!(result.error.unwrap().code, "INVALID_VALUE");
    assert!(run(command, &["--color=red"], RunOptions::default()).success());
    let result = run(
        Command::new("app").flag(
            flag::string("color")
                .choices(["red"])
                .default("blue".into()),
        ),
        &[],
        RunOptions::default(),
    );
    assert_eq!(result.error.unwrap().code, "INVALID_SCHEMA");
}

#[test]
fn reusable_parsers_keep_element_constraints_and_bind_on_the_collection() {
    let parser = value::string().non_empty();
    let tags = flag::array("tags", parser.clone()).env("TAGS").unique();
    let mut options = RunOptions::default();
    options.resolve.env.insert("TAGS".into(), "a,b,a".into());
    let result = run(Command::new("app").flag(tags.clone()), &[], options);
    assert!(result.success());
    assert_eq!(result.resolved.unwrap().get(&tags).unwrap(), ["a", "b"]);
    let name = parser.arg("name");
    assert_eq!(
        run(Command::new("app").arg(name), &[""], RunOptions::default()).exit_code,
        2
    );
}

#[test]
fn json_help_exports_root_default_and_dispatched_command_without_executing() {
    let app = Cli::new("app")
        .default(
            Command::new("run")
                .flag(flag::string("name"))
                .action(|_| panic!("help executed")),
        )
        .command(
            Command::new("deploy")
                .flag(flag::boolean("force"))
                .action(|_| panic!("help executed")),
        );
    for args in [vec!["--help", "--json"], vec!["help", "--json"]] {
        let result = app.execute(args, RunOptions::default());
        assert!(result.success());
        assert!(result.stderr.is_empty());
        let document: Value = serde_json::from_str(&result.stdout).unwrap();
        assert_eq!(document["format"], "dreamcli-rust");
        assert_eq!(document["default"]["flags"][0]["name"], "name");
    }
    for args in [
        vec!["deploy", "--help", "--json"],
        vec!["help", "deploy", "--json"],
    ] {
        let result = app.execute(args, RunOptions::default());
        assert!(result.success());
        let document: Value = serde_json::from_str(&result.stdout).unwrap();
        assert_eq!(document["name"], "deploy");
        assert_eq!(document["flags"][0]["name"], "force");
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn help_never_rewrites_a_spelling_rejected_by_case_parity_or_a_collision() {
    let app = Cli::new("app").default(
        Command::new("app")
            .flag(flag::string("wasmPath"))
            .action(|_| Ok(())),
    );
    let mut options = RunOptions::default();
    options.parse.case_parity = false;
    let help = app.execute(["--help"], options.clone());
    assert!(help.stdout.contains("--wasmPath"));
    assert!(!help.stdout.contains("--wasm-path"));
    assert!(app.execute(["--wasmPath=ok"], options.clone()).success());
    assert!(!app.execute(["--wasm-path=ok"], options).success());
    let app = Cli::new("app").default(
        Command::new("app")
            .flag(flag::string("wasmPath"))
            .flag(flag::string("wasm-path"))
            .action(|_| Ok(())),
    );
    let help = app.execute(["--help"], RunOptions::default());
    assert!(help.stdout.contains("--wasmPath"));
    assert!(help.stdout.contains("--wasm-path"));
}

#[test]
fn blank_prompt_answers_fall_through_and_prompt_defaults_have_precedence() {
    let name = flag::string("name")
        .prompt(Prompt::input("Name?"))
        .default("Anonymous".into());
    for answer in ["", "  "] {
        let result = run(
            Command::new("app").flag(name.clone()),
            &[],
            prompted([Answer::from(answer)]),
        );
        assert!(result.success());
        let resolved = result.resolved.unwrap();
        assert_eq!(resolved.get(&name).unwrap(), "Anonymous");
        assert_eq!(resolved.sources.flags["name"].stage, Stage::Default);
    }
    let name = flag::string("name")
        .prompt(Prompt::input("Name?").default("Prompt default"))
        .default("Anonymous".into());
    let result = run(
        Command::new("app").flag(name.clone()),
        &[],
        prompted([Answer::from("")]),
    );
    let resolved = result.resolved.unwrap();
    assert_eq!(resolved.get(&name).unwrap(), "Prompt default");
    assert_eq!(resolved.sources.flags["name"].stage, Stage::Prompt);
    let result = run(
        Command::new("app").flag(
            flag::string("name")
                .required()
                .prompt(Prompt::input("Name?")),
        ),
        &[],
        prompted([Answer::from("")]),
    );
    assert_eq!(result.error.unwrap().code, "REQUIRED_FLAG");
}

#[test]
fn confirmation_defaults_and_prompt_validation_are_shared_with_testkit() {
    for default in [None, Some(false), Some(true)] {
        let prompt = default
            .map(|value| Prompt::confirm("Continue?").default(value))
            .unwrap_or_else(|| Prompt::confirm("Continue?"));
        let yes = flag::boolean("yes").prompt(prompt);
        let result = run(
            Command::new("app").flag(yes.clone()),
            &[],
            prompted([Answer::from("")]),
        );
        assert_eq!(
            result.resolved.unwrap().get(&yes).unwrap(),
            default.unwrap_or(true)
        );
    }
    let name = flag::string("name")
        .prompt(Prompt::input("Name?").validate(|value| {
            if value.len() >= 3 {
                Ok(())
            } else {
                Err(CliError::validation("BAD_NAME", "Name too short"))
            }
        }))
        .default("Anonymous".into());
    let result = run(
        Command::new("app").flag(name.clone()),
        &[],
        prompted([Answer::from("x")]),
    );
    assert_eq!(result.resolved.unwrap().get(&name).unwrap(), "Anonymous");
    let result = run(
        Command::new("app").flag(name.clone()),
        &[],
        prompted([Answer::from("x"), Answer::from("Ada")]),
    );
    assert_eq!(result.resolved.unwrap().get(&name).unwrap(), "Ada");
    let result = run(
        Command::new("app")
            .flag(flag::boolean("yes").prompt(Prompt::confirm("Continue?").default("wrong"))),
        &[],
        prompted([Answer::from("")]),
    );
    assert_eq!(result.error.unwrap().code, "INVALID_SCHEMA");
}

#[test]
fn filesystem_skips_are_visible_for_flags_args_and_creating_paths() {
    let command = Command::new("app")
        .flag(flag::path("input").path_checks(PathChecks::file()))
        .flag(flag::path("cache").path_checks(PathChecks::directory().create()))
        .arg(arg::path("output").path_checks(PathChecks::file().allow_missing()));
    let result = run(
        command.clone(),
        &["--input=in", "--cache=cache", "out"],
        RunOptions::default(),
    );
    assert!(result.success());
    assert_eq!(result.skipped_checks.len(), 3);
    assert!(
        result
            .skipped_checks
            .iter()
            .all(|check| check.reason == SkipReason::MissingFilesystem)
    );
    assert_eq!(result.skipped_checks[2].name, "output");
    let mut options = RunOptions::default();
    options.resolve.filesystem_policy = CheckPolicy::Required;
    let result = run(command, &["--input=in", "--cache=cache", "out"], options);
    assert_eq!(result.error.unwrap().code, "MISSING_CAPABILITY");
}

struct NoFilesystemAccess;
impl FileSystem for NoFilesystemAccess {
    fn stat(&self, _: &Path) -> dreamcli::Result<Option<PathType>> {
        panic!("disabled check accessed filesystem")
    }
    fn mkdir(&self, _: &Path) -> dreamcli::Result<()> {
        panic!("disabled check created a directory")
    }
}
#[test]
fn disabled_filesystem_checks_are_explicit_even_with_a_filesystem() {
    let mut options = RunOptions::default();
    options.resolve.filesystem = Some(Arc::new(NoFilesystemAccess));
    options.resolve.filesystem_policy = CheckPolicy::Disabled;
    let result = run(
        Command::new("app").flag(flag::path("file").path_checks(PathChecks::file())),
        &["--file=missing"],
        options,
    );
    assert!(result.success());
    assert_eq!(result.skipped_checks[0].reason, SkipReason::Disabled);
}

#[test]
fn native_primitives_export_values_defaults_and_bounded_json_schema() {
    let times = flag::parse::<u16>("times").default(1);
    let command = Command::new("app").flag(times.clone());
    let app = Cli::new("app").default(command.clone().action(|_| Ok(())));
    assert_eq!(app.definition()["default"]["flags"][0]["defaultValue"], 1);
    assert!(
        app.execute(["--help"], RunOptions::default())
            .stdout
            .contains("default: 1")
    );
    let result = app.execute(["--times=3"], RunOptions::default());
    assert_eq!(result.resolved.unwrap().json()["flags"]["times"], 3);
    let schema = dreamcli::json_schema::input_schema(command.schema());
    assert_eq!(
        schema["properties"]["flags"]["properties"]["times"]["maximum"],
        65535
    );
    let values =
        flag::array("values", value::parse::<u16>()).split_cli(Split::Delimiter(",".into()));
    let result = run(
        Command::new("app").flag(values),
        &["--values=001,2"],
        RunOptions::default(),
    );
    assert_eq!(
        result.resolved.unwrap().json()["flags"]["values"],
        json!([1, 2])
    );
}

#[derive(Clone, PartialEq, Debug)]
struct Port(u16);
#[test]
fn custom_encoders_support_non_serde_types_and_reencode_existing_defaults() {
    let port = flag::custom("port", |raw| {
        Ok(Port(raw.as_str().unwrap().parse().unwrap()))
    })
    .default(Port(80))
    .encode_json(|port| json!(port.0))
    .json_schema(json!({"type":"integer"}));
    let app = Cli::new("app").default(Command::new("app").flag(port.clone()).action(|_| Ok(())));
    assert_eq!(app.definition()["default"]["flags"][0]["defaultValue"], 80);
    let result = app.execute(["--port=8080"], RunOptions::default());
    let resolved = result.resolved.unwrap();
    assert_eq!(resolved.get(&port).unwrap(), Port(8080));
    assert_eq!(resolved.json()["flags"]["port"], 8080);
    let parser = value::custom(|raw| Ok(Port(raw.as_str().unwrap().parse().unwrap())))
        .encode_json(|port| json!(port.0));
    let result = run(
        Command::new("app").flag(flag::key_value_of("ports", parser)),
        &["--ports=http=80"],
        RunOptions::default(),
    );
    assert_eq!(
        result.resolved.unwrap().json()["flags"]["ports"],
        json!({"http":80})
    );
}

#[test]
fn default_annotations_do_not_change_machine_values_and_reset_on_new_default() {
    let retries = flag::parse::<u16>("retries").default(3).hide_default();
    let timeout = flag::number("timeout")
        .default(-1.0)
        .default_description("auto");
    let app = Cli::new("app").default(
        Command::new("app")
            .flag(retries)
            .flag(timeout)
            .action(|_| Ok(())),
    );
    let help = app.execute(["--help"], RunOptions::default()).stdout;
    assert!(help.contains("default: auto"));
    assert!(!help.contains("default: 3"));
    assert_eq!(app.definition()["default"]["flags"][0]["defaultValue"], 3);
    let count = flag::number("count")
        .default(1.0)
        .default_description("old")
        .default(2.0);
    let result = run(
        Command::new("app").flag(count),
        &["--help"],
        RunOptions::default(),
    );
    assert!(!result.stdout.contains("old"));
    assert!(result.stdout.contains("default: 2"));
}

#[test]
fn secret_prompt_defaults_do_not_leak_into_definition_documents() {
    let app = Cli::new("app").default(Command::new("app").flag(
        flag::string("token").prompt(Prompt::input("Token?").secret().default("PRIVATE_TOKEN")),
    ));
    assert!(!app.definition().to_string().contains("PRIVATE_TOKEN"));
}

#[test]
fn testkit_preserves_builtin_ownership_on_configured_clis_and_commands() {
    let builtins = Builtins {
        json: false,
        ..Default::default()
    };
    let command = Command::new("app")
        .flag(flag::boolean("json"))
        .action(|_| Ok(()));
    let app = Cli::new("app").builtins(builtins).default(command.clone());
    assert!(dreamcli::testkit::run_cli(&app, ["--json"], RunOptions::default()).success());
    assert!(
        dreamcli::testkit::run_command_with_builtins(
            command,
            ["--json"],
            RunOptions::default(),
            builtins
        )
        .success()
    );
}

#[test]
fn config_candidates_follow_the_shipped_project_user_and_system_conventions() {
    let settings = dreamcli::config::ConfigSettings::new("app");
    let paths = settings.candidates_in(
        Path::new("/project/sub"),
        &[PathBuf::from("/user")],
        &[PathBuf::from("/etc")],
    );
    assert_eq!(
        &paths[..6],
        [
            "/project/sub/.app.json",
            "/project/sub/app.config.json",
            "/project/sub/.config/app.json",
            "/project/.app.json",
            "/project/app.config.json",
            "/project/.config/app.json"
        ]
        .map(PathBuf::from)
    );
    assert_eq!(
        &paths[paths.len() - 2..],
        [
            PathBuf::from("/user/app/config.json"),
            PathBuf::from("/etc/app/config.json")
        ]
    );
    let paths = settings
        .base_dir("/anchor")
        .candidates_in(Path::new("/elsewhere"), &[], &[]);
    assert_eq!(paths[0], PathBuf::from("/anchor/.app.json"));
}

#[test]
fn platform_config_directories_use_named_reads_and_include_macos_and_windows() {
    use dreamcli::config::Platform;
    use dreamcli::config::config_directories;
    let mac = config_directories(Platform::MacOs, |name| match name {
        "HOME" => Some("/home/person".into()),
        "XDG_CONFIG_HOME" => Some("/xdg".into()),
        _ => panic!("unrelated env read"),
    });
    assert_eq!(
        mac.user,
        [
            PathBuf::from("/xdg"),
            PathBuf::from("/home/person/Library/Application Support")
        ]
    );
    assert_eq!(mac.system, [PathBuf::from("/etc")]);
    let windows = config_directories(Platform::Windows, |name| match name {
        "APPDATA" => Some("C:/Users/person/AppData/Roaming".into()),
        _ => panic!("unrelated env read"),
    });
    assert_eq!(
        windows.user,
        [PathBuf::from("C:/Users/person/AppData/Roaming")]
    );
    assert!(windows.system.is_empty());
}

#[test]
fn help_ordering_supports_declaration_order_and_custom_comparators() {
    use dreamcli::help::FlagOrder;
    use dreamcli::help::HelpOptions;
    let app = Cli::new("app").default(
        Command::new("app")
            .flag(flag::string("zebra"))
            .flag(flag::string("apple")),
    );
    let alphabetical = app.execute(["--help"], RunOptions::default()).stdout;
    assert!(alphabetical.find("--apple").unwrap() < alphabetical.find("--zebra").unwrap());
    let options = RunOptions {
        help: HelpOptions {
            flag_order: FlagOrder::Declaration,
            ..Default::default()
        },
        ..Default::default()
    };
    let declaration = app.execute(["--help"], options).stdout;
    assert!(declaration.find("--zebra").unwrap() < declaration.find("--apple").unwrap());
    let options = RunOptions {
        help: HelpOptions::default().sort_flags(|a, b| b.cmp(a)),
        ..Default::default()
    };
    let reversed = app.execute(["--help"], options).stdout;
    assert!(reversed.find("--zebra").unwrap() < reversed.find("--apple").unwrap());
}

#[test]
#[cfg(feature = "completion")]
fn completion_headers_record_the_program_version_in_all_shells() {
    use dreamcli::completion::Shell;
    use dreamcli::completion::generate;
    let app = Cli::new("app").version("1.2.3");
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        let script = generate(app.schema(), shell);
        assert!(script.contains("# Completion for app v1.2.3\n"));
        if shell == Shell::Zsh {
            assert!(script.starts_with("#compdef app\n"));
        }
    }
}

#[test]
#[cfg(feature = "completion")]
fn completion_spelling_respects_disabled_case_parity() {
    use dreamcli::completion::Shell;
    use dreamcli::completion::generate_with_options;
    let app = Cli::new("app").default(Command::new("app").flag(flag::string("wasmPath")));
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        let script = generate_with_options(
            app.schema(),
            shell,
            dreamcli::parse::ParseOptions { case_parity: false },
        );
        assert!(script.contains("wasmPath"));
        assert!(!script.contains("wasm-path"));
    }
}

struct StaticConfig;
impl dreamcli::config::ConfigReader for StaticConfig {
    fn read(&self, _: &Path) -> dreamcli::Result<Option<String>> {
        Ok(Some("8080".into()))
    }
}
#[test]
fn config_loaders_receive_the_discovered_path() {
    let settings =
        dreamcli::config::ConfigSettings::new("app").path_loader(["txt"], |path, content| {
            assert_eq!(path, Path::new("/project/app.txt"));
            Ok(json!({"port": content.parse::<u16>().unwrap()}))
        });
    let found = dreamcli::config::discover(
        &settings,
        &StaticConfig,
        &[PathBuf::from("/project/app.txt")],
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(found.value, json!({"port":8080}));
}

#[test]
fn invalid_json_schema_and_wrong_kind_choices_are_configuration_errors() {
    let result = run(
        Command::new("app").flag(flag::string("name").json_schema(json!(42))),
        &["--name=x"],
        RunOptions::default(),
    );
    assert_eq!(result.error.unwrap().code, "INVALID_SCHEMA");
    let result = run(
        Command::new("app").flag(flag::number("count").choices(["red"])),
        &["--count=1"],
        RunOptions::default(),
    );
    assert_eq!(result.error.unwrap().code, "INVALID_SCHEMA");
}

#[test]
fn default_validation_uses_native_constraints_before_json_encoding() {
    let port = flag::number("port")
        .min(1.0)
        .default(80.0)
        .encode_json(|port| json!({"number":port}))
        .json_schema(json!({"type":"object"}));
    let result = run(
        Command::new("app").flag(port.clone()),
        &[],
        RunOptions::default(),
    );
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(
        result.resolved.unwrap().json()["flags"]["port"],
        json!({"number":80.0})
    );
    let parser = value::number()
        .min(1.0)
        .encode_json(|port| json!({"number":port}));
    let ports = flag::array("ports", parser.clone()).default(vec![80.0]);
    let result = run(Command::new("app").flag(ports), &[], RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(
        result.resolved.unwrap().json()["flags"]["ports"],
        json!([{"number":80.0}])
    );
    let invalid = flag::array("ports", parser).default(vec![0.0]);
    let result = run(
        Command::new("app").flag(invalid),
        &[],
        RunOptions::default(),
    );
    assert_eq!(result.error.unwrap().code, "INVALID_SCHEMA");
}

#[test]
fn variadic_conversion_preserves_default_presence_and_explicit_precedence() {
    let files = arg::string("files")
        .default("fallback.txt".into())
        .variadic();
    let command = Command::new("app").arg(files.clone());
    let result = run(command.clone(), &[], RunOptions::default());
    assert!(result.success());
    let values: Vec<String> = result.resolved.unwrap().get(&files).unwrap();
    assert_eq!(values, ["fallback.txt"]);
    let result = run(command, &["a.txt", "b.txt"], RunOptions::default());
    assert_eq!(
        result.resolved.unwrap().get(&files).unwrap(),
        ["a.txt", "b.txt"]
    );
    let invalid = arg::number("ports").min(1.0).default(0.0).variadic();
    assert_eq!(
        run(Command::new("app").arg(invalid), &[], RunOptions::default())
            .error
            .unwrap()
            .code,
        "INVALID_SCHEMA"
    );
}

#[test]
fn opaque_path_defaults_obey_filesystem_policy_without_json_encoding() {
    let file = flag::parse::<PathBuf>("file")
        .path_checks(PathChecks::file())
        .default(PathBuf::from("missing"));
    let command = Command::new("app").flag(file);
    let result = run(command.clone(), &[], RunOptions::default());
    assert_eq!(result.skipped_checks.len(), 1);
    let mut options = RunOptions::default();
    options.resolve.filesystem_policy = CheckPolicy::Required;
    assert_eq!(
        run(command, &[], options).error.unwrap().code,
        "MISSING_CAPABILITY"
    );
}

#[test]
#[cfg(unix)]
fn native_path_defaults_support_non_unicode_filesystem_names() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(OsString::from_vec(vec![b'f', 0xff]));
    std::fs::write(&path, "fixture").unwrap();
    let file = flag::path("file")
        .path_checks(PathChecks::file())
        .default(path);
    let mut options = RunOptions::default();
    options.resolve.filesystem_policy = CheckPolicy::Required;
    options.resolve.filesystem = Some(Arc::new(dreamcli::runtime::NativeRuntime));
    let command = Command::new("app").flag(file.clone());
    assert!(run(command.clone(), &[], options.clone()).success());
    std::fs::remove_file(dir.path().join(OsString::from_vec(vec![b'f', 0xff]))).unwrap();
    assert_eq!(
        run(command, &[], options).error.unwrap().code,
        "CONSTRAINT_VIOLATED"
    );
}
