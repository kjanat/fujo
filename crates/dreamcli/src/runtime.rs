use crate::CliError;
use crate::Output;
use crate::Result;
use crate::config::ConfigReader;
use crate::output::ActivityEvent;
use crate::output::OutputOptions;
use crate::parse::ParseOptions;
use crate::resolve::FileSystem;
use crate::resolve::ResolveOptions;
use crate::resolve::Resolved;
use crate::schema::PathType;
use std::io::IsTerminal;
use std::io::Read;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct RunOptions {
    pub resolve: ResolveOptions,
    pub output: OutputOptions,
    pub parse: ParseOptions,
    pub help: crate::help::HelpOptions,
    pub channel: Option<Output>,
}
#[derive(Debug, Clone)]
pub struct RunResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<CliError>,
    pub resolved: Option<Resolved>,
    pub activities: Vec<ActivityEvent>,
    pub skipped_checks: Vec<crate::resolve::SkippedCheck>,
}
impl RunResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0 && self.error.is_none()
    }
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.exit_code.clamp(0, 255) as u8)
    }
    pub fn into_result(self) -> Result<Self> {
        if let Some(error) = self.error.clone() {
            Err(error)
        } else {
            Ok(self)
        }
    }
}
/// OS boundary. Tests can implement this without modifying environment or stdio.
pub trait Runtime: Send + Sync {
    fn env(&self, name: &str) -> Option<String>;
    fn stdin_is_tty(&self) -> bool;
    fn stdout_is_tty(&self) -> bool;
    fn read_stdin(&self) -> Result<String>;
    fn cwd(&self) -> Result<PathBuf>;
    fn reader(&self) -> &dyn ConfigReader;
    fn filesystem(&self) -> Arc<dyn FileSystem>;
    fn output(&self, options: OutputOptions) -> Output;
    fn config_directories(&self) -> crate::config::ConfigDirectories {
        let platform = if cfg!(target_os = "windows") {
            crate::config::Platform::Windows
        } else if cfg!(target_os = "macos") {
            crate::config::Platform::MacOs
        } else if cfg!(target_os = "linux") {
            crate::config::Platform::Linux
        } else {
            crate::config::Platform::Other
        };
        crate::config::config_directories(platform, |name| self.env(name))
    }
}
#[derive(Debug, Default)]
pub struct NativeRuntime;
impl FileSystem for NativeRuntime {
    fn stat(&self, path: &str) -> Result<Option<PathType>> {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => Ok(Some(PathType::File)),
            Ok(meta) if meta.is_dir() => Ok(Some(PathType::Directory)),
            Ok(_) => Err(CliError::new(
                "UNSUPPORTED_PATH_TYPE",
                "Path is neither a file nor a directory",
            )),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn mkdir(&self, path: &str) -> Result<()> {
        std::fs::create_dir_all(path).map_err(Into::into)
    }
}
impl ConfigReader for NativeRuntime {
    fn read(&self, path: &Path) -> Result<Option<String>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
impl Runtime for NativeRuntime {
    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn stdin_is_tty(&self) -> bool {
        io::stdin().is_terminal()
    }
    fn stdout_is_tty(&self) -> bool {
        io::stdout().is_terminal()
    }
    fn read_stdin(&self) -> Result<String> {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        Ok(input)
    }
    fn cwd(&self) -> Result<PathBuf> {
        std::env::current_dir().map_err(Into::into)
    }
    fn reader(&self) -> &dyn ConfigReader {
        self
    }
    fn filesystem(&self) -> Arc<dyn FileSystem> {
        Arc::new(Self)
    }
    fn output(&self, options: OutputOptions) -> Output {
        Output::with_writers(io::stdout(), io::stderr(), options)
    }
}
