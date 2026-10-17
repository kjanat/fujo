use fujo::Cli;
use fujo::Command;
use fujo::arg;
use fujo::flag;
use std::process::ExitCode;

pub fn app() -> Cli {
    let name = arg::string("name").describe("Who to greet");
    let loud = flag::boolean("loud")
        .alias("l")
        .describe("Shout the greeting");
    let times = flag::parse::<u16>("times")
        .alias("t")
        .default(1)
        .describe("Repeat count");
    let greet = Command::new("greet")
        .describe("Greet someone")
        .inputs((name, loud, times))
        .action(|ctx, (name, loud, times), ()| {
            let message = format!("Hello, {name}!");
            let message = if *loud {
                message.to_uppercase()
            } else {
                message
            };
            for _ in 0..*times {
                ctx.out.log(&message);
            }
            Ok(())
        });
    Cli::new("greet")
        .version(env!("CARGO_PKG_VERSION"))
        .default(greet)
        .completions()
}

pub fn main() -> ExitCode {
    app().run()
}
