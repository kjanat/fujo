use fujo::{Cli, CliError, Command, Flag, RunOptions, flag};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn assert_conflicting_modes(local: Flag<String>, inherited: Flag<String>) {
    for (local, inherited) in [(local.clone(), inherited.clone()), (inherited, local)] {
        let pipeline = fujo::pipeline(inherited).derive(|_, mode, ()| Ok(mode.cloned()));
        let command = Command::new("run")
            .inputs(local.clone())
            .inherit(&pipeline)
            .action(|_, _, _| panic!("conflicting declarations must prevent execution"));
        let root = Cli::new("demo").share(&pipeline).command(
            Command::new("run")
                .inputs(local.clone())
                .action(|_, _, ()| panic!("conflicting declarations must prevent execution")),
        );
        let group = Cli::new("demo").command(
            Command::new("group").share(&pipeline).command(
                Command::new("run")
                    .inputs(local.clone())
                    .action(|_, _, ()| panic!("conflicting declarations must prevent execution")),
            ),
        );
        let root_registration = Cli::new("demo")
            .inputs(local.clone())
            .share(&pipeline)
            .command(Command::new("run").action(|_| panic!("must not execute")));
        let group_registration = Cli::new("demo").command(
            Command::new("group")
                .flag(local)
                .share(&pipeline)
                .command(Command::new("run").action(|_| panic!("must not execute"))),
        );
        for (app, argv) in [
            (
                Cli::new("demo").command(command),
                vec!["run", "--mode=safe"],
            ),
            (root, vec!["run", "--mode=safe"]),
            (group, vec!["group", "run", "--mode=safe"]),
            (root_registration, vec!["run", "--mode=safe"]),
            (group_registration, vec!["group", "run", "--mode=safe"]),
        ] {
            let result = app.execute(argv, RunOptions::default());
            let error = result.error.expect("incompatible declarations must fail");
            assert_eq!(error.code, "INVALID_SCHEMA");
            assert!(error.message.contains("mode"), "{error:?}");
            assert!(result.stdout.is_empty());
        }
    }
}

#[test]
fn independent_enum_declarations_cannot_discard_validation() {
    assert_conflicting_modes(flag::string("mode"), flag::enumeration("mode", ["safe"]));
    assert_conflicting_modes(
        flag::enumeration("mode", ["safe", "fast"]),
        flag::enumeration("mode", ["safe", "strict"]),
    );
}

#[test]
fn independent_custom_validators_cannot_be_discarded() {
    assert_conflicting_modes(
        flag::string("mode"),
        flag::string("mode").validate(|mode| {
            if mode == "safe" {
                Ok(())
            } else {
                Err(CliError::new("UNSAFE_MODE", "Expected safe mode"))
            }
        }),
    );
}

#[test]
fn modified_clones_are_distinct_declarations() {
    let mode = flag::string("mode");
    assert_conflicting_modes(mode.clone(), mode.clone().choices(["safe"]));
    assert_conflicting_modes(mode.clone(), mode.clone().min_length(4));
    assert_conflicting_modes(mode.clone(), mode.clone().env("MODE"));
    assert_conflicting_modes(mode.clone(), mode.clone().validate(|_| Ok(())));
    assert_conflicting_modes(mode.clone(), mode.clone().encode_json(|_| "safe".into()));
}

#[test]
fn independently_created_identical_declarations_require_explicit_reuse() {
    assert_conflicting_modes(flag::string("mode"), flag::string("mode"));
}

