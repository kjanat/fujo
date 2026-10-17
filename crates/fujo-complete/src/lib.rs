use fujo_core::CliError;
use fujo_core::Result;
use fujo_core::command::CommandSchema;
use fujo_core::command::GlobalFlag;
use fujo_core::command::GlobalValue;
use fujo_core::command::ProgramSchema;
use fujo_core::command::SHELLS;
use fujo_core::parse::ParseOptions;
use fujo_core::schema::InputSchema;
use fujo_core::schema::Kind;
use fujo_core::schema::Split;
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Values {
    Text,
    Files,
    Choices(Vec<String>),
    Collection {
        element: Box<Values>,
        delimiter: Option<String>,
        keyed: bool,
    },
}
impl Values {
    fn of(input: &InputSchema) -> Self {
        if input.hidden {
            return Self::Text;
        }
        if let Some(element) = &input.element {
            return Self::Collection {
                element: Box::new(Self::of(element)),
                delimiter: match &input.cli_split {
                    Split::Delimiter(value) => Some(value.as_str().to_owned()),
                    _ => None,
                },
                keyed: input.kind == Kind::KeyValue,
            };
        }
        let target = input;
        if !target.enum_values.is_empty() {
            Self::Choices(target.enum_values.clone())
        } else if target.kind == Kind::Path {
            Self::Files
        } else {
            Self::Text
        }
    }
    fn global(value: GlobalValue) -> Self {
        match value {
            GlobalValue::Shell => Self::Choices(SHELLS.map(str::to_owned).to_vec()),
            GlobalValue::Path => Self::Files,
        }
    }
}

struct Positional {
    index: usize,
    rest: bool,
    values: Values,
}

