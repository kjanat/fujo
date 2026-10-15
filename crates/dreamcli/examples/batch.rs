//! Plan a bounded batch of local file operations without copying, moving, or linking files.
use dreamcli::Cli;
use dreamcli::CliError;
use dreamcli::Command;
use dreamcli::arg;
use dreamcli::config::ConfigSettings;
use dreamcli::flag;
use dreamcli::json;
use dreamcli::output::Column;
use dreamcli::output::TableFormat;
use dreamcli::schema::DuplicatePolicy;
use dreamcli::schema::PathChecks;
use dreamcli::value;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::time::Duration;

pub fn app() -> Cli {
    let label = flag::string("label")
        .env("BATCH_LABEL")
        .default("batch".into());
    let inputs = arg::path("inputs")
        .path_checks(PathChecks::file())
        .variadic();
    let output = flag::path("output")
        .alias("o")
        .path_checks(PathChecks::directory().allow_missing())
        .required();
    let mode = flag::enumeration("mode", ["copy", "move", "link"])
        .default("copy".into())
        .config("mode");
    let workers = flag::parse::<NonZeroU16>("workers")
        .alias("j")
        .env("BATCH_WORKERS")
        .config("workers")
        .validate(|v| {
            if v.get() <= 64 {
                Ok(())
            } else {
                Err(CliError::validation("TOO_MANY_WORKERS", "Use 1–64 workers"))
            }
        })
        .default(NonZeroU16::new(4).expect("nonzero constant"))
        .encode_json(|v| json!(v.get()))
        .json_schema(json!({"type":"integer","minimum":1,"maximum":64}));
    let timeout = flag::duration("timeout").default(Duration::from_secs(30));
    let budget = flag::bytes("budget").default(64 * 1024 * 1024);
    let tags = flag::array("tag", value::string().non_empty())
        .separator(",")
        .unique()
        .env("BATCH_TAGS");
    let priorities = flag::key_value_of(
        "priority",
        value::parse::<u16>().validate(|v| {
            if *v <= 100 {
                Ok(())
            } else {
                Err(CliError::validation(
                    "INVALID_PRIORITY",
                    "Priority must be 0–100",
                ))
            }
        }),
    )
    .duplicate_keys(DuplicatePolicy::Error)
    .default(BTreeMap::new());
    let explain = flag::boolean("explain");
    let plan_label = label.clone();
    let plan = Command::new("plan")
        .describe("Plan file jobs with native constraints and collision checks")
        .arg(inputs.clone())
        .flag(output.clone())
        .flag(mode.clone())
        .flag(workers.clone())
        .flag(timeout.clone())
        .flag(budget.clone())
        .flag(tags.clone())
        .flag(priorities.clone())
        .flag(explain.clone())
        .example(
            "{bin} a.txt b.txt -o out -j 8 --priority=a.txt=10 --json",
            "Plan a prioritized batch",
        )
        .derive(move |ctx| {
            let inputs: Vec<PathBuf> = ctx.get(&inputs)?;
            let output: PathBuf = ctx.get(&output)?;
            let workers: NonZeroU16 = ctx.get(&workers)?;
            let timeout: Duration = ctx.get(&timeout)?;
            let priorities: BTreeMap<String, u16> = ctx.get(&priorities)?;
            let mut targets = BTreeSet::new();
            let jobs = inputs
                .into_iter()
                .map(|source| {
                    let name = source.file_name().ok_or_else(|| {
                        CliError::validation("INVALID_INPUT", "Inputs need file names")
                    })?;
                    let target = output.join(name);
                    if target == source || !targets.insert(target.clone()) {
                        return Err(CliError::validation(
                            "TARGET_COLLISION",
                            "Every input must have a distinct output path",
                        ));
                    }
                    let priority = name
                        .to_str()
                        .and_then(|name| priorities.get(name))
                        .copied()
                        .unwrap_or(0);
                    Ok(Job {
                        source,
                        target,
                        priority,
                    })
                })
                .collect::<dreamcli::Result<Vec<_>>>()?;
            Ok(BatchPlan {
                label: ctx.get(&plan_label)?,
                mode: ctx.get(&mode)?,
                workers: workers.get(),
                timeout_ms: timeout.as_millis(),
                budget: ctx.get(&budget)?,
                tags: ctx.get(&tags)?,
                jobs,
                sources: ctx.get(&explain)?.then(|| json!(ctx.sources())),
            })
        })
        .action(|ctx| {
            let plan = ctx.state::<BatchPlan>()?;
            let mut progress = ctx.out.progress("Planning jobs", plan.jobs.len() as u64);
            for _ in &plan.jobs {
                progress.increment(1);
            }
            if ctx.out.options().json {
                ctx.out.json(plan)?;
            } else {
                ctx.out.table(
                    &plan.jobs.iter().map(|job| json!(job)).collect::<Vec<_>>(),
                    &[
                        Column::new("source", "Source"),
                        Column::new("target", "Target"),
                        Column::new("priority", "Priority"),
                    ],
                    TableFormat::Pretty,
                )?;
            }
            progress.done("Batch plan ready");
            Ok(())
        });
    Cli::new("batch")
        .version(env!("CARGO_PKG_VERSION"))
        .describe("Build a typed file-operation plan")
        .flag(label)
        .config(ConfigSettings::new("batch"))
        .command(plan)
        .completions()
}

#[derive(Serialize)]
struct Job {
    source: PathBuf,
    target: PathBuf,
    priority: u16,
}
#[derive(Serialize)]
struct BatchPlan {
    label: String,
    mode: String,
    workers: u16,
    timeout_ms: u128,
    budget: u64,
    tags: Vec<String>,
    jobs: Vec<Job>,
    sources: Option<dreamcli::Value>,
}

fn main() -> std::process::ExitCode {
    app().run()
}
