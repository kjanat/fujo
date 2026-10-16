mod support;
use fujo::Cli;
use fujo::CliError;
use fujo::Command;
use fujo::RunOptions;
use fujo::arg;
use fujo::flag;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[test]
fn registered_values_provenance_and_native_state_reach_the_handler() {
    struct Plan {
        root: PathBuf,
        jobs: NonZeroU16,
    }
    let command = Command::new("build")
        .inputs((
            arg::path("root"),
            flag::parse::<NonZeroU16>("jobs")
                .env("JOBS")
                .default(NonZeroU16::new(4).unwrap()),
        ))
        .derive(|_, (root, jobs), ()| {
            Ok(Plan {
                root: root.clone(),
                jobs: *jobs,
            })
        })
        .action(|ctx, (_, jobs), plan| {
            assert_eq!(jobs.get(), 8);
            assert_eq!(ctx.sources.flags["jobs"].stage, fujo::Stage::Env);
            ctx.out
                .log(format!("{}:{}", plan.root.display(), plan.jobs));
            Ok(())
        });
    let mut options = RunOptions::default();
    options.resolve.env.insert("JOBS".into(), "8".into());
    let result = Cli::new("build").default(command).execute(["src"], options);
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "src:8\n");
}

#[test]
fn opaque_non_clone_inputs_are_borrowed_and_optional_inputs_are_registered() {
    struct Opaque(u16);
    let command = Command::new("demo")
        .inputs((
            flag::custom("opaque", |_| Ok(Opaque(7))).required(),
            flag::string("optional"),
        ))
        .action(|ctx, (opaque, optional), ()| {
            assert_eq!(opaque.0, 7);
            assert!(optional.is_none());
            ctx.out.log(opaque.0);
            Ok(())
        });
    let result = Cli::new("demo")
        .default(command)
        .execute(["--opaque=x"], RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn state_is_an_explicit_owned_transformation_and_can_be_local() {
    let command = Command::new("demo")
        .inputs(())
        .derive(|_, (), ()| Ok(Rc::new(String::from("first"))))
        .derive(|_, (), previous| Ok((previous, String::from("second"))))
        .action(|ctx, (), (first, second)| {
            ctx.out.log(format!("{first}/{second}"));
            Ok(())
        });
    assert_eq!(
        Cli::new("demo")
            .default(command)
            .execute(Vec::<String>::new(), RunOptions::default())
            .stdout,
        "first/second\n"
    );
}

#[test]
fn shared_globals_and_propagated_flags_work_in_nested_commands() {
    let globals = (flag::string("profile").default("dev".into()),);
    let region = flag::string("region").default("eu".into()).propagate();
    let command = Command::new("seed")
        .inputs((globals.clone(), region.clone(), arg::string("name")))
        .action(|ctx, ((profile,), region, name), ()| {
            ctx.out.log(format!("{profile}/{region}/{name}"));
            Ok(())
        });
    let app = Cli::new("demo")
        .inputs(globals)
        .command(Command::new("db").flag(region).command(command));
    for args in [
        vec!["--profile=prod", "db", "--region=us", "seed", "one"],
        vec!["db", "seed", "one", "--profile=prod", "--region=us"],
    ] {
        let result = app.execute(args, RunOptions::default());
        assert!(result.success(), "{:?}", result.error);
        assert_eq!(result.stdout, "prod/us/one\n");
    }
}

#[test]
fn failing_derivation_drops_owned_state_and_never_calls_action() {
    struct Guard(Arc<AtomicUsize>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let count = dropped.clone();
    let command = Command::new("demo")
        .inputs(())
        .derive(move |_, (), ()| Ok(Guard(count.clone())))
        .derive(|_, (), _previous| -> fujo::Result<()> {
            Err(CliError::validation("DENIED", "denied"))
        })
        .action(|_, (), ()| panic!("action must not execute"));
    let result = Cli::new("demo")
        .default(command)
        .execute(Vec::<String>::new(), RunOptions::default());
    assert_eq!(result.error.unwrap().code, "DENIED");
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn async_derivations_can_borrow_inputs_and_pass_local_state_across_suspension() {
    let command = Command::new("demo")
        .inputs(arg::path("root"))
        .derive(|_, root, ()| Ok(Rc::new(root.clone())))
        .derive_async(async |_, root, previous| {
            support::yield_once().await;
            assert_eq!(&*previous, root);
            Ok((previous, root.display().to_string()))
        })
        .derive(|_, _, previous| Ok(previous))
        .action_async(async |ctx, root, (previous, label)| {
            support::yield_once().await;
            assert_eq!(&**previous, root);
            ctx.out.log(label);
            Ok(())
        });
    let result = support::run(
        Cli::new("demo")
            .default(command)
            .execute_async(["src"], RunOptions::default()),
    );
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "src\n");
}

#[test]
fn async_state_with_sync_action_requires_async_execution_without_running_derives() {
    let called = Arc::new(AtomicUsize::new(0));
    let count = called.clone();
    let command = Command::new("demo")
        .inputs(())
        .derive_async(async move |_, (), ()| {
            count.fetch_add(1, Ordering::SeqCst);
            support::yield_once().await;
            Ok(42_u16)
        })
        .action(|ctx, (), value| {
            ctx.out.log(value);
            Ok(())
        });
    let app = Cli::new("demo").default(command);
    let sync = app.execute(Vec::<String>::new(), RunOptions::default());
    assert_eq!(sync.error.unwrap().code, "ASYNC_REQUIRED");
    assert_eq!(called.load(Ordering::SeqCst), 0);
    let result = support::run(app.execute_async(Vec::<String>::new(), RunOptions::default()));
    assert!(result.success());
    assert_eq!(result.stdout, "42\n");
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

#[test]
fn middleware_receives_the_state_at_its_position_and_wraps_later_stages() {
    let command = Command::new("demo")
        .inputs(arg::string("name"))
        .derive(|_, name, ()| Ok(name.len()))
        .middleware(|ctx, name, length, next| {
            ctx.out.log(format!("before {name}:{length}"));
            let result = next.run(length);
            ctx.out.log("after");
            result
        })
        .derive(|_, _, length| Ok(length * 2))
        .middleware(|ctx, _, doubled, next| {
            ctx.out.log(format!("inner {doubled}"));
            next.run(doubled)
        })
        .action(|ctx, _, doubled| {
            ctx.out.log(format!("action {doubled}"));
            Ok(())
        });
    let result = Cli::new("demo")
        .default(command)
        .execute(["abc"], RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "before abc:3\ninner 6\naction 6\nafter\n");
}

#[test]
fn dropping_the_continuation_short_circuits_later_stages_and_drops_state() {
    struct Guard(Arc<AtomicUsize>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let count = dropped.clone();
    let command = Command::new("demo")
        .inputs(flag::boolean("skip"))
        .derive(move |_, _, ()| Ok(Guard(count.clone())))
        .middleware(|ctx, skip, guard, next| {
            if *skip {
                ctx.out.log("skipped");
                return Ok(());
            }
            next.run(guard)
        })
        .derive(|_, _, _guard| -> fujo::Result<()> { panic!("derive must not execute") })
        .action(|_, _, ()| panic!("action must not execute"));
    let result = Cli::new("demo")
        .default(command)
        .execute(["--skip"], RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "skipped\n");
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn middleware_observes_action_failures_and_can_replace_them() {
    let command = Command::new("demo")
        .inputs(())
        .middleware(|ctx, (), state, next| {
            let activity = ctx.out.spinner("Working");
            match next.run(state) {
                Ok(()) => {
                    activity.succeed("Done");
                    Ok(())
                }
                Err(error) => {
                    activity.fail("Failed");
                    Err(CliError::new("WRAPPED", format!("wrapped: {}", error.code)).exit_code(3))
                }
            }
        })
        .action(|_, (), ()| Err(CliError::new("INNER", "inner failure")));
    let result = Cli::new("demo")
        .default(command)
        .execute(Vec::<String>::new(), RunOptions::default());
    let error = result.error.unwrap();
    assert_eq!(error.code, "WRAPPED");
    assert_eq!(error.message, "wrapped: INNER");
    assert_eq!(result.exit_code, 3);
    assert!(matches!(
        result.activities.last(),
        Some(fujo::output::ActivityEvent::Finish { success: false, .. })
    ));
}

#[test]
fn async_middleware_wraps_async_derives_and_sync_middleware_wraps_a_sync_tail() {
    let command = Command::new("demo")
        .inputs(arg::string("name"))
        .middleware_async(async |ctx, name, (), next| {
            ctx.out.log(format!("outer {name}"));
            support::yield_once().await;
            let result = next.run(()).await;
            ctx.out.log("outer done");
            result
        })
        .derive_async(async |_, name, ()| {
            support::yield_once().await;
            Ok(Rc::new(name.to_uppercase()))
        })
        .middleware(|ctx, _, upper, next| {
            ctx.out.log(format!("inner {upper}"));
            next.run(upper)
        })
        .derive(|_, _, upper| Ok(format!("{upper}!")))
        .action(|ctx, _, shout| {
            ctx.out.log(shout);
            Ok(())
        });
    let app = Cli::new("demo").default(command);
    assert_eq!(
        app.execute(["ada"], RunOptions::default())
            .error
            .unwrap()
            .code,
        "ASYNC_REQUIRED"
    );
    let result = support::run(app.execute_async(["ada"], RunOptions::default()));
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "outer ada\ninner ADA\nADA!\nouter done\n");
}

#[test]
fn dynamic_middleware_registered_before_inputs_wraps_the_whole_typed_pipeline() {
    let command = Command::new("demo")
        .middleware(|ctx, next| {
            ctx.out.log("dynamic before");
            let result = next.run(ctx);
            ctx.out.log("dynamic after");
            result
        })
        .inputs(())
        .derive(|ctx, (), ()| {
            ctx.out.log("derive");
            Ok(())
        })
        .middleware(|ctx, (), (), next| {
            ctx.out.log("typed before");
            let result = next.run(());
            ctx.out.log("typed after");
            result
        })
        .action(|ctx, (), ()| {
            ctx.out.log("action");
            Ok(())
        });
    let result = Cli::new("demo")
        .default(command)
        .execute(Vec::<String>::new(), RunOptions::default());
    assert_eq!(
        result.stdout,
        "dynamic before\nderive\ntyped before\naction\ntyped after\ndynamic after\n"
    );
}

fujo::flags! {
    struct Globals => GlobalValues {
        profile: fujo::Flag<String, fujo::Required>,
        verbose: fujo::Flag<u64, fujo::Required>,
    }
}
fujo::inputs! {
    struct Build => BuildValues {
        globals: Globals,
        root: fujo::Arg<PathBuf>,
        jobs: fujo::Flag<NonZeroU16, fujo::Required>,
        label: fujo::Flag<String>,
        limits: (fujo::Flag<u64>, fujo::Flag<u64>),
    }
}

#[test]
fn named_bundles_register_inputs_and_deliver_named_references() {
    let globals = Globals {
        profile: flag::string("profile").default("dev".into()),
        verbose: flag::count("verbose").alias("v"),
    };
    let build = Build {
        globals: globals.clone(),
        root: arg::path("root"),
        jobs: flag::parse("jobs")
            .env("JOBS")
            .default(NonZeroU16::new(4).unwrap()),
        label: flag::string("label"),
        limits: (flag::bytes("min"), flag::bytes("max")),
    };
    let command = Command::new("build")
        .inputs(build)
        .derive(|_, BuildValues { root, jobs, .. }, ()| Ok((root.clone(), *jobs)))
        .middleware(|ctx, values, plan, next| {
            let copy = values;
            ctx.out
                .log(format!("{}:{}", copy.globals.profile, copy.globals.verbose));
            next.run(plan)
        })
        .action(|ctx, values, (root, jobs)| {
            assert_eq!(values.root, root);
            assert_eq!(values.jobs, jobs);
            assert_eq!(values.label.map(String::as_str), Some("nightly"));
            assert_eq!(values.limits, (Some(&1024), None));
            assert_eq!(ctx.sources.flags["jobs"].stage, fujo::Stage::Env);
            ctx.out.log(format!("{}:{jobs}", root.display()));
            Ok(())
        });
    let app = Cli::new("demo").inputs(globals).command(command);
    let mut options = RunOptions::default();
    options.resolve.env.insert("JOBS".into(), "8".into());
    let result = app.execute(
        [
            "-vv",
            "--profile=prod",
            "build",
            "src",
            "--label=nightly",
            "--min=1kb",
        ],
        options,
    );
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "prod:2\nsrc:8\n");
    let schema = app.schema();
    assert_eq!(
        schema
            .flags
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["profile", "verbose"]
    );
    assert_eq!(
        schema.commands[0]
            .flags
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["profile", "verbose", "jobs", "label", "min", "max"]
    );
    assert_eq!(schema.commands[0].args[0].name, "root");
}

struct Trace(&'static str);
impl<I: fujo::InputSet, S> fujo::Middleware<I, S> for Trace {
    fn call<'a>(
        &'a self,
        ctx: fujo::Scope<'a>,
        _: I::Values<'a>,
        state: S,
        next: fujo::typed::Next<'a, S>,
    ) -> fujo::Result<()> {
        ctx.out.log(format!("{} in", self.0));
        let result = next.run(state);
        ctx.out.log(format!("{} out", self.0));
        result
    }
}
struct Len;
impl<I: fujo::InputSet> fujo::Derive<I, String> for Len {
    type Output = usize;
    fn call<'a>(
        &'a self,
        _: fujo::Scope<'a>,
        _: I::Values<'a>,
        state: String,
    ) -> fujo::Result<usize> {
        Ok(state.len())
    }
}

#[test]
fn struct_stages_are_reused_across_commands_and_listed_in_order() {
    let first = Command::new("first")
        .inputs(arg::string("name"))
        .derive(|_, name, ()| Ok(name.clone()))
        .wrap(Trace("first"))
        .then(Len)
        .action(|ctx, _, length| {
            ctx.out.log(length);
            Ok(())
        });
    let second = Command::new("second")
        .inputs(())
        .map(|()| String::from("second"))
        .wrap(Trace("second"))
        .then(Len)
        .action(|ctx, (), length| {
            ctx.out.log(length);
            Ok(())
        });
    let stages: Vec<_> = second
        .stages()
        .iter()
        .map(|stage| (stage.kind, stage.asynchronous))
        .collect();
    assert_eq!(
        stages,
        [
            (fujo::StageKind::Derive, false),
            (fujo::StageKind::Middleware, false),
            (fujo::StageKind::Derive, false),
        ]
    );
    assert!(second.stages()[1].handler.ends_with("Trace"));
    assert!(second.stages()[2].handler.ends_with("Len"));
    let app = Cli::new("demo").command(first).command(second);
    let result = app.execute(["first", "ada"], RunOptions::default());
    assert_eq!(result.stdout, "first in\n3\nfirst out\n");
    let result = app.execute(["second"], RunOptions::default());
    assert_eq!(result.stdout, "second in\n6\nsecond out\n");
}

struct Timed;
impl<I: fujo::InputSet, S: 'static> fujo::AsyncMiddleware<I, S> for Timed {
    fn call<'a>(
        &'a self,
        ctx: fujo::Scope<'a>,
        _: I::Values<'a>,
        state: S,
        next: fujo::typed::AsyncNext<'a, S>,
    ) -> fujo::LocalFuture<'a, fujo::Result<()>> {
        Box::pin(async move {
            ctx.out.log("timed in");
            let result = next.run(state).await;
            ctx.out.log("timed out");
            result
        })
    }
}
struct Shout;
impl<I: fujo::InputSet> fujo::AsyncDerive<I, String> for Shout {
    type Output = String;
    fn call<'a>(
        &'a self,
        _: fujo::Scope<'a>,
        _: I::Values<'a>,
        state: String,
    ) -> fujo::LocalFuture<'a, fujo::Result<String>> {
        Box::pin(async move {
            support::yield_once().await;
            Ok(state.to_uppercase())
        })
    }
}

