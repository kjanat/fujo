use dreamcli::Cli;
use dreamcli::CliError;
use dreamcli::Command;
use dreamcli::Result;
use dreamcli::RunOptions;
use dreamcli::arg;
use dreamcli::cli;
use dreamcli::command;
use dreamcli::flag;
use dreamcli::group;
use dreamcli::json;
use dreamcli::output::OutputOptions;
use dreamcli::output::Verbosity;
use dreamcli::prompt::Answer;
use dreamcli::prompt::Choice;
use dreamcli::prompt::Prompt;
use dreamcli::prompt::TestPrompter;
use dreamcli::resolve::ResolveOptions;
use dreamcli::resolve::Stage;
use dreamcli::resolve::was_explicit;
use dreamcli::schema::DuplicatePolicy;
use dreamcli::schema::PathChecks;
use dreamcli::schema::PathType;
use dreamcli::schema::Split;
use dreamcli::schema::StdinBinding;
use dreamcli::schema::StdinConsume;
use dreamcli::schema::StdinWhen;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

fn empty() -> RunOptions {
    RunOptions::default()
}
fn ok_action() -> Command {
    command("ok").action(|_| Ok(()))
}

#[test]
fn typed_handles_infer_presence_and_values() {
    let name = arg::string("name");
    let force = flag::boolean("force");
    let region = flag::string("region");
    let cmd = Command::new("test")
        .arg(name.clone())
        .flag(force.clone())
        .flag(region.clone())
        .action(move |ctx| {
            let _: String = ctx.get(&name)?;
            let _: bool = ctx.get(&force)?;
            let _: Option<String> = ctx.get(&region)?;
            Ok(())
        });
    assert!(
        Cli::new("test")
            .default(cmd)
            .execute(["Ada"], empty())
            .success()
    );
}
#[derive(Debug, Clone, PartialEq)]
enum Region {
    Us,
    Eu,
}
impl std::str::FromStr for Region {
    type Err = &'static str;
    fn from_str(raw: &str) -> std::result::Result<Self, Self::Err> {
        match raw {
            "us" => Ok(Self::Us),
            "eu" => Ok(Self::Eu),
            _ => Err("Choose us or eu"),
        }
    }
}
#[test]
fn custom_values_need_neither_serde_nor_string_casts() {
    let region = flag::parse::<Region>("region")
        .choices(["us", "eu"])
        .default(Region::Us);
    let region_ = region.clone();
    let app = cli("deploy").default(command("deploy").flag(region).action(move |ctx| {
        assert_eq!(ctx.get(&region_)?, Region::Eu);
        Ok(())
    }));
    assert!(app.execute(["--region=eu"], empty()).success());
}
#[test]
fn opaque_non_clone_values_can_be_borrowed() {
    struct Connection {
        port: u16,
    }
    let connection = flag::custom("connection", |v| {
        Ok(Connection {
            port: v.as_str().unwrap().parse().unwrap(),
        })
    })
    .required();
    let key = connection.clone();
    let app = cli("app").default(command("app").flag(connection).action(move |ctx| {
        assert_eq!(ctx.get_ref(&key)?.port, 8080);
        Ok(())
    }));
    assert!(app.execute(["--connection=8080"], empty()).success());
}
#[test]
fn native_types_reach_the_handler() {
    let timeout = flag::duration("timeout");
    let bytes = flag::bytes("size");
    let path = flag::path("path");
    let date = flag::date("date");
    let url = flag::url("url");
    let app = cli("app").default(
        command("app")
            .flag(timeout.clone())
            .flag(bytes.clone())
            .flag(path.clone())
            .flag(date.clone())
            .flag(url.clone())
            .action(move |ctx| {
                let duration: Duration = ctx.get(&timeout)?.unwrap();
                assert_eq!(duration, Duration::from_secs(90));
                let size: u64 = ctx.get(&bytes)?.unwrap();
                assert_eq!(size, 1024);
                let path: PathBuf = ctx.get(&path)?.unwrap();
                assert_eq!(path, PathBuf::from("somewhere"));
                assert_eq!(ctx.get(&date)?.unwrap().timestamp(), 0);
                assert_eq!(ctx.get(&url)?.unwrap().scheme(), "https");
                Ok(())
            }),
    );
    assert!(
        app.execute(
            [
                "--timeout=1m30s",
                "--size=1kb",
                "--path=somewhere",
                "--date=1970-01-01",
                "--url=https://example.com"
            ],
            empty()
        )
        .success()
    );
}
#[test]
fn prompt_precedes_default_but_never_explicit_values() {
    let region = flag::enumeration("region", ["us", "eu"])
        .prompt(Prompt::select("Region?", []))
        .default("us".into());
    let app = cli("app").default(command("app").flag(region.clone()).action(|_| Ok(())));
    let prompter = Arc::new(TestPrompter::new([Answer::from("eu")]));
    let options = RunOptions {
        resolve: ResolveOptions {
            prompter: Some(prompter.clone()),
            ..Default::default()
        },
        ..empty()
    };
    let result = app.execute(Vec::<String>::new(), options.clone());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.resolved.unwrap().get(&region).unwrap(), "eu");
    assert_eq!(prompter.prompts().len(), 1);
    let result = app.execute(["--region=us"], options);
    assert!(result.success());
    assert_eq!(prompter.prompts().len(), 1);
}
#[test]
fn cancellation_has_exit_code_130() {
    let app = cli("app").default(
        command("app")
            .flag(flag::string("name").prompt(Prompt::input("Name?")))
            .action(|_| Ok(())),
    );
    let options = RunOptions {
        resolve: ResolveOptions {
            prompter: Some(Arc::new(TestPrompter::new([Answer::Cancel]))),
            ..Default::default()
        },
        ..empty()
    };
    assert_eq!(app.execute(Vec::<String>::new(), options).exit_code, 130);
}
#[test]
fn incompatible_prompts_fail_before_consuming_an_answer() {
    let prompter = Arc::new(TestPrompter::new([Answer::from("wrong")]));
    let app = cli("app").default(
        command("app")
            .flag(flag::boolean("yes").prompt(Prompt::input("Bad")))
            .action(|_| Ok(())),
    );
    let result = app.execute(
        Vec::<String>::new(),
        RunOptions {
            resolve: ResolveOptions {
                prompter: Some(prompter.clone()),
                ..Default::default()
            },
            ..empty()
        },
    );
    assert_eq!(result.error.unwrap().code, "CONSTRAINT_VIOLATED");
    assert!(prompter.prompts().is_empty());
}
#[test]
fn stdin_modes_and_provenance_preserve_the_source() {
    let text = arg::string("text").stdin_with(StdinBinding {
        when: StdinWhen::Missing,
        ..Default::default()
    });
    let app = cli("app").default(command("app").arg(text.clone()).action(|_| Ok(())));
    let options = RunOptions {
        resolve: ResolveOptions {
            stdin: Some("pipe\n".into()),
            ..Default::default()
        },
        ..empty()
    };
    let result = app.execute(["-"], options.clone());
    assert_eq!(result.resolved.unwrap().get(&text).unwrap(), "-");
    let resolved = app.execute(Vec::<String>::new(), options).resolved.unwrap();
    assert_eq!(resolved.get(&text).unwrap(), "pipe\n");
    assert_eq!(resolved.sources.args["text"].stage, Stage::Stdin);
    assert!(was_explicit(resolved.sources.args.get("text")));
}
#[test]
fn exclusive_stdin_consumers_are_rejected() {
    let app = cli("app").default(
        command("app")
            .flag(flag::string("text").stdin())
            .arg(arg::string("file").stdin()),
    );
    assert_eq!(app.validate().unwrap_err().code, "DUPLICATE_STDIN_INPUT");
}
#[test]
fn broadcast_stdin_is_shared() {
    let binding = StdinBinding {
        consume: StdinConsume::Broadcast,
        ..Default::default()
    };
    let a = flag::string("a").stdin_with(binding);
    let b = arg::string("b").stdin_with(binding);
    let app = cli("app").default(
        command("app")
            .flag(a.clone())
            .arg(b.clone())
            .action(|_| Ok(())),
    );
    let result = app.execute(
        Vec::<String>::new(),
        RunOptions {
            resolve: ResolveOptions {
                stdin: Some("shared".into()),
                ..Default::default()
            },
            ..empty()
        },
    );
    let values = result.resolved.unwrap();
    assert_eq!(values.get(&a).unwrap().unwrap(), "shared");
    assert_eq!(values.get(&b).unwrap(), "shared");
}
#[test]
fn dispatch_skips_flag_operands_and_inherits_flags_and_middleware() {
    let label = flag::string("label").propagate();
    let alias = label.clone();
    let app = cli("app").command(
        group("db")
            .flag(label)
            .middleware(|ctx, next| {
                ctx.out.log("parent");
                next.run(ctx)
            })
            .command(command("seed").alias("s").action(move |ctx| {
                ctx.out.log(ctx.get(&alias)?.unwrap());
                ctx.out.log(&ctx.meta.command);
                Ok(())
            })),
    );
    let result = app.execute(["db", "--label", "seed", "s"], empty());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "parent\nseed\ndb seed\n");
}
#[test]
fn root_flags_work_at_every_depth() {
    let region = flag::string("region").default("us".into());
    let key = region.clone();
    let app = cli("app")
        .flag(region)
        .command(group("db").command(command("seed").action(move |ctx| {
            ctx.out.log(ctx.get(&key)?);
            Ok(())
        })));
    assert_eq!(
        app.execute(["--region", "seed", "db", "seed"], empty())
            .stdout,
        "seed\n"
    );
}
#[test]
fn default_command_and_named_commands_coexist() {
    let target = arg::string("target");
    let key = target.clone();
    let app = cli("app")
        .default(command("default").arg(target).action(move |ctx| {
            ctx.out.log(ctx.get(&key)?);
            Ok(())
        }))
        .command(command("status").action(|ctx| {
            ctx.out.log("status");
            Ok(())
        }));
    assert_eq!(app.execute(["other"], empty()).stdout, "other\n");
    assert_eq!(app.execute(["status"], empty()).stdout, "status\n");
}
#[test]
fn no_arg_default_does_not_swallow_unknown_commands() {
    let result = cli("app").default(ok_action()).execute(["typo"], empty());
    assert_eq!(result.error.unwrap().code, "UNKNOWN_COMMAND");
}
#[test]
fn middleware_wraps_action_and_derive_supplies_native_state() {
    struct Credentials(String);
    let app = cli("app").default(
        command("app")
            .middleware(|ctx, next| {
                ctx.out.log("before");
                next.run(ctx)?;
                ctx.out.log("after");
                Ok(())
            })
            .derive(|_| Ok(Credentials("token".into())))
            .action(|ctx| {
                ctx.out.log(&ctx.state::<Credentials>()?.0);
                Ok(())
            }),
    );
    assert_eq!(
        app.execute(Vec::<String>::new(), empty()).stdout,
        "before\ntoken\nafter\n"
    );
}
#[test]
fn middleware_can_short_circuit() {
    let app = cli("app").default(
        command("app")
            .middleware(|ctx, _| {
                ctx.out.log("stopped");
                Ok(())
            })
            .action(|_| panic!("action must not run")),
    );
    assert_eq!(
        app.execute(Vec::<String>::new(), empty()).stdout,
        "stopped\n"
    );
}
#[test]
fn failure_is_structured_and_json_keeps_stdout_machine_readable() {
    let app = cli("app").default(command("app").action(|ctx| {
        ctx.out.log("working");
        Err(CliError::new("FAILED", "Failed")
            .suggest("Try again")
            .details(json!({"target":"prod"})))
    }));
    let result = app.execute(["--json"], empty());
    assert_eq!(result.stderr, "working\n");
    assert_eq!(
        serde_json::from_str::<dreamcli::Value>(&result.stdout).unwrap()["code"],
        "FAILED"
    );
    assert_eq!(result.exit_code, 1);
}
#[test]
fn quiet_and_json_boolean_modes_use_last_occurrence() {
    let app = cli("app").default(command("app").action(|ctx| {
        ctx.out.log("log");
        ctx.out.info("info");
        ctx.out.warn("warn");
        Ok(())
    }));
    let result = app.execute(["--json", "--json=false", "--quiet"], empty());
    assert_eq!(result.stdout, "log\n");
    assert_eq!(result.stderr, "warn\n");
}
#[test]
fn separator_preserves_literal_builtin_names() {
    let rest = arg::string("rest").variadic();
    let key = rest.clone();
    let app = cli("app")
        .version("1")
        .default(command("app").arg(rest).action(move |ctx| {
            ctx.out.log(ctx.get(&key)?.join(","));
            Ok(())
        }));
    let result = app.execute(["--", "--help", "--json", "--version"], empty());
    assert_eq!(result.stdout, "--help,--json,--version\n");
}
#[test]
fn help_and_version_precede_invalid_value_validation() {
    let app = cli("app").version("1.2.3").default(
        command("app")
            .arg(arg::string("name"))
            .flag(flag::number("port"))
            .action(|_| panic!("help must not execute")),
    );
    let result = app.execute(["--help", "--port=bad", "--json=invalid"], empty());
    assert!(result.success());
    assert!(result.stdout.contains("Usage:"));
    assert_eq!(
        app.execute(["--version", "--port=bad"], empty()).stdout,
        "1.2.3\n"
    );
}
#[test]
fn help_uses_canonical_kebab_spelling_and_hides_sensitive_defaults() {
    let app = cli("app").default(
        command("app")
            .flag(flag::string("outputPath"))
            .flag(flag::string("secret").sensitive().default("PRIVATE".into()))
            .flag(flag::boolean("hidden").hidden())
            .action(|_| Ok(())),
    );
    let text = app.execute(["--help"], empty()).stdout;
    assert!(text.contains("--output-path"));
    assert!(!text.contains("PRIVATE"));
    assert!(!text.contains("--hidden"));
    assert!(
        !dreamcli::json_schema::definition(app.schema())
            .to_string()
            .contains("PRIVATE")
    );
    assert!(
        !serde_json::to_string(app.schema())
            .unwrap()
            .contains("PRIVATE")
    );
}
#[test]
fn sensitive_invalid_input_is_absent_from_every_diagnostic() {
    let app = cli("app").default(
        command("app")
            .flag(flag::parse::<u16>("token").sensitive())
            .action(|_| Ok(())),
    );
    let result = app.execute(["--json", "--token=SECRET_VALUE"], empty());
    assert!(!result.stdout.contains("SECRET_VALUE"));
    assert!(!result.stderr.contains("SECRET_VALUE"));
    assert!(!format!("{:?}", result.error).contains("SECRET_VALUE"));
}
#[test]
fn all_missing_inputs_are_reported_together() {
    let app = cli("app").default(
        command("app")
            .flag(flag::string("token").required())
            .arg(arg::string("target"))
            .action(|_| Ok(())),
    );
    let result = app.execute(Vec::<String>::new(), empty());
    assert_eq!(result.error.unwrap().details.unwrap()["count"], 2);
}
#[test]
fn duplicate_policies_and_constraints_apply() {
    for (policy, expected) in [
        (DuplicatePolicy::First, "one"),
        (DuplicatePolicy::Last, "two"),
    ] {
        let name = flag::string("name").duplicate(policy);
        let app = cli("app").default(command("app").flag(name.clone()).action(|_| Ok(())));
        assert_eq!(
            app.execute(["--name=one", "--name=two"], empty())
                .resolved
                .unwrap()
                .get(&name)
                .unwrap()
                .unwrap(),
            expected
        );
    }
    let app = cli("app").default(
        command("app")
            .flag(flag::string("name").duplicate(DuplicatePolicy::Error))
            .action(|_| Ok(())),
    );
    assert_eq!(
        app.execute(["--name=one", "--name=two"], empty())
            .error
            .unwrap()
            .code,
        "DUPLICATE_FLAG"
    );
    assert!(
        cli("app")
            .default(
                command("app")
                    .flag(flag::number("port").integer().min(1.0).max(65535.0))
                    .action(|_| Ok(()))
            )
            .execute(["--port=0"], empty())
            .error
            .is_some()
    );
}
#[test]
fn arrays_have_native_empty_defaults_and_element_validation() {
    let ports = flag::array(
        "ports",
        flag::parse::<u16>("port").validate(|v| {
            if *v > 0 {
                Ok(())
            } else {
                Err(CliError::validation("CONSTRAINT_VIOLATED", "Port is zero"))
            }
        }),
    );
    let app = cli("app").default(command("app").flag(ports.clone()).action(|_| Ok(())));
    assert_eq!(
        app.execute(Vec::<String>::new(), empty())
            .resolved
            .unwrap()
            .get(&ports)
            .unwrap(),
        Vec::<u16>::new()
    );
    assert!(app.execute(["--ports=0"], empty()).error.is_some());
}
#[test]
fn json_and_whitespace_split_policies_work() {
    let tags = flag::array("tags", flag::string("tag"))
        .split_env(Split::Json)
        .env("TAGS");
    let app = cli("app").default(command("app").flag(tags.clone()).action(|_| Ok(())));
    let options = RunOptions {
        resolve: ResolveOptions {
            env: BTreeMap::from([("TAGS".into(), "[\"a\",\"b\"]".into())]),
            ..Default::default()
        },
        ..empty()
    };
    assert_eq!(
        app.execute(Vec::<String>::new(), options)
            .resolved
            .unwrap()
            .get(&tags)
            .unwrap(),
        vec!["a", "b"]
    );
}
#[test]
fn schema_errors_reject_alias_collisions_and_arg_order() {
    assert!(
        command("app")
            .flag(flag::string("name").alias("n"))
            .flag(flag::number("count").alias("n"))
            .validate()
            .is_err()
    );
    assert!(
        command("app")
            .arg(arg::string("optional").optional())
            .arg(arg::string("required"))
            .validate()
            .is_err()
    );
    assert!(
        command("app")
            .arg(arg::string("all").variadic())
            .arg(arg::string("extra"))
            .validate()
            .is_err()
    );
    assert!(
        command("app")
            .flag(flag::string("name").pattern("["))
            .validate()
            .is_err()
    );
    assert!(
        command("app")
            .flag(flag::enumeration("region", ["us"]).default("ap".into()))
            .validate()
            .is_err()
    );
}
#[test]
fn builtin_names_can_be_released_explicitly() {
    let app = cli("app").default(
        command("app")
            .flag(flag::boolean("json"))
            .action(|_| Ok(())),
    );
    assert_eq!(app.validate().unwrap_err().code, "RESERVED_FLAG");
    let app = app.builtins(dreamcli::Builtins {
        json: false,
        ..Default::default()
    });
    assert!(app.execute(["--json"], empty()).success());
}
#[test]
fn path_checks_can_create_directories_and_preserve_missing_output_files() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a/b");
    let path = flag::path("path").path_checks(PathChecks {
        must_exist: true,
        path_type: Some(PathType::Directory),
        create: true,
    });
    let app = cli("app").default(command("app").flag(path).action(|_| Ok(())));
    let options = RunOptions {
        resolve: ResolveOptions {
            filesystem: Some(Arc::new(dreamcli::runtime::NativeRuntime)),
            ..Default::default()
        },
        ..empty()
    };
    assert!(
        app.execute([format!("--path={}", nested.display())], options.clone())
            .success()
    );
    assert!(nested.is_dir());
    let app = cli("app").default(
        command("app")
            .flag(flag::path("output").path_checks(PathChecks {
                must_exist: false,
                path_type: Some(PathType::File),
                create: false,
            }))
            .action(|_| Ok(())),
    );
    assert!(
        app.execute(
            [format!("--output={}", dir.path().join("new.txt").display())],
            options
        )
        .success()
    );
}
#[test]
fn config_native_scalars_work_with_from_str_types() {
    let port = flag::parse::<u16>("port").config("server.port");
    let app = cli("app").default(command("app").flag(port.clone()).action(|_| Ok(())));
    let result = app.execute(
        Vec::<String>::new(),
        RunOptions {
            resolve: ResolveOptions {
                config: json!({"server":{"port":8080}}),
                ..Default::default()
            },
            ..empty()
        },
    );
    assert_eq!(result.resolved.unwrap().get(&port).unwrap(), Some(8080));
}
#[test]
fn exit_code_can_be_set_without_an_error() {
    let app = cli("app").default(command("app").action(|ctx| {
        ctx.out.set_exit_code(3);
        Ok(())
    }));
    let result = app.execute(Vec::<String>::new(), empty());
    assert_eq!(result.exit_code, 3);
    assert!(result.error.is_none());
}
#[test]
fn plugin_hooks_wrap_execution() {
    struct Trace(Arc<Mutex<Vec<&'static str>>>);
    impl dreamcli::Plugin for Trace {
        fn before_parse(&self, _: &mut Vec<String>) -> Result<()> {
            self.0.lock().unwrap().push("parse");
            Ok(())
        }
        fn before_execute(&self, _: &mut dreamcli::Context) -> Result<()> {
            self.0.lock().unwrap().push("before");
            Ok(())
        }
        fn after_execute(&self, _: &dreamcli::Context) -> Result<()> {
            self.0.lock().unwrap().push("after");
            Ok(())
        }
        fn on_error(&self, _: &CliError) {
            self.0.lock().unwrap().push("error");
        }
    }
    let trace = Arc::new(Mutex::new(vec![]));
    let app = cli("app").plugin(Trace(trace.clone())).default(ok_action());
    assert!(app.execute(Vec::<String>::new(), empty()).success());
    assert_eq!(*trace.lock().unwrap(), vec!["parse", "before", "after"]);
}
#[test]
fn output_modes_can_be_injected_without_flags() {
    let app = cli("app").default(command("app").action(|ctx| {
        ctx.out.info("info");
        ctx.out.json(&json!({"ok":true}))
    }));
    let result = app.execute(
        Vec::<String>::new(),
        RunOptions {
            output: OutputOptions {
                json: true,
                verbosity: Verbosity::Quiet,
                ..Default::default()
            },
            ..empty()
        },
    );
    assert_eq!(result.stdout, "{\"ok\":true}\n");
    assert!(result.stderr.is_empty());
}
#[test]
fn multiselect_prompts_return_native_collections() {
    let tags = flag::array("tags", flag::string("tag")).prompt(Prompt::multiselect(
        "Tags",
        [Choice::new("a", "A"), Choice::new("b", "B")],
    ));
    let app = cli("app").default(command("app").flag(tags.clone()).action(|_| Ok(())));
    let result = app.execute(
        Vec::<String>::new(),
        RunOptions {
            resolve: ResolveOptions {
                prompter: Some(Arc::new(TestPrompter::new([Answer::Value(json!([
                    "a", "b"
                ]))]))),
                ..Default::default()
            },
            ..empty()
        },
    );
    assert_eq!(result.resolved.unwrap().get(&tags).unwrap(), vec!["a", "b"]);
}

#[test]
fn conditional_prompts_receive_all_explicit_values_before_defaults() {
    let target = arg::string("target");
    let region = flag::string("region").default("us".into());
    let target_key = target.clone();
    let region_key = region.clone();
    let app = cli("app").default(
        command("app")
            .arg(target)
            .flag(region.clone())
            .interactive(move |inputs| {
                assert_eq!(inputs.get(&target_key)?, "production");
                assert!(inputs.peek(&region_key)?.is_none());
                Ok(BTreeMap::from([(
                    "region".into(),
                    Some(Prompt::input("Production region?")),
                )]))
            })
            .action(|_| Ok(())),
    );
    let mut options = empty();
    options.resolve.prompter = Some(Arc::new(TestPrompter::new([Answer::from("eu")])));
    let result = app.execute(["production"], options);
    assert_eq!(result.resolved.unwrap().get(&region).unwrap(), "eu");
}
#[test]
fn root_help_ignores_malformed_flags_and_unconfigured_json_is_an_error() {
    let app = cli("app").command(command("status").action(|_| Ok(())));
    assert!(
        app.execute(["--unknown", "--help"], empty())
            .stdout
            .contains("Commands:")
    );
    assert_eq!(
        app.execute(["--json=invalid"], empty()).error.unwrap().code,
        "INVALID_VALUE"
    );
}
#[test]
fn array_defaults_obey_element_constraints_and_native_validation() {
    let ports = flag::array("ports", flag::number("element").min(1.0)).default(vec![0.0]);
    assert!(command("app").flag(ports).validate().is_err());
}

#[test]
fn cancellation_stops_resolution_even_when_other_inputs_are_missing() {
    let app = cli("app").default(
        command("app")
            .flag(flag::string("name").prompt(Prompt::input("Name?")))
            .flag(flag::string("token").required())
            .action(|_| Ok(())),
    );
    let mut options = empty();
    options.resolve.prompter = Some(Arc::new(TestPrompter::new([Answer::Cancel])));
    assert_eq!(app.execute(Vec::<String>::new(), options).exit_code, 130);
}
#[test]
fn injected_writers_obey_the_invocations_output_modes() {
    let channel = dreamcli::Output::capture(Default::default());
    let app = cli("app").default(command("app").action(|ctx| {
        ctx.out.log("human");
        ctx.out.json(&json!({"ok":true}))
    }));
    let result = app.execute(
        ["--json"],
        RunOptions {
            channel: Some(channel),
            ..empty()
        },
    );
    assert_eq!(result.stdout, "{\"ok\":true}\n");
    assert_eq!(result.stderr, "human\n");
}
#[test]
fn successful_sensitive_values_stay_out_of_automatic_debug_views() {
    let token = flag::string("token").sensitive();
    let app = cli("app").default(command("app").flag(token.clone()).action(|_| Ok(())));
    let result = app.execute(["--token=SECRET_VALUE"], empty());
    assert!(!format!("{result:?}").contains("SECRET_VALUE"));
    let resolved = result.resolved.unwrap();
    assert!(!resolved.json().to_string().contains("SECRET_VALUE"));
    assert_eq!(resolved.get(&token).unwrap().unwrap(), "SECRET_VALUE");
}
