use dreamcli::CliError;
use dreamcli::Result;
use dreamcli::RunOptions;
use dreamcli::arg;
use dreamcli::cli;
use dreamcli::command;
use dreamcli::config::ConfigReader;
use dreamcli::config::ConfigSettings;
use dreamcli::config::discover;
use dreamcli::flag;
use dreamcli::json;
use dreamcli::output::Output;
use dreamcli::output::OutputOptions;
use dreamcli::resolve::FileSystem;
use dreamcli::resolve::Stage;
use dreamcli::runtime::NativeRuntime;
use dreamcli::runtime::Runtime;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[derive(Default)]
struct Reader(BTreeMap<PathBuf, String>);
impl ConfigReader for Reader {
    fn read(&self, path: &Path) -> Result<Option<String>> {
        Ok(self.0.get(path).cloned())
    }
}
struct Host {
    env: BTreeMap<String, String>,
    queried: Mutex<Vec<String>>,
    stdin: String,
    reads: AtomicUsize,
    reader: Reader,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            env: BTreeMap::new(),
            queried: Mutex::new(vec![]),
            stdin: "pipe\n".into(),
            reads: AtomicUsize::new(0),
            reader: Reader::default(),
        }
    }
}
impl Runtime for Host {
    fn env(&self, name: &str) -> Option<String> {
        self.queried.lock().unwrap().push(name.into());
        self.env.get(name).cloned()
    }
    fn stdin_is_tty(&self) -> bool {
        false
    }
    fn stdout_is_tty(&self) -> bool {
        false
    }
    fn read_stdin(&self) -> Result<String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.stdin.clone())
    }
    fn cwd(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/project"))
    }
    fn reader(&self) -> &dyn ConfigReader {
        &self.reader
    }
    fn filesystem(&self) -> Arc<dyn FileSystem> {
        Arc::new(NativeRuntime)
    }
    fn output(&self, options: OutputOptions) -> Output {
        Output::capture(options)
    }
}
#[test]
fn native_run_reads_only_declared_environment_values() {
    let name = flag::string("name").env("APP_NAME").required();
    let key = name.clone();
    let app = cli("app").default(command("app").flag(name).action(move |ctx| {
        ctx.out.log(ctx.get(&key)?);
        Ok(())
    }));
    let host = Host {
        env: BTreeMap::from([
            ("APP_NAME".into(), "Ada".into()),
            ("UNRELATED_SECRET".into(), "not-read".into()),
        ]),
        ..Default::default()
    };
    assert_eq!(
        app.run_with(Vec::<String>::new(), RunOptions::default(), &host)
            .stdout,
        "Ada\n"
    );
    assert_eq!(*host.queried.lock().unwrap(), vec!["APP_NAME"]);
}
#[test]
fn stdin_is_read_once_only_when_a_selected_binding_needs_it() {
    let text = flag::string("text").stdin();
    let app = cli("app").default(command("app").flag(text).action(|_| Ok(())));
    let host = Host::default();
    assert!(
        app.run_with(["--text=cli"], RunOptions::default(), &host)
            .success()
    );
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
    assert!(
        app.run_with(Vec::<String>::new(), RunOptions::default(), &host)
            .success()
    );
    assert_eq!(host.reads.load(Ordering::SeqCst), 1);
    assert!(
        app.run_with(["--help"], RunOptions::default(), &host)
            .success()
    );
    assert_eq!(host.reads.load(Ordering::SeqCst), 1);
}
#[test]
fn unrelated_commands_do_not_trigger_stdin_reads() {
    let app = cli("app")
        .command(
            command("pipe")
                .arg(arg::string("data").stdin())
                .action(|_| Ok(())),
        )
        .command(command("status").action(|_| Ok(())));
    let host = Host::default();
    assert!(
        app.run_with(["status"], RunOptions::default(), &host)
            .success()
    );
    assert_eq!(host.reads.load(Ordering::SeqCst), 0);
}
#[test]
fn explicit_config_and_discovery_feed_the_same_resolution_pipeline() {
    let port = flag::parse::<u16>("port").config("server.port").default(80);
    let key = port.clone();
    let app =
        cli("app")
            .config(ConfigSettings::new("app"))
            .default(command("app").flag(port).action(move |ctx| {
                ctx.out.log(ctx.get(&key)?);
                Ok(())
            }));
    let host = Host {
        reader: Reader(BTreeMap::from([
            (
                PathBuf::from("/project/.apprc.json"),
                json!({"server":{"port":8080}}).to_string(),
            ),
            (
                PathBuf::from("custom.json"),
                json!({"server":{"port":9000}}).to_string(),
            ),
        ])),
        ..Default::default()
    };
    let result = app.run_with(Vec::<String>::new(), RunOptions::default(), &host);
    assert_eq!(result.stdout, "8080\n");
    assert_eq!(
        result.resolved.unwrap().sources.flags["port"].stage,
        Stage::Config
    );
    assert_eq!(
        app.run_with(["--config=custom.json"], RunOptions::default(), &host)
            .stdout,
        "9000\n"
    );
    assert_eq!(
        app.run_with(
            ["--config", "custom.json", "--port=7000"],
            RunOptions::default(),
            &host
        )
        .stdout,
        "7000\n"
    );
}
#[test]
fn config_errors_are_returned_without_exposing_file_contents() {
    let settings = ConfigSettings::new("app");
    let reader = Reader(BTreeMap::from([(
        PathBuf::from("bad.json"),
        "PRIVATE_STRING".into(),
    )]));
    let error = discover(&settings, &reader, &[], Some(Path::new("bad.json"))).unwrap_err();
    assert_eq!(error.code, "CONFIG_PARSE_ERROR");
    assert!(!error.to_string().contains("PRIVATE_STRING"));
    assert_eq!(
        discover(&settings, &reader, &[], Some(Path::new("missing.json")))
            .unwrap_err()
            .code,
        "CONFIG_NOT_FOUND"
    );
}
#[test]
fn custom_config_loaders_and_ordered_candidate_paths_are_supported() {
    let settings=ConfigSettings::new("app").loader(["txt"],|text| Ok(json!({"port":text.parse::<u16>().map_err(|_| CliError::new("INVALID_CONFIG","Invalid port"))?})));
    let reader = Reader(BTreeMap::from([(
        PathBuf::from("second.txt"),
        "8080".into(),
    )]));
    assert_eq!(
        discover(
            &settings,
            &reader,
            &[PathBuf::from("first.txt"), PathBuf::from("second.txt")],
            None
        )
        .unwrap()
        .unwrap()
        .value,
        json!({"port":8080})
    );
}
#[test]
fn config_search_is_project_then_user_then_system() {
    let settings = ConfigSettings::new("app");
    let paths = settings.candidates(
        Path::new("/project/sub"),
        Some(Path::new("/user/config")),
        &[PathBuf::from("/system/config")],
    );
    assert_eq!(paths[0], PathBuf::from("/project/sub/.apprc.json"));
    assert!(
        paths
            .iter()
            .position(|p| p.starts_with("/project"))
            .unwrap()
            < paths.iter().position(|p| p.starts_with("/user")).unwrap()
    );
    assert!(
        paths.iter().position(|p| p.starts_with("/user")).unwrap()
            < paths.iter().position(|p| p.starts_with("/system")).unwrap()
    );
}
#[test]
fn input_schema_exports_constraints_and_omits_sensitive_defaults() {
    let cmd = command("app")
        .flag(flag::number("port").integer().min(1.0).required())
        .flag(
            flag::string("secret")
                .sensitive()
                .default("PRIVATE_STRING".into()),
        );
    let schema = dreamcli::json_schema::input_schema(cmd.schema());
    assert_eq!(
        schema["properties"]["flags"]["properties"]["port"]["minimum"],
        1.0
    );
    assert_eq!(schema["properties"]["flags"]["required"], json!(["port"]));
    assert!(!schema.to_string().contains("PRIVATE_STRING"));
}
#[test]
fn native_parser_errors_do_not_get_swallowed_by_an_adapter() {
    let app = cli("app").default(
        command("app")
            .flag(flag::parse::<u16>("port"))
            .action(|_| Ok(())),
    );
    let result = app.run_with(["--port=bad"], RunOptions::default(), &Host::default());
    assert!(!result.success());
    assert_eq!(result.exit_code, 2);
}
#[test]
fn help_does_not_probe_config_or_environment_sources() {
    let app = cli("app").config(ConfigSettings::new("app")).default(
        command("app")
            .flag(flag::string("token").env("TOKEN").required())
            .action(|_| Ok(())),
    );
    let host = Host::default();
    assert!(
        app.run_with(
            ["--config=missing.json", "--help"],
            RunOptions::default(),
            &host
        )
        .success()
    );
    assert!(host.queried.lock().unwrap().is_empty());
}
#[test]
fn explicitly_injected_environment_takes_priority_over_runtime_environment() {
    let name = flag::string("name").env("NAME");
    let key = name.clone();
    let app = cli("app").default(command("app").flag(name).action(move |ctx| {
        ctx.out.log(ctx.get(&key)?.unwrap());
        Ok(())
    }));
    let host = Host {
        env: BTreeMap::from([("NAME".into(), "runtime".into())]),
        ..Default::default()
    };
    let mut options = RunOptions::default();
    options.resolve.env.insert("NAME".into(), "injected".into());
    assert_eq!(
        app.run_with(Vec::<String>::new(), options, &host).stdout,
        "injected\n"
    );
}
#[test]
fn config_arrays_are_not_accepted_as_objects() {
    let reader = Reader(BTreeMap::from([(PathBuf::from("array.json"), "[]".into())]));
    assert!(
        discover(
            &ConfigSettings::new("app"),
            &reader,
            &[],
            Some(Path::new("array.json"))
        )
        .is_err()
    );
}
#[test]
fn missing_discovery_is_successful_without_a_config() {
    assert!(
        discover(
            &ConfigSettings::new("app"),
            &Reader::default(),
            &[PathBuf::from("missing.json")],
            None
        )
        .unwrap()
        .is_none()
    );
}
#[test]
fn cargo_metadata_is_available_without_package_json_discovery() {
    let manifest = dreamcli::cargo_manifest!();
    assert_eq!(manifest.name.as_deref(), Some("dreamcli"));
}

#[test]
#[cfg(unix)]
fn invalid_os_arguments_are_structured_errors_instead_of_panics() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let app = cli("app")
        .default(command("app").action(|_| panic!("invalid invocation must not execute")));
    let result = app.run_os_with(
        [OsString::from("--json"), OsString::from_vec(vec![0xff])],
        RunOptions::default(),
        &Host::default(),
    );
    assert_eq!(result.exit_code, 2);
    assert_eq!(
        serde_json::from_str::<dreamcli::Value>(&result.stdout).unwrap()["code"],
        "INVALID_UTF8"
    );
}