#[test]
fn unchanged_clones_deduplicate_and_keep_validation() {
    let mode = flag::enumeration("mode", ["safe", "blocked"]).validate(|mode| {
        if mode == "blocked" {
            Err(CliError::new("BLOCKED_MODE", "Mode is blocked"))
        } else {
            Ok(())
        }
    });
    let pipeline = fujo::pipeline(mode.clone()).derive(|_, mode, ()| Ok(mode.cloned()));
    let command = Command::new("run")
        .inputs(mode.clone())
        .inherit(&pipeline)
        .action(|ctx, mode, inherited| {
            assert_eq!(mode, inherited.as_ref());
            ctx.out.log(mode.unwrap());
            Ok(())
        });
    assert_eq!(command.schema().flags.len(), 1);
    let app = Cli::new("demo")
        .inputs(mode)
        .share(&pipeline)
        .command(Command::new("group").share(&pipeline).command(command));
    assert_eq!(app.schema().flags.len(), 1);
    let result = app.execute(["group", "run", "--mode=safe"], RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "safe\n");
    for mode in ["--mode=fast", "--mode=blocked"] {
        let result = app.execute(["group", "run", mode], RunOptions::default());
        assert!(!result.success());
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn repeated_shared_clones_without_local_inputs_deduplicate() {
    let validations = Arc::new(AtomicUsize::new(0));
    let calls = validations.clone();
    let mode = flag::enumeration("mode", ["safe"]).validate(move |_| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    });
    let pipeline = fujo::pipeline(mode).derive(|_, mode, ()| Ok(mode.cloned()));
    let app = Cli::new("demo").share(&pipeline).command(
        Command::new("group").share(&pipeline).command(
            Command::new("run")
                .inputs(())
                .shared(&pipeline)
                .action(|ctx, (), mode| {
                    ctx.out.log(mode.as_ref().unwrap());
                    Ok(())
                }),
        ),
    );
    let result = app.execute(["group", "run", "--mode=safe"], RunOptions::default());
    assert!(result.success(), "{:?}", result.error);
    assert_eq!(result.stdout, "safe\n");
    assert_eq!(validations.load(Ordering::Relaxed), 1);
}

#[test]
fn nested_global_and_propagated_conflicts_fail_before_any_stage() {
    let mode = flag::enumeration("mode", ["safe"]);
    let pipeline = fujo::pipeline(mode.clone())
        .tap(|_, _, ()| panic!("schema conflicts must be rejected before stages"));
    let middle = || {
        Command::new("middle").flag(mode.clone()).command(
            Command::new("run")
                .inputs(flag::string("mode"))
                .action(|_, _, ()| panic!("must not execute")),
        )
    };
    for (app, argv) in [
        (
            Cli::new("demo").share(&pipeline).command(middle()),
            vec!["middle", "run", "--mode=safe"],
        ),
        (
            Cli::new("demo").command(Command::new("group").share(&pipeline).command(middle())),
            vec!["group", "middle", "run", "--mode=safe"],
        ),
    ] {
        let error = app.execute(argv, RunOptions::default()).error.unwrap();
        assert_eq!(error.code, "INVALID_SCHEMA");
        assert_eq!(error.message, "Input 'mode' has conflicting declarations");
    }
}

#[test]
fn parser_reuse_creates_distinct_named_declarations() {
    let parser = fujo::value::string();
    assert_conflicting_modes(parser.clone().flag("mode"), parser.flag("mode"));
}

#[test]
fn modified_defaults_and_positional_choices_conflict() {
    let mode = flag::string("mode").default("safe".into());
    let pipeline = fujo::pipeline(mode.clone());
    let command = Command::new("run")
        .inputs(mode.default("fast".into()))
        .inherit(&pipeline)
        .action(|_, _, ()| panic!("must not execute"));
    let error = Cli::new("demo")
        .default(command)
        .execute(Vec::<String>::new(), RunOptions::default())
        .error
        .unwrap();
    assert_eq!(error.code, "INVALID_SCHEMA");

    let mode = fujo::arg::string("mode");
    let pipeline = fujo::pipeline(mode.clone().choices(["safe"]));
    let command = Command::new("run")
        .inputs(mode)
        .inherit(&pipeline)
        .action(|_, _, ()| panic!("must not execute"));
    let error = Cli::new("demo")
        .default(command)
        .execute(["fast"], RunOptions::default())
        .error
        .unwrap();
    assert_eq!(error.code, "INVALID_SCHEMA");
}
