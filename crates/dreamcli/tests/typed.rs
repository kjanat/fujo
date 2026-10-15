mod support;
use dreamcli::Cli;
use dreamcli::CliError;
use dreamcli::Command;
use dreamcli::RunOptions;
use dreamcli::arg;
use dreamcli::flag;
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
            assert_eq!(ctx.sources.flags["jobs"].stage, dreamcli::Stage::Env);
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
        .derive(|_, (), _previous| -> dreamcli::Result<()> {
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
