//! Fingerprint a directory tree into a manifest, then detect drift against it.
use fujo::Arg;
use fujo::AsyncMiddleware;
use fujo::Cli;
use fujo::CliError;
use fujo::Command;
use fujo::Flag;
use fujo::InputSet;
use fujo::LocalFuture;
use fujo::Required;
use fujo::Scope;
use fujo::arg;
use fujo::config::ConfigSettings;
use fujo::flag;
use fujo::json;
use fujo::output::Column;
use fujo::output::TableFormat;
use fujo::schema::PathChecks;
use fujo::typed::AsyncNext;
use fujo::value;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

fujo::flags! {
    pub struct Globals => GlobalValues {
        jobs: Flag<NonZeroUsize, Required>,
        ignore: Flag<Vec<String>, Required>,
    }
}
fujo::inputs! {
    pub struct ScanInputs => ScanValues {
        globals: Globals,
        root: Arg<PathBuf>,
    }
}
fujo::inputs! {
    pub struct BuildInputs => BuildValues {
        globals: Globals,
        root: Arg<PathBuf>,
        out: Flag<PathBuf, Required>,
        force: Flag<bool, Required>,
    }
}
fujo::inputs! {
    pub struct VerifyInputs => VerifyValues {
        globals: Globals,
        root: Arg<PathBuf>,
        manifest: Flag<PathBuf, Required>,
    }
}

pub fn app() -> Cli {
    let globals = Globals {
        jobs: flag::parse("jobs")
            .alias("j")
            .env("MANIFEST_JOBS")
            .config("jobs")
            .default(NonZeroUsize::new(4).expect("nonzero constant"))
            .validate(|jobs| {
                if jobs.get() <= 64 {
                    Ok(())
                } else {
                    Err(CliError::validation("INVALID_JOBS", "Use 1–64 jobs"))
                }
            })
            .encode_json(|jobs| json!(jobs.get()))
            .json_schema(json!({"type":"integer","minimum":1,"maximum":64}))
            .describe("Concurrent fingerprint workers"),
        ignore: flag::array("ignore", value::string().non_empty())
            .separator(",")
            .unique()
            .env("MANIFEST_IGNORE")
            .config("ignore")
            .default(vec![".git".into(), "target".into()])
            .describe("File or directory names to skip"),
    };
    let root = || {
        arg::path("root")
            .path_checks(PathChecks::directory())
            .describe("Directory to fingerprint")
    };
    let scanner = fujo::pipeline(ScanInputs {
        globals: globals.clone(),
        root: root(),
    })
    .derive(|ctx, ScanValues { globals, root }, ()| scan(ctx, root, globals.ignore))
    .wrap_async(Timed);
    let build = Command::new("build")
        .describe("Fingerprint every file under a directory into a manifest")
        .example("{bin} . --out manifest.json", "Write a manifest")
        .inputs(BuildInputs {
            globals: globals.clone(),
            root: root(),
            out: flag::path("out")
                .alias("o")
                .path_checks(PathChecks::file().allow_missing())
                .config("manifest")
                .default("manifest.json".into())
                .describe("Manifest file to write"),
            force: flag::boolean("force").describe("Overwrite an existing manifest"),
        })
        .tap(|_, BuildValues { out, force, .. }, ()| {
            if out.exists() && !*force {
                return Err(CliError::validation(
                    "MANIFEST_EXISTS",
                    format!("{} already exists", out.display()),
                )
                .suggest("Pass --force to overwrite it"));
            }
            Ok(())
        })
        .inherit(&scanner)
        .derive_async(async |ctx, BuildValues { globals, root, .. }, files| {
            fingerprint_all(ctx, root, files, *globals.jobs).await
        })
        .action(|ctx, BuildValues { out, .. }, manifest| {
            std::fs::write(out, serde_json::to_vec_pretty(manifest)?)?;
            let bytes: u64 = manifest.files.values().map(|entry| entry.size).sum();
            if ctx.out.options().json {
                ctx.out.json(&json!({
                    "manifest": out,
                    "files": manifest.files.len(),
                    "bytes": bytes,
                }))
            } else {
                ctx.out.log(format!(
                    "{} files, {bytes} bytes → {}",
                    manifest.files.len(),
                    out.display()
                ));
                Ok(())
            }
        });
    let verify = Command::new("verify")
        .describe("Fingerprint a directory again and report drift against its manifest")
        .example(
            "{bin} . --json",
            "Compare the current directory with manifest.json",
        )
        .inputs(VerifyInputs {
            globals: globals.clone(),
            root: root(),
            manifest: flag::path("manifest")
                .alias("m")
                .path_checks(PathChecks::file())
                .config("manifest")
                .default("manifest.json".into())
                .describe("Manifest to compare against"),
        })
        .inherit(&scanner)
        .derive_async(
            async |ctx,
                   VerifyValues {
                       globals,
                       root,
                       manifest,
                   },
                   files| {
                let recorded: Manifest = serde_json::from_slice(&std::fs::read(manifest)?)?;
                let current = fingerprint_all(ctx, root, files, *globals.jobs).await?;
                Ok(recorded.compare(current))
            },
        )
        .middleware(|ctx, VerifyValues { manifest, .. }, report, next| {
            ctx.out.status(format!(
                "{}: {} changed, {} missing, {} added, {} unchanged",
                manifest.display(),
                report.changed.len(),
                report.missing.len(),
                report.added.len(),
                report.unchanged
            ));
            next.run(report)
        })
        .action(|ctx, _, report| {
            if ctx.out.options().json {
                ctx.out.json(report)?;
            } else if report.drift() {
                ctx.out.table(
                    &report.rows(),
                    &[Column::new("path", "Path"), Column::new("status", "Status")],
                    TableFormat::Pretty,
                )?;
            } else {
                ctx.out
                    .log(format!("{} files match the manifest", report.unchanged));
            }
            if report.drift() {
                ctx.out.set_exit_code(1);
            }
            Ok(())
        });
    Cli::new("manifest")
        .version(env!("CARGO_PKG_VERSION"))
        .describe("Fingerprint directories and detect drift")
        .inputs(globals)
        .config(ConfigSettings::new("manifest"))
        .command(build)
        .command(verify)
        .completions()
}