#[test]
fn async_struct_stages_and_async_taps_run_under_async_execution() {
    let command = Command::new("demo")
        .inputs(arg::string("name"))
        .derive(|_, name, ()| Ok(name.clone()))
        .wrap_async(Timed)
        .then_async(Shout)
        .tap_async(async |ctx, name, upper| {
            support::yield_once().await;
            ctx.out.log(format!("{name} -> {upper}"));
            Ok(())
        })
        .action_async(async |ctx, _, upper| {
            ctx.out.log(upper);
            Ok(())
        });
    let stages: Vec<_> = command
        .stages()
        .iter()
        .map(|stage| (stage.kind, stage.asynchronous))
        .collect();
    assert_eq!(
        stages,
        [
            (fujo::StageKind::Derive, false),
            (fujo::StageKind::Middleware, true),
            (fujo::StageKind::Derive, true),
            (fujo::StageKind::Tap, true),
        ]
    );
    let app = Cli::new("demo").default(command);
    assert_eq!(
        app.execute(["ada"], RunOptions::default())
            .error
            .unwrap()
            .code,
        "ASYNC_REQUIRED"
    );
    let result = support::run(app.execute_async(["ada"], RunOptions::default()));
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "timed in\nada -> ADA\nADA\ntimed out\n");
}

