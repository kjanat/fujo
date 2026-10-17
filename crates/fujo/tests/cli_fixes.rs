use fujo::RunOptions;
use fujo::cli;
use fujo::command;
use fujo::config::ConfigReader;
use fujo::output::Output;
use fujo::output::OutputOptions;
use fujo::resolve::FileSystem;
use fujo::runtime::NativeRuntime;
use fujo::runtime::Runtime;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

struct Host {
    stdout_tty: bool,
    stderr_tty: bool,
    seen: Mutex<Option<OutputOptions>>,
}
impl Host {
    fn new(stdout_tty: bool, stderr_tty: bool) -> Self {
        Self {
            stdout_tty,
            stderr_tty,
            seen: Mutex::new(None),
        }
    }
    fn options(&self) -> OutputOptions {
        self.seen.lock().unwrap().clone().unwrap()
    }
}
impl ConfigReader for Host {
    fn read(&self, _: &Path) -> fujo::Result<Option<String>> {
        Ok(None)
    }
}
impl Runtime for Host {
    fn env(&self, _: &str) -> Option<String> {
        None
    }
    fn stdin_is_tty(&self) -> bool {
        false
    }
    fn stdout_is_tty(&self) -> bool {
        self.stdout_tty
    }
    fn stderr_is_tty(&self) -> bool {
        self.stderr_tty
    }
    fn read_stdin(&self) -> fujo::Result<String> {
        Ok(String::new())
    }
    fn cwd(&self) -> fujo::Result<PathBuf> {
        Ok(PathBuf::from("/"))
    }
    fn reader(&self) -> &dyn ConfigReader {
        self
    }
    fn filesystem(&self) -> Arc<dyn FileSystem> {
        Arc::new(NativeRuntime)
    }
    fn output(&self, options: OutputOptions) -> Output {
        *self.seen.lock().unwrap() = Some(options.clone());
        Output::capture(options)
    }
}

#[test]
fn activity_follows_the_stderr_terminal_and_color_follows_stdout() {
    let app = cli("demo").default(command("run").action(|_| Ok(())));
    let host = Host::new(true, false);
    assert!(
        app.run_with(Vec::<String>::new(), RunOptions::default(), &host)
            .success()
    );
    let options = host.options();
    assert!(!options.tty);
    assert!(options.color);
    let host = Host::new(false, true);
    assert!(
        app.run_with(Vec::<String>::new(), RunOptions::default(), &host)
            .success()
    );
    let options = host.options();
    assert!(options.tty);
    assert!(!options.color);
}

#[test]
fn the_config_flag_appears_in_help_and_completions_only_when_configured() {
    let plain = cli("demo").default(command("run").action(|_| Ok(())));
    let help = plain.execute(["--help"], RunOptions::default()).stdout;
    assert!(!help.contains("--config"), "{help}");
    let configured = cli("demo")
        .config(fujo::config::ConfigSettings::new("demo"))
        .default(command("run").action(|_| Ok(())));
    let help = configured.execute(["--help"], RunOptions::default()).stdout;
    assert!(
        help.contains("--config <path>  Read configuration from this file\n"),
        "{help}"
    );
    #[cfg(feature = "completion")]
    {
        use fujo::completion::Shell;
        use fujo::completion::generate;
        assert!(!generate(plain.schema(), Shell::Bash).contains("--config"));
        assert!(generate(configured.schema(), Shell::Bash).contains("'--config'"));
        assert!(generate(configured.schema(), Shell::Fish).contains("-l 'config' -r\n"));
    }
}
