//! Exercise the actual example definitions, rather than copying their CLI logic into tests.
#[allow(dead_code)]
#[path = "../examples/batch.rs"]
mod batch;
#[allow(dead_code)]
#[path = "../examples/release.rs"]
mod release;
#[allow(dead_code)]
#[path = "../examples/settings.rs"]
mod settings;

use dreamcli::Cli;
use dreamcli::RunOptions;
use dreamcli::RunResult;
use dreamcli::Value;
use dreamcli::json;
use dreamcli::prompt::Answer;
use dreamcli::prompt::TestPrompter;
use dreamcli::resolve::CheckPolicy;
use dreamcli::runtime::NativeRuntime;
use std::sync::Arc;

fn checked() -> RunOptions {
    let mut options = RunOptions::default();
    options.resolve.filesystem = Some(Arc::new(NativeRuntime));
    options.resolve.filesystem_policy = CheckPolicy::Required;
    options
}
fn invoke(app: Cli, args: &[&str], options: RunOptions) -> RunResult {
    app.execute(args.iter().copied(), options)
}
fn document(result: &RunResult) -> Value {
    assert!(result.success(), "{:?}: {}", result.error, result.stderr);
    serde_json::from_str(&result.stdout).unwrap()
}

#[test]
fn release_resolves_precedence_derives_a_typed_plan_and_redacts_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = dir.path().join("package.tar");
    std::fs::write(&artifact, "artifact fixture").unwrap();
    let mut options = checked();
    options.resolve.config = json!({"release":{"from":"1.2.3","bump":"minor"}});
    options
        .resolve
        .env
        .insert("SHIPYARD_VERSION".into(), "2.4.8".into());
    options.resolve.prompter = Some(Arc::new(TestPrompter::new([Answer::from("PRIVATE_TOKEN")])));
    let result = invoke(
        release::app(),
        &["plan", artifact.to_str().unwrap(), "--publish", "--json"],
        options.clone(),
    );
    let plan = document(&result);
    assert_eq!(plan["current"], "2.4.8");
    assert_eq!(plan["next"], "2.5.0");
    assert_eq!(plan["publish"], true);
    assert!(result.skipped_checks.is_empty());
    assert!(!format!("{result:?}").contains("PRIVATE_TOKEN"));
    assert_eq!(result.activities.len(), 2);
    let result = invoke(
        release::app(),
        &["plan", artifact.to_str().unwrap(), "--from=3.0.0", "--json"],
        options,
    );
    assert_eq!(document(&result)["next"], "3.1.0");
    assert_eq!(
        std::fs::read_to_string(artifact).unwrap(),
        "artifact fixture"
    );
}

#[test]
fn release_reports_auth_filesystem_codec_and_version_overflow_errors() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = dir.path().join("package.tar");
    std::fs::write(&artifact, "fixture").unwrap();
    let artifact = artifact.to_str().unwrap();
    for (args, code) in [
        (vec!["plan", artifact, "--publish"], "AUTH_REQUIRED"),
        (vec!["plan", artifact, "--from=1.02.3"], "INVALID_VALUE"),
        (
            vec!["plan", artifact, "--from=4294967295.0.0", "--bump=major"],
            "VERSION_OVERFLOW",
        ),
    ] {
        assert_eq!(
            invoke(release::app(), &args, checked()).error.unwrap().code,
            code
        );
    }
    let missing = dir.path().join("missing.tar");
    assert_eq!(
        invoke(
            release::app(),
            &["plan", missing.to_str().unwrap()],
            checked()
        )
        .exit_code,
        2
    );
}