#[test]
fn tap_observes_state_and_its_error_short_circuits() {
    let command = Command::new("demo")
        .inputs(flag::boolean("deny"))
        .derive(|_, _, ()| Ok(String::from("state")))
        .tap(|ctx, deny, state| {
            ctx.out.log(format!("tap {state}"));
            if *deny {
                Err(CliError::validation("DENIED", "denied"))
            } else {
                Ok(())
            }
        })
        .map(|state| state.len())
        .action(|ctx, _, length| {
            ctx.out.log(length);
            Ok(())
        });
    let app = Cli::new("demo").default(command);
    let result = app.execute(Vec::<String>::new(), RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "tap state\n5\n");
    let result = app.execute(["--deny"], RunOptions::default());
    assert_eq!(result.error.unwrap().code, "DENIED");
    assert_eq!(result.stdout, "tap state\n");
}

#[test]
fn catch_replaces_errors_from_later_stages_in_sync_and_async_tails() {
    let sync = Command::new("sync")
        .inputs(flag::boolean("fail"))
        .catch(|ctx, fail, error| {
            ctx.out.log(format!("caught {} fail={fail}", error.code));
            Err(CliError::new("REPLACED", "replaced").exit_code(4))
        })
        .action(|ctx, fail, ()| {
            if *fail {
                Err(CliError::new("INNER", "inner"))
            } else {
                ctx.out.log("ok");
                Ok(())
            }
        });
    let app = Cli::new("demo").default(sync);
    let result = app.execute(["--fail"], RunOptions::default());
    assert_eq!(result.stdout, "caught INNER fail=true\n");
    let error = result.error.unwrap();
    assert_eq!((error.code.as_str(), result.exit_code), ("REPLACED", 4));
    let result = app.execute(Vec::<String>::new(), RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "ok\n");
    let asynchronous = Command::new("async")
        .inputs(())
        .catch(|ctx, (), error| {
            ctx.out.log(format!("caught {}", error.code));
            Ok(())
        })
        .derive_async(async |_, (), ()| -> fujo::Result<()> {
            support::yield_once().await;
            Err(CliError::new("LATER", "later"))
        })
        .action(|_, (), ()| panic!("action must not execute"));
    assert_eq!(asynchronous.stages()[0].kind, fujo::StageKind::Catch);
    let result = support::run(
        Cli::new("demo")
            .default(asynchronous)
            .execute_async(Vec::<String>::new(), RunOptions::default()),
    );
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "caught LATER\n");
}

