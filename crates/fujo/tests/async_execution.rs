mod support;
use fujo::Cli;
use fujo::CliError;
use fujo::Command;
use fujo::Context;
use fujo::Plugin;
use fujo::Result;
use fujo::RunOptions;
use fujo::output::ActivityEvent;
use fujo::output::Output;
use fujo::output::OutputOptions;
use std::sync::Arc;
use std::sync::Mutex;

#[test]
fn async_middleware_wraps_sync_derives_async_derives_and_actions_in_order() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .middleware_async(async |ctx, next| {
                ctx.out.log("outer:before");
                support::yield_once().await;
                next.run(ctx).await?;
                ctx.out.log("outer:after");
                Ok(())
            })
            .derive(|ctx| {
                ctx.out.log("sync:derive");
                Ok(7_u16)
            })
            .derive_async(async |ctx| {
                support::yield_once().await;
                Ok(ctx.state::<u16>()?.to_string())
            })
            .middleware_async(async |ctx, next| {
                ctx.out.log("inner:before");
                next.run(ctx).await?;
                ctx.out.log("inner:after");
                Ok(())
            })
            .action_async(async |ctx| {
                support::yield_once().await;
                assert_eq!(ctx.state::<String>()?, "7");
                ctx.out.log("action");
                Ok(())
            }),
    );
    let result = support::run(app.execute_async(Vec::<String>::new(), RunOptions::default()));
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(
        result.stdout,
        "outer:before\nsync:derive\ninner:before\naction\ninner:after\nouter:after\n"
    );
}

#[test]
fn async_middleware_can_short_circuit_and_wrap_an_entire_sync_tail() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .middleware_async(async |ctx, next| {
                support::yield_once().await;
                next.run(ctx).await
            })
            .middleware(|ctx, next| {
                ctx.out.log("before");
                next.run(ctx)?;
                ctx.out.log("after");
                Ok(())
            })
            .action(|ctx| {
                ctx.out.log("sync");
                Ok(())
            }),
    );
    assert_eq!(
        support::run(app.execute_async(Vec::<String>::new(), RunOptions::default())).stdout,
        "before\nsync\nafter\n"
    );
    let app = Cli::new("demo").default(
        Command::new("demo")
            .middleware_async(async |ctx, _next| {
                support::yield_once().await;
                ctx.out.log("stopped");
                Ok(())
            })
            .action_async(async |_| panic!("short-circuited action")),
    );
    assert_eq!(
        support::run(app.execute_async(Vec::<String>::new(), RunOptions::default())).stdout,
        "stopped\n"
    );
}

#[test]
fn incompatible_sync_middleware_is_rejected_before_execute_side_effects() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .middleware(|_, _| panic!("middleware must not start"))
            .action_async(async |_| panic!("action must not start")),
    );
    let result = support::run(app.execute_async(Vec::<String>::new(), RunOptions::default()));
    assert_eq!(result.error.unwrap().code, "ASYNC_MIDDLEWARE_REQUIRED");
    assert!(result.stdout.is_empty());
    assert_eq!(
        app.execute(Vec::<String>::new(), RunOptions::default())
            .error
            .unwrap()
            .code,
        "ASYNC_REQUIRED"
    );
}

#[test]
fn help_version_and_completions_do_not_require_async_execution() {
    let app = Cli::new("demo").version("1.2.3").default(
        Command::new("demo")
            .inputs(fujo::arg::string("required"))
            .derive_async(async |_, _, ()| -> Result<()> { panic!("derive must not start") })
            .action_async(async |_, _, ()| panic!("action must not start")),
    );
    #[cfg(feature = "completion")]
    let app = app.completions();
    for args in [
        vec!["--help"],
        vec!["--help", "--json"],
        vec!["--version"],
        #[cfg(feature = "completion")]
        vec!["completions", "bash"],
    ] {
        let sync = app.execute(args.clone(), RunOptions::default());
        let asynchronous = support::run(app.execute_async(args, RunOptions::default()));
        assert!(sync.success());
        assert_eq!(sync.stdout, asynchronous.stdout);
        assert!(sync.activities.is_empty());
    }
}

#[derive(Clone)]
struct Hooks {
    events: Arc<Mutex<Vec<String>>>,
    fail: &'static str,
}
impl Hooks {
    fn record(&self, stage: &str) -> Result<()> {
        self.events.lock().unwrap().push(stage.into());
        if self.fail == stage {
            Err(CliError::new("PLUGIN_FAILURE", stage))
        } else {
            Ok(())
        }
    }
}
impl Plugin for Hooks {
    fn before_parse(&self, _: &mut Vec<String>) -> Result<()> {
        self.record("parse")
    }
    fn before_execute(&self, _: &mut Context) -> Result<()> {
        self.record("before")
    }
    fn after_execute(&self, _: &Context) -> Result<()> {
        self.record("after")
    }
    fn on_error(&self, _: &CliError) {
        self.events.lock().unwrap().push("error".into());
    }
}
#[test]
fn plugin_hooks_and_failures_share_the_same_lifecycle_in_both_executors() {
    for asynchronous in [false, true] {
        for (fail, expected) in [
            ("", vec!["parse", "before", "action", "after"]),
            ("parse", vec!["parse", "error"]),
            ("before", vec!["parse", "before", "error"]),
            ("after", vec!["parse", "before", "action", "after", "error"]),
        ] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let calls = events.clone();
            let command = if asynchronous {
                Command::new("demo").action_async(async move |_| {
                    support::yield_once().await;
                    calls.lock().unwrap().push("action".into());
                    Ok(())
                })
            } else {
                Command::new("demo").action(move |_| {
                    calls.lock().unwrap().push("action".into());
                    Ok(())
                })
            };
            let app = Cli::new("demo")
                .plugin(Hooks {
                    events: events.clone(),
                    fail,
                })
                .default(command);
            let result = if asynchronous {
                support::run(app.execute_async(Vec::<String>::new(), RunOptions::default()))
            } else {
                app.execute(Vec::<String>::new(), RunOptions::default())
            };
            assert_eq!(*events.lock().unwrap(), expected);
            assert_eq!(result.success(), fail.is_empty());
        }
    }
}