#[test]
fn batch_combines_native_limits_collections_provenance_and_progress_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.txt");
    let output = dir.path().join("out");
    std::fs::write(&input, "unchanged").unwrap();
    let result = invoke(
        batch::app(),
        &[
            "plan",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "-j8",
            "--timeout=1m30s",
            "--budget=2mb",
            "--tag=fast,fast,local",
            "--priority=input.txt=20",
            "--explain",
            "--json",
        ],
        checked(),
    );
    let plan = document(&result);
    assert_eq!(plan["workers"], 8);
    assert_eq!(plan["timeout_ms"], 90000);
    assert_eq!(plan["budget"], 2 * 1024 * 1024);
    assert_eq!(plan["tags"], json!(["fast", "local"]));
    assert_eq!(plan["jobs"][0]["priority"], 20);
    assert_eq!(plan["sources"]["flags"]["workers"]["stage"], "cli");
    assert!(result.activities.len() >= 3);
    assert!(result.skipped_checks.is_empty());
    assert!(!output.exists());
    assert_eq!(std::fs::read_to_string(input).unwrap(), "unchanged");
}

#[test]
fn batch_rejects_invalid_workers_and_target_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.txt");
    std::fs::write(&input, "fixture").unwrap();
    for invalid in ["0", "65", "not-a-number"] {
        let args = [
            "plan",
            input.to_str().unwrap(),
            "-o",
            "output",
            "--workers",
            invalid,
        ];
        assert!(!invoke(batch::app(), &args, checked()).success());
    }
    let second = dir.path().join("nested");
    std::fs::create_dir(&second).unwrap();
    let duplicate = second.join("input.txt");
    std::fs::write(&duplicate, "fixture").unwrap();
    let result = invoke(
        batch::app(),
        &[
            "plan",
            input.to_str().unwrap(),
            duplicate.to_str().unwrap(),
            "-o",
            "output",
        ],
        checked(),
    );
    assert_eq!(result.error.unwrap().code, "TARGET_COLLISION");
}

#[test]
fn settings_resolves_nested_global_flags_stdin_config_and_redacted_provenance() {
    let mut options = RunOptions::default();
    options.resolve.config = json!({"profile":"prod","server":{"port":9000},"features":["cache"],"limits":{"requests":50}});
    options
        .resolve
        .env
        .insert("SETTINGS_PORT".into(), "9001".into());
    options.resolve.stdin = Some("PRIVATE_TOKEN\n".into());
    let result = invoke(
        settings::app(),
        &[
            "config",
            "show",
            "--port=9002",
            "--token=-",
            "--feature=metrics,metrics",
            "--json",
        ],
        options,
    );
    let settings = document(&result);
    assert_eq!(settings["address"], "localhost:9002");
    assert_eq!(settings["values"]["token"], "[redacted]");
    assert_eq!(settings["values"]["profile"], "prod");
    assert_eq!(settings["values"]["limit"]["requests"], 50);
    assert_eq!(settings["sources"]["flags"]["token"]["via"], "stdin");
    assert_eq!(settings["values"]["feature"], json!(["metrics"]));
    assert!(!format!("{result:?}").contains("PRIVATE_TOKEN"));
}

#[test]
fn settings_supports_validation_schema_and_cross_input_policy() {
    let result = invoke(
        settings::app(),
        &[
            "config",
            "validate",
            "--profile=prod",
            "--endpoint=http://localhost",
        ],
        RunOptions::default(),
    );
    assert_eq!(result.error.unwrap().code, "HTTPS_REQUIRED");
    let result = invoke(
        settings::app(),
        &[
            "config",
            "validate",
            "--profile=dev",
            "--endpoint=http://localhost",
        ],
        RunOptions::default(),
    );
    assert_eq!(document(&result)["valid"], true);
    let result = invoke(
        settings::app(),
        &["config", "schema"],
        RunOptions::default(),
    );
    assert_eq!(
        document(&result)["properties"]["flags"]["properties"]["profile"]["enum"],
        json!(["dev", "prod"])
    );
}

#[test]
fn every_example_has_non_executing_json_help_and_four_shell_completions() {
    for app in [release::app(), batch::app(), settings::app()] {
        let result = invoke(app.clone(), &["--help", "--json"], RunOptions::default());
        assert_eq!(document(&result)["format"], "dreamcli-rust");
        assert!(result.stderr.is_empty());
        for shell in ["bash", "zsh", "fish", "powershell"] {
            let result = invoke(app.clone(), &["completions", shell], RunOptions::default());
            assert!(result.success());
            assert!(!result.stdout.is_empty());
        }
    }
}