#[test]
fn finally_observes_both_outcomes_without_changing_them() {
    let seen = Arc::new(AtomicUsize::new(0));
    let count = seen.clone();
    let command = Command::new("demo")
        .inputs(flag::boolean("fail"))
        .finally(move |ctx, fail, result| {
            count.fetch_add(1, Ordering::SeqCst);
            ctx.out
                .log(format!("finally fail={fail} ok={}", result.is_ok()));
        })
        .derive_async(async |_, fail, ()| {
            support::yield_once().await;
            if *fail {
                Err(CliError::new("INNER", "inner"))
            } else {
                Ok(())
            }
        })
        .action(|ctx, _, ()| {
            ctx.out.log("action");
            Ok(())
        });
    assert_eq!(command.stages()[0].kind, fujo::StageKind::Finally);
    let app = Cli::new("demo").default(command);
    let result = support::run(app.execute_async(Vec::<String>::new(), RunOptions::default()));
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "action\nfinally fail=false ok=true\n");
    let result = support::run(app.execute_async(["--fail"], RunOptions::default()));
    assert_eq!(result.error.unwrap().code, "INNER");
    assert_eq!(result.stdout, "finally fail=true ok=false\n");
    assert_eq!(seen.load(Ordering::SeqCst), 2);
    let sync = Command::new("sync")
        .inputs(())
        .finally(|ctx, (), result| ctx.out.log(format!("sync ok={}", result.is_ok())))
        .action(|_, (), ()| Err(CliError::new("SYNC", "sync")));
    let result = Cli::new("demo")
        .default(sync)
        .execute(Vec::<String>::new(), RunOptions::default());
    assert_eq!(result.error.unwrap().code, "SYNC");
    assert_eq!(result.stdout, "sync ok=false\n");
}

