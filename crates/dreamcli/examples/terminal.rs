//! A deliberately paced terminal showcase: real spinner ticks, live bars, and logs.
use dreamcli::Cli;
use dreamcli::CliError;
use dreamcli::Command;
use dreamcli::flag;
use dreamcli::json;
use dreamcli::output::Column;
use dreamcli::output::TableFormat;
use dreamcli::output::Verbosity;
use std::num::NonZeroU16;
use std::thread;
use std::time::Duration;

pub fn app() -> Cli {
    let steps = flag::parse::<NonZeroU16>("steps")
        .default(NonZeroU16::new(24).expect("nonzero constant"))
        .validate(|n| {
            if n.get() <= 100 {
                Ok(())
            } else {
                Err(CliError::validation("INVALID_STEPS", "Use 1–100 steps"))
            }
        })
        .describe("Number of simulated work items (1–100)");
    let delay = flag::duration("delay")
        .default(Duration::from_millis(100))
        .validate(|d| {
            if *d <= Duration::from_secs(1) {
                Ok(())
            } else {
                Err(CliError::validation(
                    "INVALID_DELAY",
                    "Use a delay of at most 1s",
                ))
            }
        })
        .describe("Time per item; 0ms runs without waiting");
    let fail_at = flag::parse::<NonZeroU16>("fail-at")
        .optional()
        .describe("Simulate a failure at this work item");
    let verbose = flag::boolean("verbose")
        .alias("v")
        .describe("Show debug messages");
    let demo = Command::new("demo")
        .describe("Watch live terminal output during a simulated build")
        .flag(steps.clone())
        .flag(delay.clone())
        .flag(fail_at.clone())
        .flag(verbose.clone())
        .example("{bin}", "Run the live demo (about five seconds)")
        .example("{bin} --fail-at 8", "See failure cleanup")
        .action(move |ctx| {
            let steps: u16 = ctx.get(&steps)?.get();
            let delay: Duration = ctx.get(&delay)?;
            let fail_at: Option<NonZeroU16> = ctx.get(&fail_at)?;
            if fail_at.is_some_and(|n| n.get() > steps) {
                return Err(CliError::validation(
                    "INVALID_FAIL_AT",
                    "--fail-at must be within --steps",
                ));
            }
            if ctx.get(&verbose)? && ctx.out.options().verbosity != Verbosity::Quiet {
                let mut options = ctx.out.options().clone();
                options.verbosity = Verbosity::Verbose;
                ctx.out = ctx.out.with_options(options);
            }
            ctx.out
                .status(ctx.out.color(36, "DreamCLI · live terminal demo"));
            ctx.out
                .debug(format!("Processing {steps} items with {delay:?} per item"));

            let mut spinner = ctx.out.spinner("Discovering inputs");
            thread::sleep(delay * 8); // Spinner animates while the handler is blocked.
            spinner.update("Checking dependencies");
            ctx.out
                .info("Found local inputs; checking the build environment");
            thread::sleep(delay * 8);
            spinner.succeed("Environment ready");

            let mut progress = ctx.out.progress("Building", u64::from(steps));
            for step in 1..=steps {
                thread::sleep(delay);
                if fail_at.is_some_and(|n| n.get() == step) {
                    progress.fail(format!("Build stopped at item {step}/{steps}"));
                    return Err(CliError::validation(
                        "DEMO_FAILURE",
                        format!("Simulated failure at item {step}"),
                    ));
                }
                progress.increment(1);
                ctx.out.debug(format!("Built item {step}/{steps}"));
                if step == steps.div_ceil(2) {
                    ctx.out
                        .status("Halfway through; status messages preserve the progress bar");
                }
            }
            progress.done(format!("Built {steps} items"));

            let spinner = ctx.out.spinner("Verifying results");
            thread::sleep(delay * 8);
            spinner.succeed("Verification passed");
            let rows = [
                json!({"phase":"Build", "items":steps, "status":"passed"}),
                json!({"phase":"Verify", "items":steps, "status":"passed"}),
            ];
            ctx.out.table(
                &rows,
                &[
                    Column::new("phase", "Phase"),
                    Column::new("items", "Items"),
                    Column::new("status", "Status"),
                ],
                TableFormat::Pretty,
            )
        });
    Cli::new("terminal").default(demo).completions()
}

fn main() -> std::process::ExitCode {
    app().run()
}
