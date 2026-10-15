//! A typed async build simulation; Tokio belongs to this application, not Fujo.
use fujo::Cli;
use fujo::CliError;
use fujo::Command;
use fujo::flag;
use fujo::json;
use std::num::NonZeroU16;
use std::time::Duration;

pub fn app() -> Cli {
    let globals = (flag::string("profile")
        .env("BUILD_PROFILE")
        .default("dev".into()),);
    let jobs = flag::parse::<NonZeroU16>("jobs")
        .default(NonZeroU16::new(12).unwrap())
        .validate(|n| {
            if n.get() <= 100 {
                Ok(())
            } else {
                Err(CliError::validation("INVALID_JOBS", "Use 1–100 jobs"))
            }
        });
    let delay = flag::duration("delay")
        .default(Duration::from_millis(100))
        .validate(|d| {
            if *d <= Duration::from_secs(1) {
                Ok(())
            } else {
                Err(CliError::validation("INVALID_DELAY", "Use at most 1s"))
            }
        });
    let command = Command::new("build")
        .describe("Simulate a build with typed state and asynchronous waits")
        .middleware_async(async |ctx, next| {
            ctx.out.status("Starting async build");
            let result = next.run(ctx).await;
            ctx.out.status("Build invocation finished");
            result
        })
        .inputs((globals.clone(), jobs, delay))
        .derive_async(async |ctx, ((profile,), jobs, delay), ()| {
            let spinner = ctx.out.spinner("Checking build environment");
            tokio::time::sleep(*delay * 8).await;
            spinner.succeed("Environment ready");
            Ok(Build {
                profile: profile.clone(),
                jobs: *jobs,
                delay: *delay,
            })
        })
        .action_async(async |ctx, _, build| {
            let mut progress = ctx.out.progress("Building", u64::from(build.jobs.get()));
            for _ in 0..build.jobs.get() {
                tokio::time::sleep(build.delay).await;
                progress.increment(1);
            }
            progress.done("Build complete");
            if ctx.out.options().json {
                ctx.out.json(
                    &json!({"profile":build.profile, "jobs":build.jobs.get(), "status":"passed"}),
                )
            } else {
                ctx.out
                    .log(format!("{}: {} jobs passed", build.profile, build.jobs));
                Ok(())
            }
        });
    Cli::new("async-build")
        .inputs(globals)
        .default(command)
        .completions()
}

struct Build {
    profile: String,
    jobs: NonZeroU16,
    delay: Duration,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    app().run_async().await
}
