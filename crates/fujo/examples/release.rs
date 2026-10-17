use fujo::Cli;
use fujo::CliError;
use fujo::Command;
use fujo::Result;
use fujo::arg;
use fujo::config::ConfigSettings;
use fujo::flag;
use fujo::json;
use fujo::prompt::Prompt;
use fujo::schema::PathChecks;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

pub fn app() -> Cli {
    let artifact = arg::path("artifact")
        .path_checks(PathChecks::file())
        .describe("Existing release artifact");
    let version = flag::parse::<Version>("from")
        .env("SHIPYARD_VERSION")
        .config("release.from")
        .default(Version(1, 0, 0))
        .encode_json(|v| json!(v.to_string()))
        .json_schema(json!({"type":"string","pattern":"^[0-9]+\\.[0-9]+\\.[0-9]+$"}))
        .describe("Current A.B.C version");
    let bump = flag::parse::<Bump>("bump")
        .choices(["patch", "minor", "major"])
        .config("release.bump")
        .default(Bump::Patch)
        .encode_json(|v| json!(v.name()))
        .json_schema(json!({"type":"string","enum":["patch","minor","major"]}));
    let publish = flag::boolean("publish")
        .config("release.publish")
        .describe("Include an authenticated publish step in the plan");
    let token = flag::string("token")
        .non_empty()
        .sensitive()
        .env("SHIPYARD_TOKEN");
    let registry = flag::url("registry")
        .protocols(["https"])
        .config("release.registry")
        .default(
            "https://registry.example.invalid"
                .parse()
                .expect("constant URL"),
        );
    let (ask_publish, ask_token) = (publish.clone(), token.clone());
    let plan = Command::new("plan")
        .describe("Compute the next version and validate a release plan")
        .arg(artifact.clone())
        .flag(version.clone())
        .flag(bump.clone())
        .flag(publish.clone())
        .flag(token.clone())
        .flag(registry.clone())
        .example(
            "{bin} package.tar --from=1.4.2 --bump=minor --json",
            "Plan a minor release",
        )
        .interactive(move |inputs| {
            Ok(
                if inputs.peek(&ask_publish)? == Some(&true) && inputs.peek(&ask_token)?.is_none() {
                    BTreeMap::from([(
                        "token".into(),
                        Some(Prompt::input("Registry token?").secret().validate(|s| {
                            if s.trim().is_empty() {
                                Err(CliError::validation("EMPTY_TOKEN", "Enter a token"))
                            } else {
                                Ok(())
                            }
                        })),
                    )])
                } else {
                    BTreeMap::new()
                },
            )
        })
        .derive(move |ctx| {
            let current: Version = ctx.get(&version)?;
            let bump: Bump = ctx.get(&bump)?;
            let publish: bool = ctx.get(&publish)?;
            if publish && ctx.get_ref(&token)?.is_none() {
                return Err(CliError::validation(
                    "AUTH_REQUIRED",
                    "Publish plans require --token, SHIPYARD_TOKEN, or an interactive answer",
                ));
            }
            Ok(ReleasePlan {
                artifact: ctx.get(&artifact)?,
                current: current.to_string(),
                next: current.bump(bump)?.to_string(),
                registry: ctx.get(&registry)?.to_string(),
                publish,
            })
        })
        .middleware(|ctx, next| {
            let activity = ctx.out.spinner("Planning release");
            let result = next.run(ctx);
            if result.is_ok() {
                activity.succeed("Release plan ready");
            } else {
                activity.fail("Release plan failed");
            }
            result
        })
        .action(|ctx| {
            let plan = ctx.state::<ReleasePlan>()?;
            if ctx.out.options().json {
                ctx.out.json(plan)
            } else {
                ctx.out.log(format!(
                    "{} -> {}: {}",
                    plan.current,
                    plan.next,
                    plan.artifact.display()
                ));
                Ok(())
            }
        });
    Cli::new("shipyard")
        .version(env!("CARGO_PKG_VERSION"))
        .describe("Typed release planning without publishing")
        .config(ConfigSettings::new("shipyard"))
        .command(plan)
        .completions()
}

#[derive(Clone, Copy)]
enum Bump {
    Patch,
    Minor,
    Major,
}
impl Bump {
    fn name(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
        }
    }
}
impl FromStr for Bump {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "patch" => Ok(Self::Patch),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            _ => Err("Choose patch, minor, or major"),
        }
    }
}
#[derive(Clone, Copy)]
struct Version(u32, u32, u32);
impl FromStr for Version {
    type Err = &'static str;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let parts = s.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err("Use an A.B.C version");
        }
        let number = |s: &str| {
            if s.is_empty()
                || !s.bytes().all(|c| c.is_ascii_digit())
                || (s.len() > 1 && s.starts_with('0'))
            {
                return Err("Use decimal version components without leading zeros");
            }
            s.parse::<u32>()
                .map_err(|_| "Version component is too large")
        };
        Ok(Self(
            number(parts[0])?,
            number(parts[1])?,
            number(parts[2])?,
        ))
    }
}
impl Version {
    fn bump(self, bump: Bump) -> Result<Self> {
        let increment = |v: u32| {
            v.checked_add(1).ok_or_else(|| {
                CliError::validation(
                    "VERSION_OVERFLOW",
                    "Version component cannot be incremented",
                )
            })
        };
        Ok(match bump {
            Bump::Patch => Self(self.0, self.1, increment(self.2)?),
            Bump::Minor => Self(self.0, increment(self.1)?, 0),
            Bump::Major => Self(increment(self.0)?, 0, 0),
        })
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}
#[derive(Serialize)]
struct ReleasePlan {
    artifact: PathBuf,
    current: String,
    next: String,
    registry: String,
    publish: bool,
}

pub fn main() -> std::process::ExitCode {
    app().run()
}
