use fujo::{Cli, Command, RunOptions, arg, flag};
use fujo_core::command::{CommandSchema, ProgramSchema};
use fujo_core::parse::{ParseOptions, parse};

#[test]
fn runtime_choices_reach_parsing_help_and_completions() {
    let choices = vec!["alpha".to_owned(), "beta".to_owned()];
    let inputs = (
        flag::string("profile").choices(choices.clone()),
        flag::enumeration("slice", &choices),
        flag::enumeration("iterator", choices.iter()),
        flag::array("array", fujo::value::string().choices(&choices)),
        arg::enumeration("positional", ["alpha", "beta"]).optional(),
    );
    let app =
        Cli::new("demo").default(Command::new("run").inputs(inputs).action(|_, values, ()| {
            assert_eq!(values.0.map(String::as_str), Some("alpha"));
            Ok(())
        }));
    assert!(
        app.execute(["--profile=alpha"], RunOptions::default())
            .success()
    );
    assert!(
        !app.execute(["--profile=gamma"], RunOptions::default())
            .success()
    );
    assert!(
        app.execute(["--help"], RunOptions::default())
            .stdout
            .contains("alpha")
    );
    #[cfg(feature = "completion")]
    for shell in [
        fujo::completion::Shell::Bash,
        fujo::completion::Shell::Zsh,
        fujo::completion::Shell::Fish,
        fujo::completion::Shell::PowerShell,
    ] {
        let script = fujo::completion::generate(app.schema(), shell);
        assert!(script.contains("alpha") && script.contains("beta"));
    }
    assert!(
        CommandSchema::new("demo")
            .flag(flag::enumeration("empty", Vec::<String>::new()).into_schema())
            .validate()
            .is_err()
    );
}

#[test]
fn short_aliases_are_case_sensitive() {
    let schema = CommandSchema::new("demo").flag(flag::string("format").alias("S").into_schema());
    assert!(parse(&schema, &["-Svalue".into()], ParseOptions::default()).is_ok());
    assert_eq!(
        parse(&schema, &["-svalue".into()], ParseOptions::default())
            .unwrap_err()
            .code,
        "UNKNOWN_FLAG"
    );
    #[cfg(feature = "completion")]
    for shell in [
        fujo::completion::Shell::Bash,
        fujo::completion::Shell::Zsh,
        fujo::completion::Shell::Fish,
        fujo::completion::Shell::PowerShell,
    ] {
        let script =
            fujo::completion::generate(&ProgramSchema::new("demo").default(schema.clone()), shell);
        assert!(!script.contains(":--S"));
        assert!(!script.contains(":-s"));
    }
}

#[test]
fn optional_values_attach_and_leave_the_next_word_positional() {
    let color = flag::enumeration("color", ["auto", "always", "never"])
        .alias("C")
        .optional_value("auto".into());
    let file = arg::string("file");
    let app = Cli::new("demo").default(
        Command::new("run")
            .inputs((color.clone(), file.clone()))
            .action(|_, _, ()| Ok(())),
    );
    for (argv, expected) in [
        (vec!["--color", "input"], "auto"),
        (vec!["--color=always", "input"], "always"),
        (vec!["-Calways", "input"], "always"),
        (vec!["-C", "input"], "auto"),
    ] {
        let result = app.execute(argv, RunOptions::default());
        assert!(result.success(), "{:?}", result.error);
        let resolved = result.resolved.unwrap();
        assert_eq!(resolved.get(&color).unwrap().as_deref(), Some(expected));
        assert_eq!(resolved.get(&file).unwrap(), "input");
    }
    assert!(
        app.execute(["--color=invalid", "input"], RunOptions::default())
            .error
            .is_some()
    );
}

#[test]
fn input_relationships_reject_explicit_selections_before_actions() {
    let files = arg::path("files").variadic().optional();
    let stdin = flag::boolean("stdin").alias("s").conflicts_with(&files);
    let in_place = flag::boolean("in-place").requires(&files);
    let spaces = flag::boolean("use-spaces");
    let tabs = flag::boolean("use-tabs").conflicts_with(&spaces);
    let app = Cli::new("demo").default(
        Command::new("run")
            .inputs((files, stdin, in_place, spaces, tabs))
            .action(|ctx, _, ()| {
                ctx.out.log("action");
                Ok(())
            }),
    );
    for (argv, code) in [
        (vec!["-s", "input.svg"], "CONFLICTING_INPUTS"),
        (vec!["--in-place"], "REQUIRED_INPUT"),
        (vec!["--use-tabs", "--use-spaces"], "CONFLICTING_INPUTS"),
    ] {
        let result = app.execute(argv, RunOptions::default());
        assert_eq!(result.error.unwrap().code, code);
        assert!(!result.stdout.contains("action"));
    }
    for argv in [
        vec![],
        vec!["--use-tabs"],
        vec!["input.svg"],
        vec!["--in-place", "input.svg"],
    ] {
        let result = app.execute(argv, RunOptions::default());
        assert!(result.success(), "{:?}", result.error);
    }
}