struct Node {
    path: String,
    flags: Vec<InputSchema>,
    args: Vec<InputSchema>,
    children: Vec<(String, String)>,
}
impl Node {
    fn visible_flags(&self) -> impl Iterator<Item = &InputSchema> {
        self.flags.iter().filter(|f| !f.hidden)
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
    fn value_forms(&self, globals: &[GlobalFlag], options: ParseOptions) -> Vec<(Values, String)> {
        let lookup = self.lookup(options);
        let mut forms: Vec<_> = lookup
            .iter()
            .filter(|(_, (flag, negated))| flag.expects_value() && !negated)
            .flat_map(|(name, (flag, _))| {
                std::iter::once(format!("--{name}"))
                    .chain((name.chars().count() == 1).then(|| format!("-{name}")))
                    .map(move |form| (Values::of(flag), form))
            })
            .collect();
        forms.extend(globals.iter().flat_map(|global| {
            global.value.into_iter().flat_map(|value| {
                std::iter::once(global.long)
                    .chain(global.short)
                    .map(move |form| (Values::global(value), form.to_owned()))
            })
        }));
        forms
    }
    fn short_forms(&self, globals: &[GlobalFlag], options: ParseOptions) -> Vec<(String, bool)> {
        let mut forms: Vec<_> = self
            .lookup(options)
            .into_iter()
            .filter(|(name, _)| name.chars().count() == 1)
            .map(|(name, (flag, negated))| (name, flag.expects_value() && !negated))
            .collect();
        forms.extend(globals.iter().filter_map(|global| {
            global.short.map(|name| {
                (
                    name.trim_start_matches('-').to_owned(),
                    global.value.is_some(),
                )
            })
        }));
        forms
    }
    fn positionals(&self) -> Vec<Positional> {
        self.args
            .iter()
            .enumerate()
            .filter(|(_, arg)| !arg.hidden)
            .map(|(index, arg)| Positional {
                index,
                rest: arg.variadic,
                values: Values::of(arg),
            })
            .collect()
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
                args: command.args.clone(),
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
        args: program
            .default
            .iter()
            .flat_map(|c| c.args.iter().cloned())
            .collect(),
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
            args: vec![],
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
fn words(values: &[String]) -> String {
    values
        .iter()
        .map(|s| quote(s))
        .collect::<Vec<_>>()
        .join(" ")
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
fn ps_words(values: &[String]) -> String {
    values
        .iter()
        .map(|s| ps_quote(s))
        .collect::<Vec<_>>()
        .join(", ")
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
        Shell::Bash | Shell::Zsh => posix(program, &nodes, &globals, &function, shell, options),
        Shell::Fish => fish(program, &nodes, &globals, &function, options),
        Shell::PowerShell => powershell(program, &nodes, &globals, options),
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

fn posix_short(nodes: &[Node], globals: &[GlobalFlag], options: ParseOptions) -> String {
    let mut script = String::from(
        "    short_flag=''; short_tail=''; short_prefix=''\n    if [[ $token == -?* && $token != --* ]]; then\n      short_tail=\"${token#-}\"; short_prefix='-'\n      while [[ -n $short_tail ]]; do\n        short_char=\"${short_tail%\"${short_tail#?}\"}\"; short_tail=\"${short_tail#?}\"; short_prefix+=\"$short_char\"\n        case \"$path:$short_char\" in\n",
    );
    for node in nodes {
        for (name, value) in node.short_forms(globals, options) {
            let action = if value {
                "short_flag=\"-$short_char\"; break"
            } else {
                ":"
            };
            let _ = writeln!(
                script,
                "          {}) {action} ;;",
                quote(&node.keyed(&name))
            );
        }
    }
    script.push_str("          *) break ;;\n        esac\n      done\n    fi\n");
    script
}

fn posix_values(values: &Values, bash: bool) -> String {
    if let Values::Collection {
        element,
        delimiter,
        keyed,
    } = values
    {
        let mut script = String::new();
        if let Some(delimiter) = delimiter {
            let _ = writeln!(script, "      delimiter={}", quote(delimiter));
            script.push_str("      while [[ $value_cur == *\"$delimiter\"* ]]; do prefix+=\"${value_cur%%\"$delimiter\"*}$delimiter\"; value_cur=\"${value_cur#*\"$delimiter\"}\"; done\n");
        }
        if *keyed {
            script.push_str(if bash {
                "      [[ $value_cur == ?*=* ]] || { COMPREPLY=(); return; }\n"
            } else {
                "      [[ $value_cur == ?*=* ]] || return 1\n"
            });
            script.push_str("      prefix+=\"${value_cur%%=*}=\"; value_cur=\"${value_cur#*=}\"\n");
        }
        script.push_str(&posix_values(element, bash));
        return script;
    }
    let mut script = String::new();
    if !bash && !matches!(values, Values::Text) {
        script.push_str("      [[ -n $prefix ]] && compset -P \"${(b)prefix}\"\n");
    }
    match values {
        Values::Choices(choices) => {
            let _ = writeln!(script, "      candidates=({})", words(choices));
            script.push_str(if bash {
                "      COMPREPLY=(); for token in \"${candidates[@]}\"; do [[ $token == \"$value_cur\"* ]] && COMPREPLY+=(\"$prefix$token\"); done\n"
            } else { "      compadd -- \"${candidates[@]}\"; return\n" });
        }
        Values::Files => script.push_str(if bash {
            "      compopt -o filenames 2>/dev/null\n      mapfile -t COMPREPLY < <(compgen -f -- \"$value_cur\"); for i in \"${!COMPREPLY[@]}\"; do COMPREPLY[i]=\"$prefix${COMPREPLY[i]}\"; done\n"
        } else { "      _files; return\n" }),
        Values::Text => return if bash { "      COMPREPLY=(); return\n" } else { "      return 1\n" }.into(),
        Values::Collection { .. } => unreachable!(),
    }
    if bash {
        // Readline replaces only the suffix after its last word-break character.
        script.push_str("      local discard='' part='' rest=\"$cur\" char\n      while [[ -n $rest ]]; do\n        char=\"${rest%\"${rest#?}\"}\"; rest=\"${rest#?}\"; part+=\"$char\"\n        [[ $COMP_WORDBREAKS == *\"$char\"* ]] && discard=\"$part\"\n      done\n      if [[ -n $discard ]]; then for i in \"${!COMPREPLY[@]}\"; do COMPREPLY[i]=\"${COMPREPLY[i]#\"$discard\"}\"; done; fi\n      return\n");
    }
    script
}

fn posix(
    program: &ProgramSchema,
    nodes: &[Node],
    globals: &[GlobalFlag],
    function: &str,
    shell: Shell,
    options: ParseOptions,
) -> String {
    let bash = shell == Shell::Bash;
    let mut script = if bash {
        format!(
            "{function}() {{\n  local cur prev path='' token i skip=0 pos=0 literal=0 files=0 cword\n  local pending='' short_flag short_tail short_prefix short_char delimiter\n  local -a words candidates=() extra=()\n  if type _comp_get_words &>/dev/null; then\n    _comp_get_words -n =: cur prev words cword\n  elif type _get_comp_words_by_ref &>/dev/null; then\n    _get_comp_words_by_ref -n =: cur prev words cword\n  else\n    words=(); cword=0\n    for ((i=0; i<${{#COMP_WORDS[@]}}; i++)); do\n      if (( i > 0 )) && [[ ${{COMP_WORDS[i]}} == =* || ${{COMP_WORDS[i]}} == :* || ${{COMP_WORDS[i-1]}} == *= || ${{COMP_WORDS[i-1]}} == *: ]]; then\n        words[${{#words[@]}}-1]+=${{COMP_WORDS[i]}}\n      else\n        words+=(\"${{COMP_WORDS[i]}}\")\n      fi\n      (( i == COMP_CWORD )) && cword=$((${{#words[@]}}-1))\n    done\n    cur=${{words[cword]}} prev=${{words[cword-1]}}\n  fi\n  for ((i=1; i<cword; i++)); do\n"
        )
    } else {
        format!(
            "#compdef {}\n{function}() {{\n  local cur=\"$words[CURRENT]\" prev=\"$words[CURRENT-1]\" path='' token i skip=0 pos=0 literal=0 files=0\n  local pending='' short_flag short_tail short_prefix short_char delimiter\n  local -a candidates extra\n  for ((i=2; i<CURRENT; i++)); do\n",
            program.name
        )
    };
    script.push_str("    token=\"${words[i]}\"\n    (( skip )) && { skip=0; pending=''; continue; }\n    (( literal )) && { pos=$((pos+1)); continue; }\n    [[ $token == -- ]] && { literal=1; continue; }\n" );
    script.push_str(&posix_short(nodes, globals, options));
    script.push_str("    if [[ -n $short_flag ]]; then [[ -z $short_tail ]] && { skip=1; pending=\"$short_flag\"; }; continue; fi\n    case \"$path:$token\" in\n");
    for node in nodes {
        for (_, form) in node.value_forms(globals, options) {
            let _ = writeln!(
                script,
                "      {}) skip=1; pending=\"$token\"; continue ;;",
                quote(&node.keyed(&form))
            );
        }
        for (name, path) in node.child_paths() {
            let _ = writeln!(
                script,
                "      {}) path={}; pos=0 ;;",
                quote(&node.keyed(name)),
                quote(&path)
            );
        }
    }
    script.push_str("      *) [[ $token == -?* ]] || pos=$((pos+1)) ;;\n    esac\n  done\n  local value_cur=\"$cur\" prefix='' value_flag=\"$pending\"\n  if [[ $cur == --*=* ]]; then value_flag=\"${cur%%=*}\"; value_cur=\"${cur#*=}\"; prefix=\"${cur%%=*}=\"; fi\n");
    script.push_str("  token=\"$cur\"\n");
    script.push_str(&posix_short(nodes, globals, options));
    script.push_str("  if [[ -n $short_flag && -n $short_tail ]]; then value_flag=\"$short_flag\"; value_cur=\"$short_tail\"; prefix=\"$short_prefix\"; fi\n");
    script.push_str("  if (( ! literal )); then\n  case \"$path:$value_flag\" in\n");
    for node in nodes {
        for (values, form) in node.value_forms(globals, options) {
            let _ = writeln!(script, "    {})", quote(&node.keyed(&form)));
            script.push_str(&posix_values(&values, bash));
            script.push_str("      ;;\n");
        }
    }
    script.push_str("  esac\n  fi\n  if (( literal )) || [[ $cur != -* ]]; then\n    :\n");
    for node in nodes {
        for positional in node.positionals() {
            let position = if positional.rest {
                format!("(( pos >= {} ))", positional.index)
            } else {
                format!("(( pos == {} ))", positional.index)
            };
            let action = match &positional.values {
                Values::Collection { .. } => {
                    let _ = writeln!(
                        script,
                        "    if [[ $path == {} ]] && {position}; then\n      value_cur=\"$cur\"; prefix=''",
                        quote(&node.path)
                    );
                    script.push_str(&posix_values(&positional.values, bash));
                    script.push_str("    fi\n");
                    continue;
                }
                Values::Text => continue,
                Values::Files => "files=1".to_owned(),
                Values::Choices(choices) => format!("extra+=({})", words(choices)),
            };
            let _ = writeln!(
                script,
                "    [[ $path == {} ]] && {position} && {action}",
                quote(&node.path)
            );
        }
    }
    script.push_str("  fi\n  if (( ! literal )); then\n  case \"$path\" in\n");
    for node in nodes {
        let _ = writeln!(
            script,
            "    {}) candidates=({}) ;;",
            quote(&node.path),
            words(&node.candidates(globals, options))
        );
    }
    script.push_str("  esac\n  fi\n  candidates+=(\"${extra[@]}\")\n");
    if bash {
        script.push_str("  COMPREPLY=(); for token in \"${candidates[@]}\"; do [[ $token == \"$cur\"* ]] && COMPREPLY+=(\"$token\"); done\n  if (( files )); then\n    compopt -o filenames 2>/dev/null\n    while IFS= read -r token; do COMPREPLY+=(\"$token\"); done < <(compgen -f -- \"$cur\")\n  fi\n}\n");
        let _ = writeln!(script, "complete -F {function} -- {}", quote(&program.name));
    } else {
        script.push_str(
            "  compadd -- \"${candidates[@]}\"\n  (( files )) && _files\n  return 0\n}\n",
        );
        let _ = writeln!(
            script,
            "if [[ ${{zsh_eval_context[-1]}} == loadautofunc ]]; then\n  eval \"${{funcstack[1]}}() {{ {function} \\\"\\$@\\\"; }}\"\n  {function} \"$@\"\nelse\n  compdef {function} {}\nfi",
            quote(&program.name)
        );
    }
    script
}

fn fish_short(
    nodes: &[Node],
    globals: &[GlobalFlag],
    function: &str,
    options: ParseOptions,
) -> String {
    let mut script = format!(
        "function {function}_short --argument-names path token\n  string match -qr -- '^-[^-]' $token; or return\n  set -l tail (string sub -s 2 -- $token)\n  set -l prefix '-'\n  while test -n \"$tail\"\n    set -l char (string sub -l 1 -- $tail)\n    set tail (string sub -s 2 -- $tail)\n    set prefix \"$prefix$char\"\n    switch \"$path:$char\"\n"
    );
    for node in nodes {
        for (name, value) in node.short_forms(globals, options) {
            let _ = writeln!(script, "      case {}", fish_quote(&node.keyed(&name)));
            if value {
                script.push_str(
                    "        printf '%s\\n' \"-$char\" \"$tail\" \"$prefix\"\n        return\n",
                );
            }
        }
    }
    script.push_str("      case '*'\n        return\n    end\n  end\nend\n");
    script
}

fn fish_collection(function: &str) -> String {
    format!(
        r#"function {function}_collection --argument-names delimiter keyed kind
  set -l value_cur (commandline -ct)
  set -l state ({function}_state)
  set -l prefix ''
  if test "$state[2]" = 0
    if string match -qr -- '^--[^=]+=' "$value_cur"
      set value_cur (string replace -r '^--[^=]+=' '' -- "$value_cur")
    else
      set -l short ({function}_short "$state[3]" "$value_cur")
      if test (count $short) -eq 3
        set value_cur "$short[2]"
      end
    end
  end
  if test -n "$delimiter"
    set -l parts (string split -m 1 -- "$delimiter" "$value_cur")
    while test (count $parts) -eq 2
      set prefix "$prefix$parts[1]$delimiter"
      set value_cur "$parts[2]"
      set parts (string split -m 1 -- "$delimiter" "$value_cur")
    end
  end
  if test "$keyed" = 1
    set -l parts (string split -m 1 -- '=' "$value_cur")
    test (count $parts) -eq 2; and test -n "$parts[1]"; or return
    set prefix "$prefix$parts[1]="
    set value_cur "$parts[2]"
  end
  switch $kind
    case files
      for candidate in (__fish_complete_path "$value_cur")
        printf '%s%s\n' "$prefix" "$candidate"
      end
    case choices
      for candidate in $argv[4..]
        if test (string sub -l (string length -- "$value_cur") -- "$candidate") = "$value_cur"
          printf '%s%s\n' "$prefix" "$candidate"
        end
      end
  end
end
"#
    )
}
fn fish_collection_action(values: &Values, function: &str) -> String {
    let Values::Collection {
        element,
        delimiter,
        keyed,
    } = values
    else {
        unreachable!()
    };
    let (kind, choices) = match element.as_ref() {
        Values::Files => ("files", String::new()),
        Values::Choices(choices) => (
            "choices",
            choices
                .iter()
                .map(|v| format!(" {}", fish_quote(v)))
                .collect(),
        ),
        _ => ("text", String::new()),
    };
    fish_quote(&format!(
        "({function}_collection {} {} {kind}{choices})",
        fish_quote(delimiter.as_deref().unwrap_or("")),
        u8::from(*keyed)
    ))
}

fn fish(
    program: &ProgramSchema,
    nodes: &[Node],
    globals: &[GlobalFlag],
    function: &str,
    options: ParseOptions,
) -> String {
    let mut script = format!(
        "function {function}_state\n  set -l tokens (commandline -opc)\n  set -e tokens[1]\n  set -l path ''\n  set -l skip 0\n  set -l pending ''\n  set -l pos 0\n  set -l literal 0\n  for token in $tokens\n    if test $skip -eq 1\n      set skip 0\n      set pending ''\n      continue\n    end\n    if test $literal -eq 1\n      set pos (math $pos + 1)\n      continue\n    end\n    if test \"$token\" = --\n      set literal 1\n      continue\n    end\n    set -l short ({function}_short \"$path\" \"$token\")\n    if test (count $short) -eq 3\n      if test -z \"$short[2]\"\n        set skip 1\n        set pending \"$short[1]\"\n      end\n      continue\n    end\n    switch \"$path:$token\"\n"
    );
    for node in nodes {
        for (_, form) in node.value_forms(globals, options) {
            let _ = writeln!(
                script,
                "      case {}\n        set skip 1\n        set pending $token\n        continue",
                quote(&node.keyed(&form))
            );
        }
        for (name, path) in node.child_paths() {
            let _ = writeln!(
                script,
                "      case {}\n        set path {}\n        set pos 0",
                quote(&node.keyed(name)),
                quote(&path)
            );
        }
    }
    let _ = write!(
        script,
        "      case '*'\n        if not string match -qr -- '^-.+' $token\n          set pos (math $pos + 1)\n        end\n    end\n  end\n  printf '%s\\n' $pos $literal $path \"$pending\"\nend\nfunction {function}_in\n  set -l state ({function}_state)\n  set -l expected (string join ' ' -- $argv)\n  test \"$state[3]\" = \"$expected\"\nend\nfunction {function}\n  set -l state ({function}_state)\n  set -l expected (string join ' ' -- $argv)\n  test \"$state[2]\" = 0; and test \"$state[3]\" = \"$expected\"\nend\nfunction {function}_arg --argument-names index rest\n  set -l state ({function}_state)\n  test -z \"$state[4]\"; or return 1\n  set -l path $argv\n  set -e path[1..2]\n  set -l expected (string join ' ' -- $path)\n  test \"$state[3]\" = \"$expected\"; or return 1\n  if test \"$rest\" = 1\n    test $state[1] -ge $index\n  else\n    test $state[1] -eq $index\n  end\nend\n"
    );
    let program_name = quote(&program.name);
    for node in nodes {
        let parts: Vec<String> = node
            .path
            .split(' ')
            .filter(|part| !part.is_empty())
            .map(quote)
            .collect();
        let suffix = parts.iter().map(|p| format!(" {p}")).collect::<String>();
        let condition = quote(&format!("{function}{suffix}"));
        let _ = writeln!(
            script,
            "complete -c {program_name} -n {} -f",
            quote(&format!("{function}_in{suffix}"))
        );
        for (name, _) in &node.children {
            let _ = writeln!(
                script,
                "complete -c {program_name} -f -n {condition} -a {}",
                quote(name)
            );
        }
        let lookup = node.lookup(options);
        for flag in node.visible_flags() {
            let negated = flag.negated_name.as_deref().map(fujo_core::parse::spelling);
            for form in node.forms(&lookup, flag) {
                let long = form.starts_with("--");
                let _ = write!(
                    script,
                    "complete -c {program_name} -n {condition} {} {} -d {}",
                    if long { "-l" } else { "-s" },
                    quote(form.trim_start_matches('-')),
                    quote(&flag.description)
                );
                if flag.expects_value() && Some(&form) != negated.as_ref() {
                    script.push_str(&fish_values(&Values::of(flag), function));
                }
                script.push('\n');
            }
        }
        for positional in node.positionals() {
            let action = match &positional.values {
                Values::Collection { .. } => format!(
                    "-f -a {}",
                    fish_collection_action(&positional.values, function)
                ),
                Values::Text => continue,
                Values::Files => "-F".to_owned(),
                Values::Choices(choices) => format!("-f -a {}", fish_words(choices)),
            };
            let _ = writeln!(
                script,
                "complete -c {program_name} -n {} {action}",
                quote(&format!(
                    "{function}_arg {} {}{suffix}",
                    positional.index,
                    u8::from(positional.rest)
                ))
            );
        }
    }
    for global in globals {
        let _ = write!(
            script,
            "complete -c {program_name} -l {}",
            quote(global.long.trim_start_matches('-'))
        );
        if let Some(short) = global.short {
            let _ = write!(script, " -s {}", quote(short.trim_start_matches('-')));
        }
        if let Some(value) = global.value {
            script.push_str(&fish_values(&Values::global(value), function));
        }
        script.push('\n');
    }
    script.push_str(&fish_short(nodes, globals, function, options));
    script.push_str(&fish_collection(function));
    script
}
fn fish_values(values: &Values, function: &str) -> String {
    match values {
        Values::Collection { .. } => format!(" -x -a {}", fish_collection_action(values, function)),
        Values::Text => " -x".into(),
        Values::Files => " -r -F".into(),
        Values::Choices(choices) => format!(" -x -a {}", fish_words(choices)),
    }
}

fn ps_short(nodes: &[Node], globals: &[GlobalFlag], options: ParseOptions) -> String {
    let mut script = String::from(
        "  $short = { param($token, $path)\n    if ($token.StartsWith('--') -or -not $token.StartsWith('-')) { return }\n    for ($j = 1; $j -lt $token.Length; $j++) {\n      $char = $token.Substring($j, 1)\n      switch -CaseSensitive (\"${path}:$char\") {\n",
    );
    for node in nodes {
        for (name, value) in node.short_forms(globals, options) {
            let action = if value {
                "return [pscustomobject]@{ Flag = '-' + $char; Tail = $token.Substring($j + 1); Prefix = $token.Substring(0, $j + 1) }"
            } else {
                ""
            };
            let _ = writeln!(
                script,
                "        {} {{ {action} }}",
                ps_quote(&node.keyed(&name))
            );
        }
    }
    script.push_str("        default { return }\n      }\n    }\n  }\n");
    script
}

fn ps_values(values: &Values) -> String {
    match values {
        Values::Collection {
            element,
            delimiter,
            keyed,
        } => {
            let mut script = String::new();
            if let Some(delimiter) = delimiter {
                let _ = write!(
                    script,
                    "$delimiter = {}; while (($index = $valueCur.IndexOf($delimiter, [StringComparison]::Ordinal)) -ge 0) {{ $index += $delimiter.Length; $prefix += $valueCur.Substring(0, $index); $valueCur = $valueCur.Substring($index) }}; ",
                    ps_quote(delimiter)
                );
            }
            if *keyed {
                script.push_str("$index = $valueCur.IndexOf('='); if ($index -lt 1) { & $noResults $wordToComplete; return }; $prefix += $valueCur.Substring(0, $index + 1); $valueCur = $valueCur.Substring($index + 1); ");
            }
            script.push_str(&ps_values(element));
            script
        }
        Values::Choices(choices) => format!(
            "$matches = @(@({}) | Where-Object {{ $_.StartsWith($valueCur, [StringComparison]::OrdinalIgnoreCase) }}); if ($matches.Count) {{ $matches | ForEach-Object {{ [System.Management.Automation.CompletionResult]::new($prefix + $_, $_, 'ParameterValue', $_) }} }} else {{ & $noResults $wordToComplete }}; return",
            ps_words(choices)
        ),
        Values::Files => "& $completeFiles $valueCur $prefix; return".into(),
        Values::Text => "& $noResults $wordToComplete; return".into(),
    }
}

fn powershell(
    program: &ProgramSchema,
    nodes: &[Node],
    globals: &[GlobalFlag],
    options: ParseOptions,
) -> String {
    // Completion options let us probe empty-result support without registering a command.
    let mut script = format!(
        r#"& {{
  $probe = 'fujo-empty-result-probe '
  $options = @{{ NativeArgumentCompleters = @{{ 'fujo-empty-result-probe' = {{ '' }} }} }}
  try {{
    $emptyResultsSupported = [System.Management.Automation.CommandCompletion]::CompleteInput($probe, $probe.Length, $options).CompletionMatches.Count -eq 0
  }} catch {{
    $emptyResultsSupported = $false
    $Error.RemoveAt(0)
  }}
  Register-ArgumentCompleter -Native -CommandName {} -ScriptBlock ({{
  param($wordToComplete, $commandAst, $cursorPosition)
  $noResults = {{
    param($text)
    if ($emptyResultsSupported) {{ '' }} elseif ($text) {{ [System.Management.Automation.CompletionResult]::new($text, $text, 'ParameterValue', $text) }}
  }}
  $completeFiles = {{
    param($text, $prefix = '')
    $matches = @([System.Management.Automation.CompletionCompleters]::CompleteFilename($text))
    if ($matches.Count) {{
      foreach ($match in $matches) {{ [System.Management.Automation.CompletionResult]::new($prefix + $match.CompletionText, $match.ListItemText, $match.ResultType, $match.ToolTip) }}
    }} else {{ & $noResults ($prefix + $text) }}
  }}
{short}
  $path = ''; $previous = ''; $pending = ''; $skip = $false; $pos = 0; $literal = $false
  $tokens = [System.Collections.Generic.List[string]]::new()
  $end = -1
  foreach ($element in ($commandAst.CommandElements | Select-Object -Skip 1 | Where-Object {{ $_.Extent.EndOffset -lt $cursorPosition }})) {{
    if ($element.Extent.StartOffset -eq $end) {{ $tokens[$tokens.Count - 1] += $element.Extent.Text }}
    else {{ $tokens.Add($element.Extent.Text) }}
    $end = $element.Extent.EndOffset
  }}
  foreach ($token in $tokens) {{
    if ($skip) {{ $skip = $false; $pending = ''; $previous = $token; continue }}
    if ($literal) {{ $pos++; $previous = $token; continue }}
    if ($token -eq '--') {{ $literal = $true; $previous = $token; continue }}
    $parsedShort = & $short $token $path
    if ($parsedShort) {{
      if (-not $parsedShort.Tail) {{ $skip = $true; $pending = $parsedShort.Flag }}
      continue
    }}
    switch -CaseSensitive ("${{path}}:$token") {{
"#,
        ps_quote(&program.name),
        short = ps_short(nodes, globals, options)
    );
    for node in nodes {
        for (_, form) in node.value_forms(globals, options) {
            let _ = writeln!(
                script,
                "      {} {{ $skip = $true; $pending = $token }}",
                ps_quote(&node.keyed(&form))
            );
        }
        for (name, path) in node.child_paths() {
            let _ = writeln!(
                script,
                "      {} {{ $path = {}; $pos = 0 }}",
                ps_quote(&node.keyed(name)),
                ps_quote(&path)
            );
        }
    }
    script.push_str("      default { if ($token -notlike '-?*') { $pos++ } }\n    }\n    $previous = $token\n  }\n  $valueFlag = $pending; $valueCur = $wordToComplete; $prefix = ''\n  if ($wordToComplete.StartsWith('--') -and $wordToComplete.Contains('=')) { $index = $wordToComplete.IndexOf('='); $valueFlag = $wordToComplete.Substring(0, $index); $valueCur = $wordToComplete.Substring($index+1); $prefix = $valueFlag + '=' }\n  $parsedShort = & $short $wordToComplete $path\n  if ($parsedShort -and $parsedShort.Tail) { $valueFlag = $parsedShort.Flag; $valueCur = $parsedShort.Tail; $prefix = $parsedShort.Prefix }\n  if (-not $literal) {\n    switch -CaseSensitive (\"${path}:$valueFlag\") {\n");
    for node in nodes {
        for (values, form) in node.value_forms(globals, options) {
            let action = ps_values(&values);
            let _ = writeln!(
                script,
                "      {} {{ {action} }}",
                ps_quote(&node.keyed(&form))
            );
        }
    }
    script.push_str("    }\n  }\n  $extra = @(); $files = $false\n  if ($literal -or -not $wordToComplete.StartsWith('-')) {\n");
    for node in nodes {
        for positional in node.positionals() {
            let action = match &positional.values {
                Values::Collection { .. } => format!(
                    "$valueCur = $wordToComplete; $prefix = ''; {}",
                    ps_values(&positional.values)
                ),
                Values::Text => continue,
                Values::Files => "$files = $true".to_owned(),
                Values::Choices(choices) => format!("$extra += @({})", ps_words(choices)),
            };
            let _ = writeln!(
                script,
                "    if ($path -ceq {} -and $pos -{} {}) {{ {action} }}",
                ps_quote(&node.path),
                if positional.rest { "ge" } else { "eq" },
                positional.index
            );
        }
    }
    script.push_str("  }\n  if ($files) { & $completeFiles $wordToComplete; return }\n  $candidates = @()\n  if (-not $literal) {\n    switch -CaseSensitive ($path) {\n");
    for node in nodes {
        let _ = writeln!(
            script,
            "      {} {{ $candidates = @({}) }}",
            ps_quote(&node.path),
            ps_words(&node.candidates(globals, options))
        );
    }
    script.push_str("    }\n  }\n  $candidates += $extra\n  $matches = @($candidates | Where-Object { $_.StartsWith($wordToComplete, [StringComparison]::OrdinalIgnoreCase) })\n  if ($matches.Count) {\n    $matches | ForEach-Object { [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_) }\n  } else { & $noResults $wordToComplete }\n  }.GetNewClosure())\n}\n");
    script
}