#[test]
fn inherited_pipelines_compose_after_local_stages_and_carry_their_tail() {
    let globals = Globals {
        profile: flag::string("profile").default("dev".into()),
        verbose: flag::count("verbose"),
    };
    let shared = fujo::pipeline(globals.clone())
        .derive(|ctx, GlobalValues { profile, .. }, ()| {
            ctx.out.log(format!("shared {profile}"));
            Ok(profile.to_uppercase())
        })
        .wrap_async(Timed);
    let show = Command::new("show")
        .inputs(arg::string("name"))
        .tap(|ctx, name, ()| {
            ctx.out.log(format!("local {name}"));
            Ok(())
        })
        .inherit(&shared)
        .derive(|_, name, profile| Ok(format!("{profile}/{name}")))
        .action(|ctx, _, label| {
            ctx.out.log(label);
            Ok(())
        });
    let count = Command::new("count")
        .inputs(())
        .inherit(&shared)
        .action(|ctx, (), profile| {
            ctx.out.log(profile.len());
            Ok(())
        });
    let stages: Vec<_> = show
        .stages()
        .iter()
        .map(|stage| (stage.kind, stage.asynchronous))
        .collect();
    assert_eq!(
        stages,
        [
            (fujo::StageKind::Tap, false),
            (fujo::StageKind::Derive, false),
            (fujo::StageKind::Middleware, true),
            (fujo::StageKind::Derive, false),
        ]
    );
    let app = Cli::new("demo")
        .inputs(globals)
        .command(show)
        .command(count);
    assert_eq!(
        app.execute(["show", "ada"], RunOptions::default())
            .error
            .unwrap()
            .code,
        "ASYNC_REQUIRED"
    );
    let result =
        support::run(app.execute_async(["--profile=prod", "show", "ada"], RunOptions::default()));
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(
        result.stdout,
        "local ada\nshared prod\ntimed in\nPROD/ada\ntimed out\n"
    );
    let result = support::run(app.execute_async(["count"], RunOptions::default()));
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "shared dev\ntimed in\n3\ntimed out\n");
}

#[test]
fn dynamic_steps_are_listed_before_typed_stages() {
    let command = Command::new("demo")
        .middleware(|ctx, next| next.run(ctx))
        .derive(|_| Ok(1_u8))
        .derive_async(async |_| Ok(2_u8))
        .middleware_async(async |ctx, next| next.run(ctx).await)
        .inputs(())
        .derive(|_, (), ()| Ok(()))
        .action(|_, (), ()| Ok(()));
    let stages: Vec<_> = command
        .stages()
        .iter()
        .map(|stage| (stage.kind, stage.asynchronous))
        .collect();
    assert_eq!(
        stages,
        [
            (fujo::StageKind::Middleware, false),
            (fujo::StageKind::Derive, false),
            (fujo::StageKind::Derive, true),
            (fujo::StageKind::Middleware, true),
            (fujo::StageKind::Derive, false),
        ]
    );
    assert!(
        command
            .stages()
            .iter()
            .all(|stage| stage.handler.contains("dynamic_steps_are_listed"))
    );
}