#[test]
fn completion_only_schemas_control_builtins_and_header_separately() {
    let schema = ProgramSchema::new("demo")
        .completion_version("1.2.3")
        .default(CommandSchema::new("run").flag(flag::boolean("help").into_schema()));
    schema.validate().unwrap();
    #[cfg(feature = "completion")]
    for shell in [
        fujo::completion::Shell::Bash,
        fujo::completion::Shell::Zsh,
        fujo::completion::Shell::Fish,
        fujo::completion::Shell::PowerShell,
    ] {
        let script = fujo::completion::generate(&schema, shell);
        assert!(script.contains("Completion for demo v1.2.3"));
        assert!(!script.contains("--version"));
        assert!(!script.contains("--quiet"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn native_path_inputs_read_the_exact_non_unicode_file() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(OsString::from_vec(b"input-\xff.svg".to_vec()));
    std::fs::write(&path, "exact file").unwrap();
    let app = Cli::new("demo").default(
        Command::new("read")
            .inputs((arg::path("file").optional(), flag::path("input").alias("i")))
            .action(|_, (file, input), ()| {
                assert_eq!(
                    std::fs::read_to_string(file.or(input).unwrap()).unwrap(),
                    "exact file"
                );
                Ok(())
            }),
    );
    let mut long = OsString::from("--input=");
    long.push(&path);
    let mut short = OsString::from("-i");
    short.push(&path);
    for argv in [
        vec![path.clone().into_os_string()],
        vec!["--input".into(), path.clone().into_os_string()],
        vec![long],
        vec![short],
    ] {
        let result = app.run_os_with(argv, RunOptions::default(), &fujo::runtime::NativeRuntime);
        assert!(result.success(), "{:?}", result.error);
    }
    let app = Cli::new("demo").default(
        Command::new("read")
            .inputs(arg::path("files").variadic())
            .action(|_, files, ()| {
                assert_eq!(files.len(), 2);
                for file in files {
                    assert_eq!(std::fs::read_to_string(file).unwrap(), "exact file");
                }
                Ok(())
            }),
    );
    assert!(
        app.run_os_with(
            [path.as_os_str(), path.as_os_str()],
            RunOptions::default(),
            &fujo::runtime::NativeRuntime
        )
        .success()
    );
}

#[test]
fn relationships_work_with_shared_dynamic_flags_and_source_precedence() {
    let files = flag::path("file");
    let stdin = flag::boolean("stdin")
        .alias("s")
        .env("USE_STDIN")
        .conflicts_with(&files);
    let app = Cli::new("demo")
        .flag(files.clone())
        .flag(stdin.clone())
        .default(
            Command::new("run")
                .flag(stdin)
                .flag(files)
                .action(|_| Ok(())),
        );
    assert!(
        app.execute(["--file=input.svg"], RunOptions::default())
            .success()
    );
    let mut options = RunOptions::default();
    options
        .resolve
        .env
        .insert("USE_STDIN".into(), "true".into());
    assert_eq!(
        app.execute(["--file=input.svg"], options)
            .error
            .unwrap()
            .code,
        "CONFLICTING_INPUTS"
    );
    assert_eq!(
        app.execute(["-s", "--file=input.svg"], RunOptions::default())
            .error
            .unwrap()
            .code,
        "CONFLICTING_INPUTS"
    );
}

#[test]
fn optional_values_preserve_command_selection_and_duplicate_policy() {
    use fujo::schema::DuplicatePolicy;
    let color = flag::enumeration("color", ["auto", "always"])
        .alias("C")
        .optional_value("auto".into())
        .default("always".into());
    let app = Cli::new("demo")
        .flag(color.clone())
        .command(Command::new("build").action(|_| Ok(())));
    for argv in [
        vec!["--color", "build"],
        vec!["-C", "build"],
        vec!["-Calways", "build"],
        vec!["--color=always", "build"],
    ] {
        let result = app.execute(argv, RunOptions::default());
        assert!(result.success(), "{:?}", result.error);
    }
    for (policy, expected) in [
        (DuplicatePolicy::First, "always"),
        (DuplicatePolicy::Last, "auto"),
    ] {
        let input = color.clone().duplicate(policy);
        let app =
            Cli::new("demo").default(Command::new("run").flag(input.clone()).action(|_| Ok(())));
        let result = app.execute(["--color=always", "--color"], RunOptions::default());
        assert_eq!(result.resolved.unwrap().get(&input).unwrap(), expected);
    }
}

#[cfg(unix)]
#[test]
fn native_text_and_invalid_short_names_report_encoding_errors() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let schema = CommandSchema::new("demo")
        .flag(flag::boolean("verbose").alias("v").into_schema())
        .arg(arg::string("text").optional().into_schema());
    for bytes in [b"\xff".to_vec(), b"-\xff".to_vec(), b"-v\xff".to_vec()] {
        assert_eq!(
            fujo_core::parse::parse_os(
                &schema,
                &[OsString::from_vec(bytes)],
                ParseOptions::default()
            )
            .unwrap_err()
            .code,
            "INVALID_UTF8"
        );
    }
}

#[test]
fn conflicting_inputs_are_rejected_before_creating_directories() {
    use fujo::schema::PathChecks;
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("must-not-exist");
    let path = flag::path("output").path_checks(PathChecks::directory().create());
    let dry = flag::boolean("dry").conflicts_with(&path);
    let app = Cli::new("demo").default(
        Command::new("run")
            .inputs((path, dry))
            .action(|_, _, ()| Ok(())),
    );
    let result = app.run_os_with(
        [
            std::ffi::OsString::from("--dry"),
            "--output".into(),
            output.clone().into_os_string(),
        ],
        RunOptions::default(),
        &fujo::runtime::NativeRuntime,
    );
    assert_eq!(result.error.unwrap().code, "CONFLICTING_INPUTS");
    assert!(!output.exists());
}

#[cfg(windows)]
#[test]
fn native_paths_preserve_unpaired_utf16() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let path = std::ffi::OsString::from_wide(&[b'f' as u16, 0xd800]);
    let input = flag::path("file").alias("f");
    let schema = CommandSchema::new("demo").flag(input.clone().into_schema());
    let mut attached = std::ffi::OsString::from("-f");
    attached.push(&path);
    let parsed = fujo_core::parse::parse_os(&schema, &[attached], ParseOptions::default()).unwrap();
    let resolved = fujo_core::resolve::resolve(&schema, &parsed, &Default::default()).unwrap();
    assert_eq!(
        resolved
            .get(&input)
            .unwrap()
            .unwrap()
            .as_os_str()
            .encode_wide()
            .collect::<Vec<_>>(),
        path.encode_wide().collect::<Vec<_>>()
    );
}

#[test]
fn nested_relationships_resolve_inherited_flags() {
    let spaces = flag::boolean("spaces");
    let tabs = flag::boolean("tabs").conflicts_with(&spaces);
    let app = Cli::new("demo")
        .flag(spaces)
        .command(Command::new("format").command(Command::new("svg").flag(tabs).action(|_| Ok(()))));
    let result = app.execute(["format", "svg", "--tabs"], RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(
        app.execute(
            ["format", "svg", "--tabs", "--spaces"],
            RunOptions::default()
        )
        .error
        .unwrap()
        .code,
        "CONFLICTING_INPUTS"
    );
}

#[cfg(unix)]
#[test]
fn native_path_inputs_preserve_bytes_on_unix() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let path = OsString::from_vec(b"input-\xff.svg".to_vec());
    let app = Cli::new("demo").default(
        Command::new("read")
            .inputs((arg::path("file").optional(), flag::path("input").alias("i")))
            .action(|_, (file, input), ()| {
                assert_eq!(
                    file.or(input).unwrap().as_os_str().as_bytes(),
                    b"input-\xff.svg"
                );
                Ok(())
            }),
    );
    let mut long = OsString::from("--input=");
    long.push(&path);
    let mut short = OsString::from("-i");
    short.push(&path);
    for argv in [
        vec![path.clone()],
        vec!["--input".into(), path],
        vec![long],
        vec![short],
    ] {
        let result = app.run_os_with(argv, RunOptions::default(), &fujo::runtime::NativeRuntime);
        assert!(result.success(), "{:?}", result.error);
    }
}
