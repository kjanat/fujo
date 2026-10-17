use fujo_core::CliError;
use fujo_core::Result;
use fujo_core::command::CommandSchema;
use fujo_core::command::GlobalFlag;
use fujo_core::command::ProgramSchema;
use fujo_core::command::SHELLS;
use fujo_core::parse::ParseOptions;
use fujo_core::schema::InputSchema;
use std::fmt::Write;
use std::path::Path;
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
impl Shell {
    pub fn from_path(path: &str) -> Option<Self> {
        Path::new(path).file_stem()?.to_str()?.parse().ok()
    }
}
struct Node {
    path: String,
    flags: Vec<InputSchema>,
    children: Vec<(String, String)>,
}
impl Node {
    fn visible_flags(&self) -> impl Iterator<Item = &InputSchema> {
        self.flags.iter().filter(|f| !f.hidden)
    }
    fn value_flags(&self) -> impl Iterator<Item = &InputSchema> {
        self.visible_flags().filter(|f| f.expects_value())
    }
    fn lookup(&self, options: ParseOptions) -> Lookup<'_> {
        fujo_core::parse::flag_lookup(&self.flags, options)
    }
    fn forms(&self, lookup: &Lookup<'_>, flag: &InputSchema) -> Vec<String> {
        std::iter::once(format!(
            "--{}",
            fujo_core::parse::canonical_flag_name_with(lookup, flag)
        ))
        .chain(
            flag.aliases
                .iter()
                .filter(|a| !a.hidden)
                .map(|a| fujo_core::parse::spelling(&a.name)),
        )
        .chain(flag.negated_name.as_deref().map(fujo_core::parse::spelling))
        .collect()
    }
    fn value_forms(
        &self,
        globals: &[GlobalFlag],
        options: ParseOptions,
    ) -> Vec<(Vec<String>, String)> {
        let lookup = self.lookup(options);
        let mut forms: Vec<_> = self
            .value_flags()
            .flat_map(|flag| {
                let values = enum_values(flag);
                self.forms(&lookup, flag)
                    .into_iter()
                    .map(move |form| (values.to_vec(), form))
            })
            .collect();
        forms.extend(
            globals
                .iter()
                .filter(|global| global.value.is_some())
                .flat_map(|global| {
                    let values: Vec<String> =
                        global.values.iter().map(|v| (*v).to_owned()).collect();
                    std::iter::once(global.long)
                        .chain(global.short)
                        .map(move |form| (values.clone(), form.to_owned()))
                }),
        );
        forms
    }
    fn keyed(&self, form: &str) -> String {
        format!("{}:{form}", self.path)
    }
    fn child_paths(&self) -> impl Iterator<Item = (&str, String)> {
        self.children.iter().map(|(name, canonical)| {
            let path = if self.path.is_empty() {
                canonical.clone()
            } else {
                format!("{} {canonical}", self.path)
            };
            (name.as_str(), path)
        })
    }
    fn candidates(&self, globals: &[GlobalFlag], options: ParseOptions) -> Vec<String> {
        let lookup = self.lookup(options);
        self.children
            .iter()
            .map(|(name, _)| name.clone())
            .chain(
                self.visible_flags()
                    .flat_map(|flag| self.forms(&lookup, flag)),
            )
            .chain(globals.iter().flat_map(|global| {
                std::iter::once(global.long)
                    .chain(global.short)
                    .map(str::to_owned)
            }))
            .collect()
    }
}
type Lookup<'a> = std::collections::BTreeMap<String, (&'a InputSchema, bool)>;
fn enum_values(flag: &InputSchema) -> &[String] {
    &flag.element.as_deref().unwrap_or(flag).enum_values
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
            flags.extend(command.flags.iter().cloned());
            let propagated: Vec<_> = flags.iter().filter(|f| f.propagate).cloned().collect();
            nodes.push(Node {
                path: path.clone(),
                flags,
                children: children(&command.commands),
            });
            walk(&command.commands, &path, &propagated, nodes);
        }
    }
    let mut flags = program.flags.clone();
    flags.extend(program.default.iter().flat_map(|c| c.flags.iter().cloned()));
    let mut nodes = vec![Node {
        path: String::new(),
        flags,
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
        .map(InputSchema::propagated)
        .collect();
    walk(&program.commands, "", &inherited, &mut nodes);
    if program.completions {
        nodes.push(Node {
            path: "completions".into(),
            flags: vec![],
            children: SHELLS.map(|s| (s.into(), s.into())).to_vec(),
        });
    }
    nodes
}
fn children(commands: &[CommandSchema]) -> Vec<(String, String)> {
    commands
        .iter()
        .filter(|c| !c.hidden)
        .flat_map(|c| {
            std::iter::once(&c.name)
                .chain(&c.aliases)
                .map(move |name| (name.clone(), c.name.clone()))
        })
        .collect()
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn fish_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}
fn fish_words(values: &[String]) -> String {
    fish_quote(
        &values
            .iter()
            .map(|v| fish_quote(v))
            .collect::<Vec<_>>()
            .join(" "),
    )
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
    options: ParseOptions,
) -> String {
    let nodes = nodes(program);
    let function = identifier(&program.name);
    let globals = program.builtins.global_flags(
        program.version.is_some(),
        program.completions,
        program.config,
    );
    let script = match shell {
        Shell::Bash | Shell::Zsh => {
            let bash = shell == Shell::Bash;
            let mut script = if bash {
                format!(
                    "{function}() {{\n  local cur prev path='' token i skip=0 cword\n  local -a words candidates=()\n  if type _comp_get_words &>/dev/null; then\n    _comp_get_words -n =: cur prev words cword\n  elif type _get_comp_words_by_ref &>/dev/null; then\n    _get_comp_words_by_ref -n =: cur prev words cword\n  else\n    words=(); cword=0\n    for ((i=0; i<${{#COMP_WORDS[@]}}; i++)); do\n      if (( i > 0 )) && [[ ${{COMP_WORDS[i]}} == =* || ${{COMP_WORDS[i]}} == :* || ${{COMP_WORDS[i-1]}} == *= || ${{COMP_WORDS[i-1]}} == *: ]]; then\n        words[${{#words[@]}}-1]+=${{COMP_WORDS[i]}}\n      else\n        words+=(\"${{COMP_WORDS[i]}}\")\n      fi\n      (( i == COMP_CWORD )) && cword=$((${{#words[@]}}-1))\n    done\n    cur=${{words[cword]}} prev=${{words[cword-1]}}\n  fi\n  for ((i=1; i<cword; i++)); do\n"
                )
            } else {
                format!(
                    "#compdef {}\n{function}() {{\n  local cur=\"$words[CURRENT]\" prev=\"$words[CURRENT-1]\" path='' token i skip=0\n  local -a candidates\n  for ((i=2; i<CURRENT; i++)); do\n",
                    program.name
                )
            };
            script.push_str("    token=\"${words[i]}\"\n    (( skip )) && { skip=0; continue; }\n    [[ $token == -- ]] && break\n    case \"$path:$token\" in\n");
            for node in &nodes {
                for (_, form) in node.value_forms(&globals, options) {
                    let _ = writeln!(
                        script,
                        "      {}) skip=1; continue ;;",
                        quote(&node.keyed(&form))
                    );
                }
                for (name, path) in node.child_paths() {
                    let _ = writeln!(
                        script,
                        "      {}) path={} ;;",
                        quote(&node.keyed(name)),
                        quote(&path)
                    );
                }
            }
            script.push_str("    esac\n  done\n  local value_cur=\"$cur\" prefix='' value_flag=\"$prev\"\n  if [[ $cur == --*=* ]]; then value_flag=\"${cur%%=*}\"; value_cur=\"${cur#*=}\"; prefix=\"${cur%%=*}=\"; fi\n");
            if bash {
                script.push_str("  [[ $COMP_WORDBREAKS == *=* ]] && prefix=''\n");
            }
            script.push_str("  case \"$path:$value_flag\" in\n");
            for node in &nodes {
                for (values, form) in node.value_forms(&globals, options) {
                    let _ = writeln!(script, "    {})", quote(&node.keyed(&form)));
                    if !values.is_empty() {
                        let _ = writeln!(
                            script,
                            "      candidates=({})",
                            values
                                .iter()
                                .map(|s| quote(s))
                                .collect::<Vec<_>>()
                                .join(" ")
                        );
                        if bash {
                            script.push_str("      COMPREPLY=(); for token in \"${candidates[@]}\"; do [[ $token == \"$value_cur\"* ]] && COMPREPLY+=(\"$prefix$token\"); done; return ;;\n");
                        } else {
                            script.push_str(
                                "      compadd -P \"$prefix\" -- \"${candidates[@]}\"; return ;;\n",
                            );
                        }
                    } else if bash {
                        script.push_str("      mapfile -t COMPREPLY < <(compgen -f -- \"$value_cur\"); if [[ -n $prefix ]]; then for i in \"${!COMPREPLY[@]}\"; do COMPREPLY[i]=\"$prefix${COMPREPLY[i]}\"; done; fi; return ;;\n");
                    } else {
                        script.push_str("      _files; return ;;\n");
                    }
                }
            }
            script.push_str("  esac\n  case \"$path\" in\n");
            for node in &nodes {
                let candidates = node
                    .candidates(&globals, options)
                    .iter()
                    .map(|s| quote(s))
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
                for (_, form) in node.value_forms(&globals, options) {
                    let _ = writeln!(
                        script,
                        "      case {}\n        set skip 1\n        continue",
                        quote(&node.keyed(&form))
                    );
                }
                for (name, path) in node.child_paths() {
                    let _ = writeln!(
                        script,
                        "      case {}\n        set path {}",
                        quote(&node.keyed(name)),
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
                let lookup = node.lookup(options);
                for flag in node.visible_flags() {
                    for form in node.forms(&lookup, flag) {
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
                        let values = enum_values(flag);
                        if !values.is_empty() {
                            let _ = write!(script, " -f -a {}", fish_words(values));
                        }
                        script.push('\n');
                    }
                }
            }
            for global in &globals {
                let _ = write!(
                    script,
                    "complete -c {} -l {}",
                    quote(&program.name),
                    quote(global.long.trim_start_matches('-'))
                );
                if let Some(short) = global.short {
                    let _ = write!(script, " -s {}", quote(short.trim_start_matches('-')));
                }
                if global.value.is_some() {
                    script.push_str(" -r");
                }
                if !global.values.is_empty() {
                    let values: Vec<String> =
                        global.values.iter().map(|v| (*v).to_owned()).collect();
                    let _ = write!(script, " -f -a {}", fish_words(&values));
                }
                script.push('\n');
            }
            script
        }
        Shell::PowerShell => {
            let mut script = format!(
                "Register-ArgumentCompleter -Native -CommandName {} -ScriptBlock {{\n  param($wordToComplete, $commandAst, $cursorPosition)\n  $path = ''; $previous = ''; $skip = $false\n  $tokens = @($commandAst.CommandElements | Select-Object -Skip 1 | Where-Object {{ $_.Extent.EndOffset -lt $cursorPosition }} | ForEach-Object {{ $_.Extent.Text }})\n  foreach ($token in $tokens) {{\n    if ($skip) {{ $skip = $false; $previous = $token; continue }}\n    if ($token -eq '--') {{ break }}\n    switch (\"${{path}}:$token\") {{\n",
                ps_quote(&program.name)
            );
            for node in &nodes {
                for (_, form) in node.value_forms(&globals, options) {
                    let _ = writeln!(
                        script,
                        "      {} {{ $skip = $true }}",
                        ps_quote(&node.keyed(&form))
                    );
                }
                for (name, path) in node.child_paths() {
                    let _ = writeln!(
                        script,
                        "      {} {{ $path = {} }}",
                        ps_quote(&node.keyed(name)),
                        ps_quote(&path)
                    );
                }
            }
            script.push_str(
                "    }\n    $previous = $token\n  }\n  $candidates = @()\n  switch ($path) {\n",
            );
            for node in &nodes {
                let candidates = node
                    .candidates(&globals, options)
                    .iter()
                    .map(|s| ps_quote(s))
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
                for (values, form) in node.value_forms(&globals, options) {
                    let _ = writeln!(
                        script,
                        "    {} {{ $candidates = @({}) }}",
                        ps_quote(&node.keyed(&form)),
                        values
                            .iter()
                            .map(|s| ps_quote(s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
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
