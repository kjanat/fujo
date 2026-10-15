//! Abstractions remain usable independently of optional native implementations.
use fujo::Cli;
use fujo::Command;
use fujo::RunOptions;
use fujo::flag;
use fujo::json;
use fujo::output::ActivityEvent;
use fujo::output::Output;
use fujo::output::OutputOptions;
use fujo::prompt::Answer;
use fujo::prompt::Prompt;
use fujo::prompt::TestPrompter;
use std::sync::Arc;

#[test]
fn injected_prompts_provenance_and_typed_state_work_without_terminal_ui() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .inputs(
                flag::string("name")
                    .prompt(Prompt::input("Name?"))
                    .required(),
            )
            .derive(|_, name, ()| Ok(name.to_uppercase()))
            .action(|ctx, _, name| {
                assert_eq!(ctx.sources.flags["name"].stage, fujo::Stage::Prompt);
                ctx.out.json(&json!({"name":name}))
            }),
    );
    let mut options = RunOptions::default();
    options.resolve.prompter = Some(Arc::new(TestPrompter::new([Answer::from("Ada")])));
    let result = app.execute(["--json"], options);
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "{\"name\":\"ADA\"}\n");
}

#[test]
fn duration_and_size_parsers_do_not_need_regex() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .inputs((
                flag::duration("timeout").required(),
                flag::bytes("size").required(),
            ))
            .action(|ctx, (timeout, size), ()| {
                ctx.out
                    .json(&json!({"ms":timeout.as_millis(), "bytes":size}))
            }),
    );
    for (timeout, size, ms, bytes) in [
        ("1h30m", "1.5MB", 5_400_000, 1_572_864),
        ("1ms2ms", "1 Kb", 3, 1024),
        ("250", "2\u{2003}kb", 250, 2048),
        ("0.5s", "12 ", 500, 12),
    ] {
        let result = app.execute(
            [format!("--timeout={timeout}"), format!("--size={size}")],
            RunOptions::default(),
        );
        assert!(result.success(), "{:?}", result.error);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.stdout).unwrap(),
            json!({"ms":ms,"bytes":bytes})
        );
    }
    for (timeout, size) in [
        ("1h30", "1kb"),
        ("1S", "1kb"),
        (".5s", "1kb"),
        ("1s", "1kb "),
        ("1s", "1e3"),
        ("1s", "-1kb"),
    ] {
        assert!(
            !app.execute(
                [format!("--timeout={timeout}"), format!("--size={size}")],
                RunOptions::default()
            )
            .success()
        );
    }
}

#[test]
fn logical_activities_are_recorded_with_or_without_the_native_backend() {
    let output = Output::capture(OutputOptions::default());
    output.spinner("work").succeed("ready");
    let mut progress = output.progress("items", 2);
    progress.increment(1);
    drop(progress);
    assert!(matches!(
        &output.events()[..],
        [
            ActivityEvent::Start { .. },
            ActivityEvent::Finish { success: true, .. },
            ActivityEvent::Start { .. },
            ActivityEvent::Update {
                current: 1,
                total: 2,
                ..
            },
            ActivityEvent::Finish { success: false, .. }
        ]
    ));
    assert!(output.captured().stdout.is_empty());
    assert!(output.captured().stderr.is_empty());
}

#[cfg(not(feature = "terminal-activity"))]
#[test]
fn a_tty_with_writers_still_has_no_animation_when_the_backend_is_disabled() {
    #[derive(Clone)]
    struct Writer(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let data = Arc::new(std::sync::Mutex::new(Vec::new()));
    let output = Output::with_writers(
        Writer(data.clone()),
        Writer(data.clone()),
        OutputOptions {
            tty: true,
            ..Default::default()
        },
    );
    output.spinner("work").succeed("ready");
    output.progress("items", 2).done("done");
    output.finish().unwrap();
    assert!(data.lock().unwrap().is_empty());
    assert_eq!(output.events().len(), 4);
}

#[cfg(feature = "regex")]
#[test]
fn regex_constraints_are_an_independent_integration() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .inputs(flag::string("name").pattern("^[a-z]+$").required())
            .action(|_, _, ()| Ok(())),
    );
    assert!(app.execute(["--name=ada"], RunOptions::default()).success());
    assert_eq!(
        app.execute(["--name=Ada"], RunOptions::default())
            .error
            .unwrap()
            .code,
        "INVALID_VALUE"
    );
}

#[cfg(feature = "url")]
#[test]
fn url_parsing_is_an_independent_native_integration() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .inputs(flag::url("endpoint").protocols(["https"]).required())
            .action(|ctx, endpoint, ()| {
                ctx.out.log(endpoint.scheme());
                Ok(())
            }),
    );
    assert_eq!(
        app.execute(["--endpoint=https://example.com"], RunOptions::default())
            .stdout,
        "https\n"
    );
    assert!(
        !app.execute(["--endpoint=http://example.com"], RunOptions::default())
            .success()
    );
}

#[cfg(feature = "chrono")]
#[test]
fn chrono_parsing_does_not_require_the_regex_feature() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .inputs(flag::date("date").required())
            .action(|ctx, date, ()| {
                ctx.out.log(date.to_rfc3339());
                Ok(())
            }),
    );
    for value in [
        "2026-10-15",
        "2026-10-15T12:30",
        "2026-10-15T12:30Z",
        "2026-10-15T12:30:25.125+02:00",
    ] {
        assert!(
            app.execute([format!("--date={value}")], RunOptions::default())
                .success(),
            "{value}"
        );
    }
    for value in [
        "2026-02-31",
        "2026-10-15T12:30.1",
        "2026-10-15T12:30:60Z",
        "2026-10-15T12:30:00.1234Z",
        "2026-10-15t12:30Z",
    ] {
        assert!(
            !app.execute([format!("--date={value}")], RunOptions::default())
                .success(),
            "{value}"
        );
    }
}
