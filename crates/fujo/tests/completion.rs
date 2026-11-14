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
fn files_app() -> fujo::Cli {
    cli("demo")
        .config(fujo::config::ConfigSettings::new("demo"))
        .command(
            command("pack")
                .flag(flag::path("out"))
                .flag(flag::string("label"))
                .flag(flag::boolean("publish"))
                .flag(flag::parse::<bool>("cache").negatable())
                .flag(flag::enumeration("speed", ["fast", "slow"]))
                .arg(fujo::arg::path("input"))
                .arg(fujo::arg::enumeration("mode", ["fast", "slow"]))
                .arg(fujo::arg::string("text")),
        )
}
#[cfg(feature = "shell-tests")]
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("input.txt"), "").unwrap();
    dir
}
#[cfg(feature = "shell-tests")]
fn run(shell: &str, args: &[&str], script: &str) -> String {
    run_in(std::path::Path::new("."), shell, args, script)
}
#[cfg(feature = "shell-tests")]
fn run_in(dir: &std::path::Path, shell: &str, args: &[&str], script: &str) -> String {
    let mut child = Command::new(shell)
        .current_dir(dir)
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
    run_powershell_in(std::path::Path::new("."), script)
}
#[cfg(feature = "shell-tests")]
fn run_powershell_in(dir: &std::path::Path, script: &str) -> String {
    // `-Command -` enters PowerShell's stdin reader, which can emit terminal
    // control sequences even with redirected streams and `-NonInteractive`.
    run_in(
        dir,
        "pwsh",
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("$ErrorActionPreference = 'Stop'\n{script}"),
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
fn powershell_offers_no_flags_or_commands_after_a_free_text_flag() {
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
#[cfg(feature = "shell-tests")]
fn bash_completes_files_only_for_path_values_and_positionals() {
    let dir = workspace();
    let script = generate(files_app().schema(), Shell::Bash);
    let probe = format!(
        "{script}\nt() {{ echo \"== $*\"; COMP_WORDS=(\"$@\"); COMP_CWORD=$(($#-1)); COMPREPLY=(); _demo_complete 2>/dev/null; printf '%s\\n' \"${{COMPREPLY[@]}}\"; }}\nt demo pack --label ''\nt demo pack --out in\nt demo pack in\nt demo pack input.txt ''\nt demo pack -- in\n"
    );
    let output = run_in(dir.path(), "bash", &[], &probe);
    let sections: Vec<&str> = output.split("== ").skip(1).collect();
    assert_eq!(sections[0], "demo pack --label \n\n", "{output}");
    assert_eq!(sections[1], "demo pack --out in\ninput.txt\n", "{output}");
    assert_eq!(sections[2], "demo pack in\ninput.txt\n", "{output}");
    let (header, mode) = sections[3].split_once('\n').unwrap();
    assert_eq!(header, "demo pack input.txt ", "{output}");
    assert!(mode.ends_with("fast\nslow\n"), "{output}");
    assert!(!mode.contains("input.txt"), "{output}");
    assert_eq!(sections[4], "demo pack -- in\ninput.txt\n", "{output}");
}
#[test]
#[cfg(feature = "shell-tests")]
fn zsh_completes_on_the_first_autoloaded_call_and_every_later_one() {
    let dir = workspace();
    let functions = dir.path().join("functions");
    std::fs::create_dir(&functions).unwrap();
    let script = generate(files_app().schema(), Shell::Zsh);
    std::fs::write(functions.join("_demo"), &script).unwrap();
    let stubs = "compadd() { local p=''; while [[ $1 != -- && $# -gt 0 ]]; do [[ $1 == -P ]] && { p=$2; shift; }; shift; done; shift; for w in \"$@\"; do print -r -- \"$p$w\"; done; }\ncompset() { print -r -- \"compset $*\"; }\n_files() { print -r -- FILES; }\ncompdef() { print -r -- \"compdef $*\"; }\n";
    let probe = format!(
        "{stubs}fpath=({} $fpath); autoload -Uz _demo\nprint '== out'; words=(demo pack --out=in); CURRENT=3; _demo\nprint '== mode'; words=(demo pack input.txt ''); CURRENT=4; _demo\nprint '== label'; words=(demo pack --label ''); CURRENT=4; _demo; print status=$?\nprint '== source'; source {}\n",
        functions.display(),
        functions.join("_demo").display()
    );
    let output = run_in(dir.path(), "zsh", &["-f"], &probe);
    let sections: Vec<&str> = output.split("== ").skip(1).collect();
    assert_eq!(sections[0], "out\ncompset -P --out=\nFILES\n", "{output}");
    assert!(sections[1].contains("\nfast\nslow\n"), "{output}");
    assert!(!sections[1].contains("FILES"), "{output}");
    assert_eq!(sections[2], "label\nstatus=1\n", "{output}");
    assert_eq!(
        sections[3], "source\ncompdef _demo_complete demo\n",
        "{output}"
    );
}
#[test]
#[cfg(feature = "shell-tests")]
fn fish_completes_files_only_for_path_values_and_positionals() {
    let dir = workspace();
    let script = generate(files_app().schema(), Shell::Fish);
    let probe = format!(
        "{script}\nfor line in 'demo pack --label ' 'demo pack --out in' 'demo pack in' 'demo pack input.txt ' 'demo pack -- in'\n  echo \"== $line\"\n  complete -C $line\nend\n"
    );
    let output = run_in(dir.path(), "fish", &["--no-config"], &probe);
    let sections: Vec<&str> = output.split("== ").skip(1).collect();
    assert_eq!(sections[0], "demo pack --label \n", "{output}");
    assert_eq!(sections[1], "demo pack --out in\ninput.txt\n", "{output}");
    assert_eq!(sections[2], "demo pack in\ninput.txt\n", "{output}");
    assert_eq!(
        sections[3], "demo pack input.txt \nfast\nslow\n",
        "{output}"
    );
    assert_eq!(sections[4], "demo pack -- in\ninput.txt\n", "{output}");
}

#[test]
#[cfg(feature = "shell-tests")]
fn fish_counts_positionals_after_boolean_and_inline_flags() {
    let dir = workspace();
    let script = generate(files_app().schema(), Shell::Fish);
    for line in [
        "demo pack --publish in",
        "demo pack --no-cache in",
        "demo pack --cache=true in",
        "demo pack --label=value in",
    ] {
        let output = run_in(
            dir.path(),
            "fish",
            &["--no-config"],
            &format!("{script}\ncomplete -C '{line}'\n"),
        );
        assert_eq!(output, "input.txt\n", "{line}: {output}");
    }
    for line in [
        "demo pack --no-cache input.txt f",
        "demo pack -- --label f",
        "demo pack - f",
    ] {
        let output = run_in(
            dir.path(),
            "fish",
            &["--no-config"],
            &format!("{script}\ncomplete -C '{line}'\n"),
        );
        assert_eq!(output, "fast\n", "{line}: {output}");
    }
}

#[test]
#[cfg(feature = "shell-tests")]
fn powershell_completes_inline_paths_and_preserves_quoted_filenames() {
    let dir = workspace();
    std::fs::write(dir.path().join("space name.txt"), "").unwrap();
    let script = generate(files_app().schema(), Shell::PowerShell);
    for (line, expected) in [
        ("demo pack --config=in", "--config=./input.txt"),
        ("demo pack --out=in", "--out=./input.txt"),
        ("demo pack --out=spa", "--out='./space name.txt'"),
        ("demo pack --out spa", "'./space name.txt'"),
    ] {
        let output = run_powershell_in(
            dir.path(),
            &format!(
                "{script}\n$t='{line}'; [System.Management.Automation.CommandCompletion]::CompleteInput($t,$t.Length,$null).CompletionMatches | ForEach-Object CompletionText\n"
            ),
        );
        // PowerShell uses the host platform's directory separator.
        assert_eq!(
            output.trim().replace('\\', "/"),
            expected,
            "{line}: {output}"
        );
    }
}

#[test]
#[cfg(feature = "shell-tests")]
fn powershell_does_not_offer_files_for_text_or_unmatched_choices() {
    let dir = workspace();
    let script = generate(files_app().schema(), Shell::PowerShell);
    for line in [
        "demo pack --label in",
        "demo pack --label=in",
        "demo pack --speed in",
        "demo pack --speed=in",
        "demo pack input.txt in",
        "demo pack input.txt fast in",
        "demo pack -- input.txt fast in",
    ] {
        let output = run_powershell_in(
            dir.path(),
            &format!(
                "{script}\n$t='{line}'; [System.Management.Automation.CommandCompletion]::CompleteInput($t,$t.Length,$null).CompletionMatches | ForEach-Object CompletionText\n"
            ),
        );
        let word = line.rsplit(' ').next().unwrap();
        assert!(
            output.trim().is_empty() || output.trim() == word,
            "{line}: {output}"
        );
    }
}

#[test]
#[cfg(feature = "shell-tests")]
fn powershell_suppresses_empty_results_when_the_runtime_supports_them() {
    let dir = workspace();
    let script = generate(files_app().schema(), Shell::PowerShell);
    let output = run_powershell_in(
        dir.path(),
        &format!(
            r#"
$probe = 'empty-result-test '
$options = @{{ NativeArgumentCompleters = @{{ 'empty-result-test' = {{ '' }} }} }}
try {{
    $supported = [System.Management.Automation.CommandCompletion]::CompleteInput($probe,$probe.Length,$options).CompletionMatches.Count -eq 0
}} catch {{ $supported = $false }}
$before = $Error.Count
{script}
if ($Error.Count -ne $before) {{ throw 'Loading completions left an error in the session' }}
$t = 'demo pack --label '
$matches = @([System.Management.Automation.CommandCompletion]::CompleteInput($t,$t.Length,$null).CompletionMatches | ForEach-Object CompletionText)
@{{ supported = $supported; matches = $matches }} | ConvertTo-Json -Compress
"#
        ),
    );
    let result: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    if result["supported"] == true {
        assert_eq!(result["matches"], serde_json::json!([]));
    } else {
        assert_eq!(
            result["matches"][0].as_str().unwrap().replace('\\', "/"),
            "./input.txt"
        );
    }
}

#[test]
#[cfg(feature = "shell-tests")]
fn powershell_completes_files_only_for_path_values() {
    let dir = workspace();
    let script = generate(files_app().schema(), Shell::PowerShell);
    let probe = format!(
        "{script}\nforeach ($t in 'demo pack --label ab', 'demo pack --out in', 'demo pack input.txt ') {{ '== ' + $t; [System.Management.Automation.CommandCompletion]::CompleteInput($t,$t.Length,$null).CompletionMatches | ForEach-Object {{ $_.CompletionText }} }}\n"
    );
    let output = run_powershell_in(dir.path(), &probe);
    let sections: Vec<&str> = output.split("== ").skip(1).collect();
    assert!(
        ["demo pack --label ab", "demo pack --label ab\nab"].contains(&sections[0].trim_end()),
        "{output}"
    );
    assert_eq!(
        sections[1].trim_end(),
        "demo pack --out in\n./input.txt",
        "{output}"
    );
    let (header, mode) = sections[2].split_once('\n').unwrap();
    assert_eq!(header, "demo pack input.txt ", "{output}");
    assert!(mode.trim_end().ends_with("fast\nslow"), "{output}");
    assert!(!mode.contains("input.txt"), "{output}");
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

#[cfg(feature = "shell-tests")]
fn review_app() -> fujo::Cli {
    use fujo::{arg, value};
    cli("demo")
        .command(
            command("hiddenarg")
                .arg(arg::string("secret").hidden())
                .arg(arg::enumeration("mode", ["fast", "slow"])),
        )
        .command(
            command("flags")
                .flag(flag::string("internal").hidden().alias("i"))
                .flag(flag::boolean("verbose").alias("v"))
                .flag(flag::path("out").alias("o").hidden_alias("x"))
                .flag(flag::enumeration("speed", ["fast", "slow"]).alias("s"))
                .flag(
                    flag::parse::<bool>("cache")
                        .negatable()
                        .alias("c")
                        .negated_name("N"),
                )
                .arg(arg::enumeration("mode", ["fast", "slow"]))
                .arg(arg::enumeration("next", ["foo", "bar"])),
        )
        .command(
            command("collections")
                .flag(flag::boolean("verbose").alias("v"))
                .flag(flag::key_value_of("dest", value::path()).alias("d"))
                .flag(flag::key_value_of("mode", value::enumeration(["fast", "slow"])).alias("m"))
                .flag(
                    flag::array("paths", value::path())
                        .separator(fujo::delimiter!("::"))
                        .alias("p"),
                )
                .flag(
                    flag::array("modes", value::enumeration(["fast", "slow"]))
                        .separator(fujo::delimiter!("::")),
                )
                .arg(
                    arg::enumeration("items", ["fast", "slow"])
                        .variadic()
                        .separator(fujo::delimiter!("::")),
                ),
        )
}

#[cfg(feature = "shell-tests")]
fn review_results(shell: Shell, cases: &[&[&str]]) -> Vec<Vec<String>> {
    let dir = workspace();
    let script = generate(review_app().schema(), shell);
    if shell == Shell::Zsh {
        let checker = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/check-completion-zsh.py");
        let output = run_in(
            dir.path(),
            "python3",
            &[checker.to_str().unwrap()],
            &serde_json::to_string(&(script, cases)).unwrap(),
        );
        return serde_json::from_str(&output).unwrap();
    }
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let mut probe = script;
    for words in cases {
        match shell {
            Shell::Bash => {
                let argv = words
                    .iter()
                    .map(|word| quote(word))
                    .collect::<Vec<_>>()
                    .join(" ");
                probe.push_str(&format!("\necho '==CASE'; COMP_WORDBREAKS=' '; COMP_WORDS=({argv}); COMP_CWORD={}; COMPREPLY=(); _demo_complete; printf '%s\\n' \"${{COMPREPLY[@]}}\"\n", words.len()-1));
            }
            Shell::Fish => {
                let line = words.join(" ");
                probe.push_str(&format!("\necho '==CASE'; complete -C {}\n", quote(&line)));
            }
            Shell::PowerShell => {
                let line = words.join(" ").replace('\'', "''");
                probe.push_str(&format!("\n'==CASE'; $t='{line}'; [System.Management.Automation.CommandCompletion]::CompleteInput($t,$t.Length,$null).CompletionMatches | ForEach-Object CompletionText\n"));
            }
            Shell::Zsh => unreachable!(),
        }
    }
    let output = match shell {
        Shell::Bash => run_in(dir.path(), "bash", &["--noprofile", "--norc"], &probe),
        Shell::Fish => run_in(dir.path(), "fish", &["--no-config"], &probe),
        Shell::PowerShell => run_powershell_in(dir.path(), &probe),
        Shell::Zsh => unreachable!(),
    };
    output
        .split("==CASE\n")
        .skip(1)
        .map(|section| {
            section
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| {
                    line.split('\t')
                        .next()
                        .unwrap()
                        .replace('\\', "/")
                        .replace("./", "")
                })
                .collect()
        })
        .collect()
}

#[cfg(feature = "shell-tests")]
fn assert_review_completions(cases: &[&[&str]], expected: &[&str]) {
    assert_eq!(cases.len(), expected.len());
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        let results = review_results(shell, cases);
        assert_eq!(results.len(), cases.len(), "{shell:?}: {results:?}");
        for ((case, actual), expected) in cases.iter().zip(results).zip(expected) {
            if expected.is_empty() {
                assert!(
                    actual.is_empty()
                        || (shell == Shell::PowerShell
                            && actual == [case.last().unwrap().to_string()]),
                    "{shell:?} {case:?}: {actual:?}"
                );
            } else {
                assert_eq!(actual, [*expected], "{shell:?} {case:?}");
            }
        }
    }
}

#[test]
#[cfg(feature = "shell-tests")]
fn hidden_inputs_participate_in_completion_state_without_being_suggested() {
    assert_review_completions(
        &[
            &["demo", "hiddenarg", "f"],
            &["demo", "hiddenarg", "secret", "f"],
            &["demo", "flags", "--internal", "value", "f"],
            &["demo", "flags", "-i", "value", "f"],
            &["demo", "flags", "-x", "input.txt", "f"],
            &["demo", "flags", "--internal", "f"],
            &["demo", "flags", "--int"],
            &["demo", "flags", "-x"],
        ],
        &["", "fast", "fast", "fast", "fast", "", "", ""],
    );
}

#[test]
#[cfg(feature = "shell-tests")]
fn short_clusters_consume_attached_and_separate_operands() {
    assert_review_completions(
        &[
            &["demo", "flags", "-vo", "input.txt", "f"],
            &["demo", "flags", "-voinput.txt", "f"],
            &["demo", "flags", "-vi", "value", "f"],
            &["demo", "flags", "-vovalue", "f"],
            &["demo", "flags", "-vs", "f"],
            &["demo", "flags", "-vsf"],
            &["demo", "flags", "-v", "f"],
            &["demo", "flags", "-vc", "true", "f"],
            &["demo", "flags", "-vN", "f"],
            &["demo", "flags", "--o", "input.txt", "f"],
            &["demo", "flags", "--", "-vo", "f"],
        ],
        &[
            "fast", "fast", "fast", "fast", "fast", "-vsfast", "fast", "fast", "fast", "fast",
            "foo",
        ],
    );
}

#[test]
#[cfg(feature = "shell-tests")]
fn collection_completions_preserve_keys_and_prior_elements() {
    assert_review_completions(
        &[
            &["demo", "collections", "--dest", "output=in"],
            &["demo", "collections", "--dest=output=in"],
            &["demo", "collections", "--mode", "output=f"],
            &["demo", "collections", "--mode=output=f"],
            &["demo", "collections", "--paths", "old::in"],
            &["demo", "collections", "--paths=old::in"],
            &["demo", "collections", "--modes", "slow::f"],
            &["demo", "collections", "slow::f"],
            &["demo", "collections", "--", "slow::f"],
            &["demo", "collections", "-vmoutput=f"],
            &["demo", "collections", "--mode", "output"],
        ],
        &[
            "output=input.txt",
            "--dest=output=input.txt",
            "output=fast",
            "--mode=output=fast",
            "old::input.txt",
            "--paths=old::input.txt",
            "slow::fast",
            "slow::fast",
            "slow::fast",
            "-vmoutput=fast",
            "",
        ],
    );
}

#[test]
#[cfg(feature = "shell-tests")]
fn collection_completion_handles_empty_elements_and_literal_delimiters() {
    for shell in [Shell::Bash, Shell::Fish, Shell::PowerShell] {
        let results = review_results(
            shell,
            &[
                &["demo", "collections", "--mode", "output="],
                &["demo", "collections", "--modes", "slow::"],
                &["demo", "collections", "--paths", "old::::in"],
            ],
        );
        assert_eq!(results[0], ["output=fast", "output=slow"], "{shell:?}");
        assert_eq!(results[1], ["slow::fast", "slow::slow"], "{shell:?}");
        assert_eq!(results[2], ["old::::input.txt"], "{shell:?}");
    }
}

#[test]
#[cfg(feature = "shell-tests")]
fn bash_collection_completion_respects_readline_word_breaks() {
    let dir = workspace();
    let script = generate(review_app().schema(), Shell::Bash);
    let probe = format!(
        r#"{script}
COMP_WORDBREAKS=' =:'
COMP_WORDS=(demo collections --dest output = in); COMP_CWORD=5
_demo_complete; printf '%s\n' "${{COMPREPLY[@]}}"
COMP_WORDS=(demo collections --mode = output = f); COMP_CWORD=6
_demo_complete; printf '%s\n' "${{COMPREPLY[@]}}"
COMP_WORDS=(demo collections --paths old : : in); COMP_CWORD=6
_demo_complete; printf '%s\n' "${{COMPREPLY[@]}}"
"#
    );
    let output = run_in(dir.path(), "bash", &["--noprofile", "--norc"], &probe);
    assert_eq!(output, "input.txt\nfast\ninput.txt\n");
}
