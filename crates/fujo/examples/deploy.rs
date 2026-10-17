use fujo::Cli;
use fujo::CliError;
use fujo::Command;
use fujo::arg;
use fujo::config::ConfigSettings;
use fujo::flag;
use fujo::group;
use fujo::json;
use std::process::ExitCode;
use std::str::FromStr;
use std::time::Duration;

pub fn app() -> Cli {
    let region = flag::parse::<Region>("region")
        .choices(["us", "eu"])
        .env("DEPLOY_REGION")
        .config("deploy.region")
        .default(Region::Us);
    let global_region = region.clone();
    let target = arg::string("target");
    let timeout = flag::duration("timeout").default(Duration::from_secs(30));
    let token = flag::string("token").sensitive().env("DEPLOY_TOKEN");
    let deploy = Command::new("deploy")
        .describe("Deploy to an environment")
        .arg(target.clone())
        .flag(timeout.clone())
        .flag(token.clone())
        .derive(move |ctx| {
            ctx.get(&token)?.map(Authenticated).ok_or_else(|| {
                CliError::new("AUTH_REQUIRED", "Provide DEPLOY_TOKEN")
                    .suggest("Authenticate before deploying")
            })
        })
        .action(move |ctx| {
            let region = match ctx.get(&region)? {
                Region::Us => "us",
                Region::Eu => "eu",
            };
            let _credentials = &ctx.state::<Authenticated>()?.0;
            ctx.out.status(format!(
                "Deploying with a {}s timeout",
                ctx.get(&timeout)?.as_secs()
            ));
            ctx.out
                .json(&json!({"target":ctx.get(&target)?,"region":region,"status":"deployed"}))
        });
    Cli::new("deploy")
        .version(env!("CARGO_PKG_VERSION"))
        .flag(global_region)
        .config(ConfigSettings::new("deploy"))
        .command(deploy)
        .command(
            group("db")
                .describe("Database operations")
                .command(Command::new("seed").action(|ctx| ctx.out.json(&json!({"seeded":true})))),
        )
        .completions()
}

#[derive(Clone, Copy)]
enum Region {
    Us,
    Eu,
}
impl FromStr for Region {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "us" => Ok(Self::Us),
            "eu" => Ok(Self::Eu),
            _ => Err("Use us or eu"),
        }
    }
}
struct Authenticated(String);

pub fn main() -> ExitCode {
    app().run()
}
