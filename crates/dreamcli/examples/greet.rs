use dreamcli::Cli;
use dreamcli::Command;
use dreamcli::arg;
use dreamcli::flag;
use std::process::ExitCode;

fn main() -> ExitCode {
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
        .arg(name.clone())
        .flag(loud.clone())
        .flag(times.clone())
        .action(move |ctx| {
            let message = format!("Hello, {}!", ctx.get(&name)?);
            let message = if ctx.get(&loud)? {
                message.to_uppercase()
            } else {
                message
            };
            for _ in 0..ctx.get(&times)? {
                ctx.out.log(&message);
            }
            Ok(())
        });
    Cli::new("greet")
        .version(env!("CARGO_PKG_VERSION"))
        .default(greet)
        .completions()
        .run()
}