#[test]
fn async_errors_can_be_transformed_and_activities_finish_on_unwind_to_caller() {
    let app = Cli::new("demo").default(
        Command::new("demo")
            .middleware_async(async |ctx, next| {
                let activity = ctx.out.spinner("outer");
                let result = next.run(ctx).await;
                activity.fail("outer failed");
                result.map_err(|_| CliError::validation("WRAPPED", "transformed failure"))
            })
            .action_async(async |ctx| {
                let _activity = ctx.out.spinner("inner");
                support::yield_once().await;
                Err(CliError::new("INNER", "inner failure"))
            }),
    );
    let result = support::run(app.execute_async(["--json"], RunOptions::default()));
    assert_eq!(result.exit_code, 2);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.stdout).unwrap()["code"],
        "WRAPPED"
    );
    assert!(matches!(
        &result.activities[..],
        [
            ActivityEvent::Start { .. },
            ActivityEvent::Start { .. },
            ActivityEvent::Finish { success: false, .. },
            ActivityEvent::Finish { success: false, .. }
        ]
    ));
}

#[test]
fn dropping_a_suspended_execution_drops_state_and_activity_handles() {
    use std::future::Future;
    use std::task::Context as TaskContext;
    use std::task::Poll;
    use std::task::Waker;
    let output = Output::capture(OutputOptions {
        tty: true,
        ..Default::default()
    });
    let dropped = Arc::new(Mutex::new(false));
    struct Guard(Arc<Mutex<bool>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            *self.0.lock().unwrap() = true;
        }
    }
    let count = dropped.clone();
    let app = Cli::new("demo").default(
        Command::new("demo")
            .inputs(())
            .derive(move |_, (), ()| Ok(Guard(count.clone())))
            .action_async(async |ctx, (), _guard| {
                let _activity = ctx.out.spinner("suspended");
                std::future::pending::<()>().await;
                Ok(())
            }),
    );
    let options = RunOptions {
        output: output.options().clone(),
        channel: Some(output.clone()),
        ..Default::default()
    };
    let mut future = Box::pin(app.execute_async(Vec::<String>::new(), options));
    assert!(matches!(
        future
            .as_mut()
            .poll(&mut TaskContext::from_waker(Waker::noop())),
        Poll::Pending
    ));
    assert_eq!(output.events().len(), 1);
    assert!(!*dropped.lock().unwrap());
    drop(future);
    assert!(*dropped.lock().unwrap());
    assert!(matches!(
        &output.events()[..],
        [
            ActivityEvent::Start { .. },
            ActivityEvent::Finish { success: false, .. }
        ]
    ));
    #[cfg(feature = "terminal-activity")]
    assert!(output.captured().stderr.ends_with("✗ suspended\n"));
}

#[test]
fn prompt_abort_precedes_sync_async_selection_and_never_starts_actions() {
    use fujo::prompt::Answer;
    use fujo::prompt::Prompt;
    use fujo::prompt::TestPrompter;
    let app = Cli::new("demo").default(
        Command::new("demo")
            .inputs(
                fujo::flag::string("name")
                    .prompt(Prompt::input("Name?"))
                    .default("fallback".into()),
            )
            .action_async(async |_, _, ()| panic!("aborted invocation must not start")),
    );
    for asynchronous in [false, true] {
        let mut options = RunOptions::default();
        options.resolve.prompter = Some(Arc::new(TestPrompter::new([Answer::Cancel])));
        let result = if asynchronous {
            support::run(app.execute_async(Vec::<String>::new(), options))
        } else {
            app.execute(Vec::<String>::new(), options)
        };
        assert_eq!(result.exit_code, 130);
        assert_eq!(result.error.unwrap().code, "PROMPT_CANCELLED");
        assert!(result.activities.is_empty());
    }
}

#[test]
fn async_writer_failure_reaches_the_run_result() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken writer"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let app = Cli::new("demo").default(Command::new("demo").action_async(async |ctx| {
        support::yield_once().await;
        ctx.out.log("data");
        Ok(())
    }));
    let options = RunOptions {
        channel: Some(Output::with_writers(
            Broken,
            std::io::sink(),
            OutputOptions::default(),
        )),
        ..Default::default()
    };
    let result = support::run(app.execute_async(Vec::<String>::new(), options));
    assert_eq!(result.error.unwrap().code, "IO_ERROR");
}
