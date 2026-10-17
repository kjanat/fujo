use fujo::Builtins;
use fujo::RunOptions;
use fujo::RunResult;
use fujo::cli;
use fujo::command;
use fujo::config::ConfigReader;
use fujo::output::Output;
use fujo::output::OutputOptions;
use fujo::resolve::FileSystem;
use fujo::runtime::NativeRuntime;
use fujo::runtime::Runtime;
use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

struct Host {
    stdout_tty: bool,
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
        Output::capture(options)
    }
}
fn result(exit_code: i32) -> RunResult {
    RunResult {
        exit_code,
        stdout: String::new(),
        stderr: String::new(),
        error: None,
        resolved: None,
        activities: vec![],
        skipped_checks: vec![],
    }
}

#[test]
fn stderr_tty_defaults_to_stdout_and_is_native_for_the_process() {
    assert!(Host { stdout_tty: true }.stderr_is_tty());
    assert!(!Host { stdout_tty: false }.stderr_is_tty());
    assert_eq!(
        NativeRuntime.stderr_is_tty(),
        std::io::stderr().is_terminal()
    );
}

#[test]
fn process_exit_codes_outside_u8_become_failure() {
    assert_eq!(result(0).exit_code(), ExitCode::from(0));
    assert_eq!(result(2).exit_code(), ExitCode::from(2));
    assert_eq!(result(255).exit_code(), ExitCode::from(255));
    assert_eq!(result(-1).exit_code(), ExitCode::from(1));
    assert_eq!(result(256).exit_code(), ExitCode::from(1));
    assert_eq!(result(i32::MIN).exit_code(), ExitCode::from(1));
}

#[test]
fn into_result_moves_the_error_out() {
    let failed = RunResult {
        error: Some(fujo::CliError::new("FAILED", "failed")),
        ..result(1)
    };
    assert_eq!(failed.into_result().unwrap_err().code, "FAILED");
    assert_eq!(result(0).into_result().unwrap().exit_code, 0);
}

#[test]
fn run_options_debug_lists_the_printable_fields() {
    let text = format!("{:?}", RunOptions::default());
    assert!(
        text.starts_with("RunOptions { output: OutputOptions"),
        "{text}"
    );
    assert!(text.contains("parse: ParseOptions"), "{text}");
    assert!(text.contains("help: HelpOptions"), "{text}");
    assert!(text.ends_with("channel: None, .. }"), "{text}");
}

#[test]
fn global_flags_follow_the_enabled_builtins() {
    let longs = |flags: Vec<fujo_core::command::GlobalFlag>| {
        flags.into_iter().map(|f| f.long).collect::<Vec<_>>()
    };
    assert_eq!(
        longs(Builtins::default().global_flags(true, true, false)),
        ["--help", "--version", "--json", "--quiet", "--completions"]
    );
    assert_eq!(
        longs(
            Builtins {
                help: false,
                json: false,
                quiet: false,
                explain: true,
            }
            .global_flags(false, false, false)
        ),
        ["--explain"]
    );
    let shorts: Vec<_> = Builtins::default()
        .global_flags(true, true, false)
        .into_iter()
        .filter_map(|f| f.short)
        .collect();
    assert_eq!(shorts, ["-h", "-V", "-q"]);
    let help = cli("demo")
        .version("1.0.0")
        .command(command("run"))
        .execute(["--help"], RunOptions::default())
        .stdout;
    assert!(
        help.ends_with(
            "\nGlobal options:\n  -h, --help     Show help\n  -V, --version  Show version\n  --json         Write structured JSON\n  -q, --quiet    Suppress status and informational output\n"
        ),
        "{help}"
    );
}

#[cfg(feature = "completion")]
mod completion {
    use super::*;
    use fujo::completion::Shell;
    use fujo::completion::generate;
    use fujo::flag;
    use fujo::group;
    use fujo::help;

    #[test]
    fn help_lists_the_completions_flag_with_its_value_hint() {
        let help = app().execute(["--help"], RunOptions::default()).stdout;
        assert!(
            help.ends_with(
                "  -q, --quiet            Suppress status and informational output\n  --completions <shell>  Generate shell completions\n"
            ),
            "{help}"
        );
    }

    fn app() -> fujo::Cli {
        cli("demo")
            .version("1.0.0")
            .command(
                group("db")
                    .flag(flag::enumeration("region", ["us", "eu"]).propagate())
                    .command(command("seed").flag(flag::string("label"))),
            )
            .completions()
    }

    #[test]
    fn shell_from_path_ignores_executable_extensions() {
        assert_eq!(Shell::from_path("pwsh.exe"), Some(Shell::PowerShell));
        assert_eq!(Shell::from_path("/usr/bin/bash.exe"), Some(Shell::Bash));
        assert_eq!(Shell::from_path("/usr/local/bin/fish"), Some(Shell::Fish));
        assert_eq!(Shell::from_path("/bin/sh"), None);
        assert_eq!(Shell::from_path(""), None);
        assert_eq!(help::shell_from_path("/bin/zsh.exe"), Some(Shell::Zsh));
    }