fn scan(ctx: Scope<'_>, root: &Path, ignore: &[String]) -> fujo::Result<Vec<PathBuf>> {
    let spinner = ctx.out.spinner("Scanning files");
    let mut files = vec![];
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            if ignore.iter().any(|skip| name == skip.as_str()) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    spinner.succeed(format!("Found {} files", files.len()));
    Ok(files)
}

struct Timed;
impl<I: InputSet> AsyncMiddleware<I, Vec<PathBuf>> for Timed {
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        _: I::Values<'a>,
        files: Vec<PathBuf>,
        next: AsyncNext<'a, Vec<PathBuf>>,
    ) -> LocalFuture<'a, fujo::Result<()>> {
        Box::pin(async move {
            let started = Instant::now();
            let count = files.len();
            let result = next.run(files).await;
            ctx.out.status(format!(
                "Processed {count} files in {:.2?}",
                started.elapsed()
            ));
            result
        })
    }
}

async fn fingerprint_all(
    ctx: Scope<'_>,
    root: &Path,
    files: Vec<PathBuf>,
    jobs: NonZeroUsize,
) -> fujo::Result<Manifest> {
    let mut progress = ctx.out.progress("Fingerprinting", files.len() as u64);
    let permits = Arc::new(Semaphore::new(jobs.get()));
    let mut tasks = JoinSet::new();
    for path in files {
        let permits = permits.clone();
        tasks.spawn(async move {
            let _permit = permits
                .acquire_owned()
                .await
                .map_err(|error| CliError::new("FINGERPRINT_ABORTED", error.to_string()))?;
            tokio::task::spawn_blocking(move || fingerprint(&path).map(|entry| (path, entry)))
                .await
                .map_err(|error| CliError::new("FINGERPRINT_PANICKED", error.to_string()))?
        });
    }
    let mut manifest = Manifest::default();
    while let Some(joined) = tasks.join_next().await {
        let (path, entry) =
            joined.map_err(|error| CliError::new("FINGERPRINT_PANICKED", error.to_string()))??;
        manifest.files.insert(relative(root, &path)?, entry);
        progress.increment(1);
    }
    progress.done(format!("Fingerprinted {} files", manifest.files.len()));
    Ok(manifest)
}

fn relative(root: &Path, path: &Path) -> fujo::Result<String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        CliError::new(
            "OUTSIDE_ROOT",
            format!("{} is outside {}", path.display(), root.display()),
        )
    })?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn fingerprint(path: &Path) -> fujo::Result<Entry> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        for byte in &buffer[..read] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    Ok(Entry {
        size,
        digest: format!("{hash:016x}"),
    })
}

#[derive(Default, Serialize, Deserialize)]
pub struct Manifest {
    files: BTreeMap<String, Entry>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    size: u64,
    digest: String,
}
impl Manifest {
    fn compare(self, current: Manifest) -> Report {
        let mut report = Report::default();
        for (path, entry) in &self.files {
            match current.files.get(path) {
                Some(found) if found == entry => report.unchanged += 1,
                Some(_) => report.changed.push(path.clone()),
                None => report.missing.push(path.clone()),
            }
        }
        report.added = current
            .files
            .into_keys()
            .filter(|path| !self.files.contains_key(path))
            .collect();
        report
    }
}

#[derive(Default, Serialize)]
pub struct Report {
    changed: Vec<String>,
    missing: Vec<String>,
    added: Vec<String>,
    unchanged: usize,
}
impl Report {
    fn drift(&self) -> bool {
        !(self.changed.is_empty() && self.missing.is_empty() && self.added.is_empty())
    }
    fn rows(&self) -> Vec<fujo::Value> {
        let rows = |paths: &[String], status: &str| {
            paths
                .iter()
                .map(|path| json!({"path": path, "status": status}))
                .collect::<Vec<_>>()
        };
        [
            rows(&self.changed, "changed"),
            rows(&self.missing, "missing"),
            rows(&self.added, "added"),
        ]
        .concat()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    app().run_async().await
}
