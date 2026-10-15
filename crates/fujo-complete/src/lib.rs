//! Static shell completions generated from the portable command schema.
use fujo_core::CliError;
use fujo_core::Result;
use fujo_core::command::CommandSchema;
use fujo_core::command::ProgramSchema;
use fujo_core::schema::InputSchema;
use std::fmt::Write;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}
impl FromStr for Shell {
    type Err = CliError;
    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            "fish" => Ok(Self::Fish),
            "powershell" | "pwsh" => Ok(Self::PowerShell),
            _ => Err(CliError::new(
                "INVALID_SHELL",
                "Use bash, zsh, fish, or powershell",
            )),
        }
    }
}
struct Node {
    path: String,
    flags: Vec<InputSchema>,
    children: Vec<(String, String)>,
}
fn nodes(program: &ProgramSchema) -> Vec<Node> {
    fn walk(
        commands: &[CommandSchema],
        parent: &str,
        inherited: &[InputSchema],
        nodes: &mut Vec<Node>,
    ) {
        for command in commands.iter().filter(|c| !c.hidden) {
            let path = if parent.is_empty() {
                command.name.clone()
            } else {
                format!("{parent} {}", command.name)
            };
            let mut flags: Vec<_> = inherited
                .iter()
                .filter(|f| !command.flags.iter().any(|local| local.name == f.name))
                .cloned()
                .collect();
            flags.extend(command.flags.clone());
            nodes.push(Node {
                path: path.clone(),
                flags: flags.clone(),
                children: children(&command.commands),
            });
            walk(
                &command.commands,
                &path,
                &flags
                    .into_iter()
                    .filter(|f| f.propagate)
                    .collect::<Vec<_>>(),
                nodes,
            );
        }
    }
    let mut flags = program.flags.clone();
    flags.extend(
        program
            .default
            .as_ref()
            .map(|c| c.flags.clone())
            .unwrap_or_default(),
    );
    let mut nodes = vec![Node {
        path: String::new(),
        flags: flags.clone(),
        children: children(&program.commands),
    }];
    if program.completions {
        nodes[0]
            .children
            .push(("completions".into(), "completions".into()));
    }
    let inherited: Vec<_> = program
        .flags
        .iter()
        .cloned()
        .map(|mut f| {
            f = f.propagated();
            f
        })
        .collect();
    walk(&program.commands, "", &inherited, &mut nodes);
    if program.completions {
        nodes.push(Node {
            path: "completions".into(),
            flags: vec![],
            children: ["bash", "zsh", "fish", "powershell"]
                .map(|s| (s.into(), s.into()))
                .to_vec(),
        });
    }
    nodes
}
fn children(commands: &[CommandSchema]) -> Vec<(String, String)> {
    commands
        .iter()
        .filter(|c| !c.hidden)
        .flat_map(|c| {
            std::iter::once(c.name.clone())
                .chain(c.aliases.clone())
                .map(|name| (name, c.name.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}
fn forms(
    input: &InputSchema,
    flags: &[InputSchema],
    options: fujo_core::parse::ParseOptions,
) -> Vec<String> {
    std::iter::once(format!(
        "--{}",
        fujo_core::parse::canonical_flag_name(input, flags, options)
    ))
    .chain(input.aliases.iter().filter(|a| !a.hidden).map(|a| {
        format!(
            "{}{}",
            if a.name.chars().count() == 1 {
                "-"
            } else {
                "--"
            },
            a.name
        )
    }))
    .chain(input.negated_name.as_ref().map(|n| format!("--{n}")))
    .collect()
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn identifier(s: &str) -> String {
    format!(
        "_{}_complete",
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    )
}

pub fn generate(program: &ProgramSchema, shell: Shell) -> String {
    generate_with_options(program, shell, Default::default())
}
pub fn generate_with_options(
    program: &ProgramSchema,
    shell: Shell,
    options: fujo_core::parse::ParseOptions,
) -> String {
    let nodes = nodes(program);
    let function = identifier(&program.name);
    let globals: Vec<&str> = [
        program.builtins.help.then_some("--help"),
        program.version.as_ref().map(|_| "--version"),
        program.builtins.json.then_some("--json"),
        program.builtins.quiet.then_some("--quiet"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let script = match shell {
        Shell::Bash | Shell::Zsh => {
            let bash = shell == Shell::Bash;
            let mut script = if bash {
                format!(
                    "{function}() {{\n  local cur=\"${{COMP_WORDS[COMP_CWORD]}}\" prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\" path='' token i skip=0\n  local -a words=(\"${{COMP_WORDS[@]}}\") candidates=()\n  local limit=$COMP_CWORD\n  for ((i=1; i<limit; i++)); do\n"
                )
            } else {
                format!(
                    "#compdef {}\n{function}() {{\n  local cur=\"$words[CURRENT]\" prev=\"$words[CURRENT-1]\" path='' token i skip=0\n  local -a candidates\n  for ((i=2; i<CURRENT; i++)); do\n",
                    program.name
                )
            };
            script.push_str("    token=\"${words[i]}\"\n    (( skip )) && { skip=0; continue; }\n    [[ $token == -- ]] && break\n    case \"$path:$token\" in\n");
            for node in &nodes {
                for flag in node.flags.iter().filter(|f| !f.hidden && f.expects_value()) {
                    for form in forms(flag, &node.flags, options) {
                        let _ = writeln!(
                            script,
                            "      {}) skip=1; continue ;;",
                            quote(&format!("{}:{form}", node.path))
                        );
                    }
                }
                for (name, canonical) in &node.children {
                    let path = if node.path.is_empty() {
                        canonical.clone()
                    } else {
                        format!("{} {canonical}", node.path)
                    };
                    let _ = writeln!(
                        script,
                        "      {}) path={} ;;",
                        quote(&format!("{}:{name}", node.path)),
                        quote(&path)
                    );
                }
            }
            script.push_str("    esac\n  done\n  local value_cur=\"$cur\" prefix='' value_flag=\"$prev\"\n  if [[ $cur == --*=* ]]; then value_flag=\"${cur%%=*}\"; value_cur=\"${cur#*=}\"; prefix=\"${cur%%=*}=\"; fi\n  case \"$path:$value_flag\" in\n");
            for node in &nodes {
                for flag in node.flags.iter().filter(|f| !f.hidden && f.expects_value()) {
                    let element = flag.element.as_deref().unwrap_or(flag);
                    for form in forms(flag, &node.flags, options) {
                        let _ =
                            writeln!(script, "    {})", quote(&format!("{}:{form}", node.path)));
                        if !element.enum_values.is_empty() {
                            let _ = writeln!(
                                script,
                                "      candidates=({})",
                                element
                                    .enum_values
                                    .iter()
                                    .map(|s| quote(s))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            );
                            if bash {
                                script.push_str("      COMPREPLY=(); for token in \"${candidates[@]}\"; do [[ $token == \"$value_cur\"* ]] && COMPREPLY+=(\"$prefix$token\"); done; return ;;\n");
                            } else {
                                script.push_str("      compadd -P \"$prefix\" -- \"${candidates[@]}\"; return ;;\n");
                            }
                        } else if bash {
                            script.push_str("      mapfile -t COMPREPLY < <(compgen -f -- \"$value_cur\"); if [[ -n $prefix ]]; then for i in \"${!COMPREPLY[@]}\"; do COMPREPLY[i]=\"$prefix${COMPREPLY[i]}\"; done; fi; return ;;\n");
                        } else {
                            script.push_str("      _files; return ;;\n");
                        }
                    }
                }
            }
            script.push_str("  esac\n  case \"$path\" in\n");
            for node in &nodes {
                let candidates = node
                    .children
                    .iter()
                    .map(|(n, _)| n.clone())
                    .chain(
                        node.flags
                            .iter()
                            .filter(|f| !f.hidden)
                            .flat_map(|flag| forms(flag, &node.flags, options)),
                    )
                    .chain(globals.iter().map(|s| s.to_string()))
                    .map(|s| quote(&s))
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = writeln!(
                    script,
                    "    {}) candidates=({candidates}) ;;",
                    quote(&node.path)
                );
            }
            script.push_str("  esac\n");
            if bash {
                script.push_str("  COMPREPLY=(); for token in \"${candidates[@]}\"; do [[ $token == \"$cur\"* ]] && COMPREPLY+=(\"$token\"); done\n}\n");
                let _ = writeln!(script, "complete -F {function} -- {}", quote(&program.name));
            } else {
                script.push_str("  compadd -- \"${candidates[@]}\"\n}\n");
                let _ = writeln!(script, "compdef {function} {}", quote(&program.name));
            }
            script
        }
        Shell::Fish => {
            let mut script = format!(
                "function {function}\n  set -l expected (string join ' ' -- $argv)\n  set -l tokens (commandline -opc)\n  set -e tokens[1]\n  set -l path ''\n  set -l skip 0\n  for token in $tokens\n    if test $skip -eq 1\n      set skip 0\n      continue\n    end\n    if test \"$token\" = --\n      break\n    end\n    switch \"$path:$token\"\n"
            );
            for node in &nodes {
                for flag in node.flags.iter().filter(|f| !f.hidden && f.expects_value()) {
                    for form in forms(flag, &node.flags, options) {
                        let _ = writeln!(
                            script,
                            "      case {}\n        set skip 1\n        continue",
                            quote(&format!("{}:{form}", node.path))
                        );
                    }
                }
                for (name, canonical) in &node.children {
                    let path = if node.path.is_empty() {
                        canonical.clone()
                    } else {
                        format!("{} {canonical}", node.path)
                    };
                    let _ = writeln!(
                        script,
                        "      case {}\n        set path {}",
                        quote(&format!("{}:{name}", node.path)),
                        quote(&path)
                    );
                }
            }
            script.push_str("    end\n  end\n  test \"$path\" = \"$expected\"\nend\n");
            for node in &nodes {
                let condition = if node.path.is_empty() {
                    function.clone()
                } else {
                    format!(
                        "{function} {}",
                        node.path
                            .split(' ')
                            .map(quote)
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                };
                for (name, _) in &node.children {
                    let _ = writeln!(
                        script,
                        "complete -c {} -f -n {} -a {}",
                        quote(&program.name),
                        quote(&condition),
                        quote(name)
                    );
                }
                for flag in node.flags.iter().filter(|f| !f.hidden) {
                    for form in forms(flag, &node.flags, options) {
                        let long = form.starts_with("--");
                        let _ = write!(
                            script,
                            "complete -c {} -n {} {} {} -d {}",
                            quote(&program.name),
                            quote(&condition),
                            if long { "-l" } else { "-s" },
                            quote(form.trim_start_matches('-')),
                            quote(&flag.description)
                        );
                        if flag.expects_value() {
                            script.push_str(" -r");
                        }
                        let element = flag.element.as_deref().unwrap_or(flag);
                        if !element.enum_values.is_empty() {
                            let _ =
                                write!(script, " -f -a {}", quote(&element.enum_values.join(" ")));
                        }
                        script.push('\n');
                    }
                }
            }
            for global in globals {
                let _ = writeln!(
                    script,
                    "complete -c {} -l {}",
                    quote(&program.name),
                    quote(global.trim_start_matches('-'))
                );
            }
            script
        }
        Shell::PowerShell => {
            let mut script = format!(
                "Register-ArgumentCompleter -Native -CommandName {} -ScriptBlock {{\n  param($wordToComplete, $commandAst, $cursorPosition)\n  $path = ''; $previous = ''; $skip = $false\n  $tokens = @($commandAst.CommandElements | Select-Object -Skip 1 | Where-Object {{ $_.Extent.EndOffset -lt $cursorPosition }} | ForEach-Object {{ $_.Extent.Text }})\n  foreach ($token in $tokens) {{\n    if ($skip) {{ $skip = $false; $previous = $token; continue }}\n    if ($token -eq '--') {{ break }}\n    switch (\"${{path}}:$token\") {{\n",
                ps_quote(&program.name)
            );
            for node in &nodes {
                for flag in node.flags.iter().filter(|f| !f.hidden && f.expects_value()) {
                    for form in forms(flag, &node.flags, options) {
                        let _ = writeln!(
                            script,
                            "      {} {{ $skip = $true }}",
                            ps_quote(&format!("{}:{form}", node.path))
                        );
                    }
                }
                for (name, canonical) in &node.children {
                    let path = if node.path.is_empty() {
                        canonical.clone()
                    } else {
                        format!("{} {canonical}", node.path)
                    };
                    let _ = writeln!(
                        script,
                        "      {} {{ $path = {} }}",
                        ps_quote(&format!("{}:{name}", node.path)),
                        ps_quote(&path)
                    );
                }
            }
            script.push_str(
                "    }\n    $previous = $token\n  }\n  $candidates = @()\n  switch ($path) {\n",
            );
            for node in &nodes {
                let candidates = node
                    .children
                    .iter()
                    .map(|(n, _)| n.clone())
                    .chain(
                        node.flags
                            .iter()
                            .filter(|f| !f.hidden)
                            .flat_map(|flag| forms(flag, &node.flags, options)),
                    )
                    .chain(globals.iter().map(|s| s.to_string()))
                    .map(|s| ps_quote(&s))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    script,
                    "    {} {{ $candidates = @({candidates}) }}",
                    ps_quote(&node.path)
                );
            }
            script.push_str("  }\n  $valueFlag = $previous; $valueCur = $wordToComplete; $prefix = ''\n  if ($wordToComplete.StartsWith('--') -and $wordToComplete.Contains('=')) { $index = $wordToComplete.IndexOf('='); $valueFlag = $wordToComplete.Substring(0, $index); $valueCur = $wordToComplete.Substring($index+1); $prefix = $valueFlag + '=' }\n  switch (\"${path}:$valueFlag\") {\n");
            for node in &nodes {
                for flag in node.flags.iter().filter(|f| !f.hidden) {
                    let element = flag.element.as_deref().unwrap_or(flag);
                    if element.enum_values.is_empty() {
                        continue;
                    }
                    for form in forms(flag, &node.flags, options) {
                        let _ = writeln!(
                            script,
                            "    {} {{ $candidates = @({}) }}",
                            ps_quote(&format!("{}:{form}", node.path)),
                            element
                                .enum_values
                                .iter()
                                .map(|s| ps_quote(s))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
            }
            script.push_str("  }\n  $candidates | Where-Object { $_.StartsWith($valueCur, [StringComparison]::OrdinalIgnoreCase) } | ForEach-Object { [System.Management.Automation.CompletionResult]::new($prefix + $_, $_, 'ParameterValue', $_) }\n}\n");
            script
        }
    };
    let tag = program
        .version
        .as_ref()
        .map(|v| format!("{} v{v}", program.name))
        .unwrap_or_else(|| program.name.clone())
        .replace(['\n', '\r'], " ");
    let header = format!("# Completion for {tag}\n");
    if shell == Shell::Zsh {
        let (first, rest) = script.split_once('\n').unwrap_or((&script, ""));
        format!("{first}\n{header}{rest}")
    } else {
        format!("{header}{script}")
    }
}