    #[test]
    fn completion_candidates_include_short_global_spellings() {
        let bash = generate(app().schema(), Shell::Bash);
        assert!(
            bash.contains(
                "'' ) candidates=('db' 'completions' '--help' '-h' '--version' '-V' '--json' '--quiet' '-q' '--completions') ;;"
            ) || bash.contains(
                "'') candidates=('db' 'completions' '--help' '-h' '--version' '-V' '--json' '--quiet' '-q' '--completions') ;;"
            ),
            "{bash}"
        );
        let fish = generate(app().schema(), Shell::Fish);
        assert!(
            fish.contains("complete -c 'demo' -l 'help' -s 'h'\n"),
            "{fish}"
        );
        assert!(fish.contains("complete -c 'demo' -l 'json'\n"), "{fish}");
        assert!(
            fish.contains("complete -c 'demo' -l 'completions' -r -f -a "),
            "{fish}"
        );
        let powershell = generate(app().schema(), Shell::PowerShell);
        assert!(
            powershell.contains("'--quiet', '-q', '--completions')"),
            "{powershell}"
        );
    }

    #[test]
    fn fish_quotes_each_enum_value_inside_the_argument_list() {
        let program = cli("demo")
            .command(command("send").flag(flag::enumeration("value", ["a b", "c'd", "e\\f"])));
        let fish = generate(program.schema(), Shell::Fish);
        let expected = r#" -f -a '\'a b\' \'c\\\'d\' \'e\\\\f\''"#;
        assert!(fish.contains(&format!("{expected}\n")), "{fish}");
    }

    #[test]
    fn powershell_clears_candidates_after_free_text_flags() {
        let powershell = generate(app().schema(), Shell::PowerShell);
        assert!(
            powershell.contains("    'db seed:--label' { $candidates = @() }\n"),
            "{powershell}"
        );
        assert!(
            powershell.contains("    'db seed:--region' { $candidates = @('us', 'eu') }\n"),
            "{powershell}"
        );
    }

    #[test]
    fn value_taking_globals_skip_their_operand_and_complete_their_value() {
        let program = app().config(fujo::config::ConfigSettings::new("demo"));
        let bash = generate(program.schema(), Shell::Bash);
        assert!(
            bash.contains("      'db seed:--config') skip=1; continue ;;\n"),
            "{bash}"
        );
        assert!(
            bash.contains(
                "    ':--completions')\n      candidates=('bash' 'zsh' 'fish' 'powershell')\n"
            ),
            "{bash}"
        );
        assert!(
            bash.contains("    'db:--config')\n      mapfile -t COMPREPLY"),
            "{bash}"
        );
        let zsh = generate(program.schema(), Shell::Zsh);
        assert!(
            zsh.contains("    'db:--config')\n      _files; return ;;\n"),
            "{zsh}"
        );
        let fish = generate(program.schema(), Shell::Fish);
        assert!(
            fish.contains("      case 'db:--config'\n        set skip 1\n        continue\n"),
            "{fish}"
        );
        assert!(
            fish.contains(
                "complete -c 'demo' -l 'completions' -r -f -a '\\'bash\\' \\'zsh\\' \\'fish\\' \\'powershell\\''\n"
            ),
            "{fish}"
        );
        assert!(
            fish.contains("complete -c 'demo' -l 'config' -r\n"),
            "{fish}"
        );
        let powershell = generate(program.schema(), Shell::PowerShell);
        assert!(
            powershell.contains("      'db:--config' { $skip = $true }\n"),
            "{powershell}"
        );
        assert!(
            powershell.contains(
                "    ':--completions' { $candidates = @('bash', 'zsh', 'fish', 'powershell') }\n"
            ),
            "{powershell}"
        );
    }

    #[test]
    fn one_character_negated_names_render_as_short_flags() {
        let program = cli("demo")
            .command(command("paint").flag(flag::boolean("color").alias("c").negated_name("N")));
        let help = program
            .execute(["paint", "--help"], RunOptions::default())
            .stdout;
        assert!(help.contains("  -c, --color, -N"), "{help}");
        let bash = generate(program.schema(), Shell::Bash);
        assert!(bash.contains(" '-N'"), "{bash}");
        assert!(!bash.contains("'--N'"), "{bash}");
        let fish = generate(program.schema(), Shell::Fish);
        assert!(fish.contains(" -s 'N' "), "{fish}");
        let error = program
            .execute(["paint", "--M"], RunOptions::default())
            .error
            .unwrap();
        assert_eq!(error.code, "UNKNOWN_FLAG");
        assert_eq!(error.suggest, None);
    }

    #[test]
    fn bash_reassembles_readline_words_before_matching() {
        let bash = generate(app().schema(), Shell::Bash);
        assert!(bash.contains("_comp_get_words -n =: cur prev words cword"));
        assert!(bash.contains("[[ $COMP_WORDBREAKS == *=* ]] && prefix=''"));
        let zsh = generate(app().schema(), Shell::Zsh);
        assert!(!zsh.contains("COMP_WORDBREAKS"));
    }
}
