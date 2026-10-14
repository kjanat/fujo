use crate::command::CommandSchema;
use crate::completion::Shell;
use crate::schema::InputSchema;
use dreamcli_core::command::ProgramSchema;
use dreamcli_output::visible_width;

#[derive(Debug, Clone)]
pub struct HelpOptions {
    pub width: usize,
}
impl Default for HelpOptions {
    fn default() -> Self {
        Self { width: 80 }
    }
}

pub fn root_help(program: &ProgramSchema, options: &HelpOptions) -> String {
    if let Some(default) = &program.default
        && program.commands.is_empty()
    {
        return command_help(default, &program.name, options, Some(program));
    }
    let mut text = format!(
        "{}{}\n",
        program.name,
        program
            .version
            .as_ref()
            .map(|v| format!(" {v}"))
            .unwrap_or_default()
    );
    if !program.description.is_empty() {
        text.push_str(&format!(
            "\n{}\n",
            wrap(&program.description, options.width, 0)
        ));
    }
    text.push_str(&format!("\nUsage: {} <command> [options]\n", program.name));
    entries(
        &mut text,
        "Commands",
        program
            .commands
            .iter()
            .filter(|c| !c.hidden)
            .map(|c| (c.name.clone(), c.description.clone()))
            .chain(program.completions.then_some((
                "completions <shell>".into(),
                "Generate shell completions".into(),
            )))
            .collect(),
        options.width,
    );
    globals(&mut text, program, options.width);
    text
}
pub fn command_help(
    command: &CommandSchema,
    bin: &str,
    options: &HelpOptions,
    program: Option<&ProgramSchema>,
) -> String {
    let mut text = format!("{bin}\n");
    if !command.description.is_empty() {
        text.push_str(&format!(
            "\n{}\n",
            wrap(&command.description, options.width, 0)
        ));
    }
    let args = command
        .args
        .iter()
        .map(|a| {
            format!(
                " {}{}{}",
                if a.required { '<' } else { '[' },
                format_args!("{}{}", a.name, if a.variadic { "..." } else { "" }),
                if a.required { '>' } else { ']' }
            )
        })
        .collect::<String>();
    text.push_str(&format!(
        "\nUsage: {bin}{args}{}{}\n",
        if command.commands.is_empty() {
            ""
        } else {
            " <command>"
        },
        if command.flags.is_empty() {
            ""
        } else {
            " [options]"
        }
    ));
    entries(
        &mut text,
        "Arguments",
        command
            .args
            .iter()
            .filter(|a| !a.hidden)
            .map(|a| (a.name.clone(), description(a)))
            .collect(),
        options.width,
    );
    entries(
        &mut text,
        "Options",
        command
            .flags
            .iter()
            .filter(|f| !f.hidden)
            .map(|flag| {
                let names = flag
                    .aliases
                    .iter()
                    .filter(|a| !a.hidden)
                    .map(|a| {
                        format!(
                            "{}{}",
                            if a.name.chars().count() == 1 {
                                "-"
                            } else {
                                "--"
                            },
                            a.name
                        )
                    })
                    .chain(std::iter::once(format!("--{}", kebab(&flag.name))))
                    .chain(flag.negated_name.as_ref().map(|n| format!("--{n}")))
                    .collect::<Vec<_>>()
                    .join(", ");
                let hint = if flag.expects_value() {
                    format!(" <{}>", flag.value_hint.as_deref().unwrap_or("value"))
                } else {
                    String::new()
                };
                (format!("{names}{hint}"), description(flag))
            })
            .collect(),
        options.width,
    );
    entries(
        &mut text,
        "Commands",
        command
            .commands
            .iter()
            .filter(|c| !c.hidden)
            .map(|c| (c.name.clone(), c.description.clone()))
            .collect(),
        options.width,
    );
    if let Some(program) = program {
        globals(&mut text, program, options.width);
    }
    if !command.examples.is_empty() {
        entries(
            &mut text,
            "Examples",
            command
                .examples
                .iter()
                .map(|e| (e.command.replace("{bin}", bin), e.description.clone()))
                .collect(),
            options.width,
        );
    }
    text
}
fn globals(text: &mut String, program: &ProgramSchema, width: usize) {
    let mut entries_ = vec![];
    if program.builtins.help {
        entries_.push(("-h, --help".into(), "Show help".into()));
    }
    if program.version.is_some() {
        entries_.push(("-V, --version".into(), "Show version".into()));
    }
    if program.builtins.json {
        entries_.push(("--json".into(), "Write structured JSON".into()));
    }
    if program.builtins.quiet {
        entries_.push((
            "-q, --quiet".into(),
            "Suppress status and informational output".into(),
        ));
    }
    if program.completions {
        entries_.push((
            "--completions <shell>".into(),
            "Generate shell completions".into(),
        ));
    }
    entries(text, "Global options", entries_, width);
}
fn description(input: &InputSchema) -> String {
    let mut description = input.description.clone();
    let mut notes = vec![];
    if input.required {
        notes.push("required".into());
    }
    if !input.enum_values.is_empty() {
        notes.push(format!("choices: {}", input.enum_values.join(", ")));
    }
    if !input.env_vars.is_empty() {
        notes.push(format!("env: {}", input.env_vars.join(", ")));
    }
    if let Some(default) = &input.default_value
        && !input.sensitive
    {
        notes.push(format!("default: {default}"));
    }
    if input.deprecated.is_some() {
        notes.push("deprecated".into());
    }
    if !notes.is_empty() {
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(&format!("[{}]", notes.join("; ")));
    }
    description
}
fn entries(text: &mut String, title: &str, rows: Vec<(String, String)>, width: usize) {
    if rows.is_empty() {
        return;
    }
    text.push_str(&format!("\n{title}:\n"));
    let column = rows
        .iter()
        .map(|(label, _)| visible_width(label))
        .max()
        .unwrap_or(0)
        .min(width / 2);
    for (label, description) in rows {
        if description.is_empty() {
            text.push_str(&format!("  {label}\n"));
            continue;
        }
        let prefix = if visible_width(&label) > column {
            format!("  {label}\n{}", " ".repeat(column + 4))
        } else {
            format!(
                "  {label}{}",
                " ".repeat(column + 2 - visible_width(&label))
            )
        };
        text.push_str(&prefix);
        text.push_str(&wrap(
            &description,
            width.saturating_sub(column + 4).max(10),
            column + 4,
        ));
        text.push('\n');
    }
}
fn wrap(text: &str, width: usize, indent: usize) -> String {
    let mut result = String::new();
    let mut column = 0;
    for word in text.split_whitespace() {
        let len = visible_width(word);
        if column > 0 && column + 1 + len > width {
            result.push('\n');
            result.push_str(&" ".repeat(indent));
            column = 0;
        } else if column > 0 {
            result.push(' ');
            column += 1;
        }
        result.push_str(word);
        column += len;
    }
    result
}
pub fn shell_from_path(path: &str) -> Option<Shell> {
    std::path::Path::new(path)
        .file_name()?
        .to_str()?
        .parse()
        .ok()
}

fn kebab(name: &str) -> String {
    let mut output = String::new();
    let mut previous = None;
    for c in name.chars() {
        if c.is_ascii_uppercase()
            && previous.is_some_and(|p: char| p.is_ascii_lowercase() || p.is_ascii_digit())
        {
            output.push('-');
        }
        output.extend(c.to_lowercase());
        previous = Some(c);
    }
    output
}
