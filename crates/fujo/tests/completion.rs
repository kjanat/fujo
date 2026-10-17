#![cfg(feature = "completion")]
use fujo::cli;
use fujo::command;
use fujo::completion::Shell;
use fujo::completion::generate;
use fujo::flag;
use fujo::group;
#[cfg(feature = "shell-tests")]
use std::io::Write;
#[cfg(feature = "shell-tests")]
use std::process::Command;
#[cfg(feature = "shell-tests")]
use std::process::Stdio;

fn app() -> fujo::Cli {
    cli("demo")
        .version("1.0.0")
        .flag(flag::boolean("global"))
        .command(
            group("db")
                .flag(flag::enumeration("region", ["us", "eu"]).propagate())
                .command(
                    command("seed")
                        .alias("s")
                        .flag(flag::string("label"))
                        .flag(flag::boolean("hidden").hidden()),
                ),
        )
        .completions()
}
#[cfg(feature = "shell-tests")]
fn run(shell: &str, args: &[&str], script: &str) -> String {
    let mut child = Command::new(shell)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{shell} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
#[cfg(feature = "shell-tests")]
fn run_powershell(script: &str) -> String {
    // `-Command -` enters PowerShell's stdin reader, which can emit terminal
    // control sequences even with redirected streams and `-NonInteractive`.
    run(
        "pwsh",
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ],
        "",
    )
}
#[test]
#[cfg(feature = "shell-tests")]
fn bash_script_parses_and_completes_flags_and_values() {
    let script = generate(app().schema(), Shell::Bash);
    run("bash", &["-n"], &script);
    let script = format!(
        "{script}\nCOMP_WORDS=(demo db seed --region e); COMP_CWORD=4; _demo_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"\n"
    );
    assert_eq!(run("bash", &[], &script), "eu\n");
}
#[test]
#[cfg(feature = "shell-tests")]
fn bash_completion_understands_aliases_inline_values_and_flag_operands() {
    let script = generate(app().schema(), Shell::Bash);
    let probe = format!(
        "{script}\nCOMP_WORDBREAKS=' '; COMP_WORDS=(demo db s --label seed --region=e); COMP_CWORD=5; _demo_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"\n"
    );
    assert_eq!(run("bash", &[], &probe), "--region=eu\n");
}
#[test]
#[cfg(feature = "shell-tests")]
fn bash_completion_reassembles_words_readline_split_at_equals() {
    let script = generate(app().schema(), Shell::Bash);
    let words = "COMP_WORDS=(demo db s --label seed --region = e); COMP_CWORD=7; COMP_LINE='demo db s --label seed --region=e'; COMP_POINT=${#COMP_LINE}";
    let probe =
        format!("{script}\n{words}; _demo_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"\n");
    assert_eq!(run("bash", &[], &probe), "eu\n");
    let probe = format!(
        "{script}\nCOMP_WORDS=(demo db s --label seed --region = ''); COMP_CWORD=7; _demo_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"\n"
    );
    assert_eq!(run("bash", &[], &probe), "us\neu\n");
    let library = "/usr/share/bash-completion/bash_completion";
    if std::path::Path::new(library).exists() {
        let probe = format!(
            "source {library}\n{script}\n{words}; _demo_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"\n"
        );
        assert_eq!(run("bash", &[], &probe), "eu\n");
    }
}
#[test]
#[cfg(feature = "shell-tests")]
fn bash_completion_inherits_root_flags_and_hides_private_options() {
    let script = generate(app().schema(), Shell::Bash);
    let probe = format!(
        "{script}\nCOMP_WORDS=(demo db seed --); COMP_CWORD=3; _demo_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"\n"
    );
    let output = run("bash", &[], &probe);
    assert!(output.contains("--region\n"));
    assert!(output.contains("--global\n"));
    assert!(!output.contains("--hidden"));
}
#[test]
#[cfg(feature = "shell-tests")]
fn zsh_script_parses_and_selects_enum_candidates() {
    let script = generate(app().schema(), Shell::Zsh);
    run("zsh", &["-n"], &script);
    let probe = format!(
        "compdef() {{ :; }}\ncompadd() {{ while [[ $1 != -- && $# -gt 0 ]]; do shift; done; shift; printf '%s\\n' \"$@\"; }}\n{script}\nwords=(demo db seed --region e); CURRENT=5; _demo_complete\n"
    );
    assert_eq!(run("zsh", &["-f"], &probe), "us\neu\n");
}
#[test]
#[cfg(feature = "shell-tests")]
fn fish_script_parses_and_completes_nested_enum_values() {
    let script = generate(app().schema(), Shell::Fish);
    run("fish", &["-n"], &script);
    let probe = format!("{script}\ncomplete -C 'demo db seed --region e'\n");
    assert!(
        run("fish", &[], &probe)
            .lines()
            .any(|line| line == "eu" || line.starts_with("eu\t"))
    );
}
#[test]
#[cfg(feature = "shell-tests")]
fn powershell_script_parses_and_completes_nested_enum_values() {
    let script = generate(app().schema(), Shell::PowerShell);
    let probe = format!(
        "{script}\n[System.Management.Automation.CommandCompletion]::CompleteInput('demo db seed --region e',23,$null).CompletionMatches | ForEach-Object {{ $_.CompletionText }}\n"
    );
    assert_eq!(run_powershell(&probe).trim(), "eu");
}
#[test]
#[cfg(feature = "shell-tests")]
fn fish_offers_enum_values_containing_spaces_as_single_candidates() {
    let program =
        cli("demo").command(command("send").flag(flag::enumeration("value", ["a b", "c'd", "e"])));
    let script = generate(program.schema(), Shell::Fish);
    let output = run(
        "fish",
        &[],
        &format!("{script}\ncomplete -C 'demo send --value '\n"),
    );
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines, ["a b", "c'd", "e"], "{output}");
}
#[test]
#[cfg(feature = "shell-tests")]
fn powershell_offers_nothing_after_a_free_text_flag() {
    let script = generate(app().schema(), Shell::PowerShell);
    let probe = format!(
        "{script}\n$text='demo db seed --label '; [System.Management.Automation.CommandCompletion]::CompleteInput($text,$text.Length,$null).CompletionMatches | ForEach-Object {{ $_.CompletionText }}\n"
    );
    let output = run_powershell(&probe);
    assert!(
        !output.contains("--") && !output.contains("seed"),
        "{output}"
    );
    let probe = format!(
        "{script}\n$text='demo db seed --label x --'; [System.Management.Automation.CommandCompletion]::CompleteInput($text,$text.Length,$null).CompletionMatches | ForEach-Object {{ $_.CompletionText }}\n"
    );
    assert!(run_powershell(&probe).contains("--region"));
}
#[test]
#[cfg(feature = "shell-tests")]
fn powershell_supports_inline_enum_values() {
    let script = generate(app().schema(), Shell::PowerShell);
    let probe = format!(
        "{script}\n$text='demo db seed --region=e'; [System.Management.Automation.CommandCompletion]::CompleteInput($text,$text.Length,$null).CompletionMatches | ForEach-Object {{ $_.CompletionText }}\n"
    );
    assert_eq!(run_powershell(&probe).trim(), "--region=eu");
}
#[test]
fn cli_completion_command_uses_the_same_generator() {
    let app = app();
    assert_eq!(
        app.execute(["completions", "bash"], Default::default())
            .stdout,
        generate(app.schema(), Shell::Bash)
    );
    assert_eq!(
        app.execute(["--completions=zsh"], Default::default())
            .stdout,
        generate(app.schema(), Shell::Zsh)
    );
}
#[test]
fn shell_names_are_explicit_and_validated() {
    assert_eq!("pwsh".parse::<Shell>().unwrap(), Shell::PowerShell);
    assert!("unknown".parse::<Shell>().is_err());
}
#[test]
#[cfg(feature = "shell-tests")]
fn generated_scripts_quote_schema_strings_as_data() {
    let program = cli("demo-tool").command(
        command("send").flag(
            flag::enumeration("value", ["a'b", "a b", "$(echo unsafe)"])
                .describe("Quoted 'description'"),
        ),
    );
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let script = generate(program.schema(), shell);
        run(
            match shell {
                Shell::Bash => "bash",
                Shell::Zsh => "zsh",
                _ => "fish",
            },
            &["-n"],
            &script,
        );
    }
}

#[test]
#[cfg(feature = "shell-tests")]
fn fish_does_not_treat_a_flag_operand_as_a_subcommand() {
    let app = cli("demo")
        .flag(flag::string("label"))
        .command(group("db").command(command("seed")));
    let script = generate(app.schema(), Shell::Fish);
    let output = run(
        "fish",
        &[],
        &format!("{script}\ncomplete -C 'demo --label db s'\n"),
    );
    assert!(
        !output
            .lines()
            .any(|line| line == "seed" || line.starts_with("seed\t"))
    );
}
