use crate::command::CommandSchema;
#[cfg(feature = "completion")]
use crate::completion::Shell;
use crate::schema::InputSchema;
use crate::typed::ActionInfo;
use crate::typed::StageInfo;
use fujo_core::command::ProgramSchema;
use fujo_output::visible_width;
use std::fmt::Write;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub enum FlagOrder {
    #[default]
    Alphabetical,
    Declaration,
}
type FlagComparator = Arc<dyn Fn(&str, &str) -> std::cmp::Ordering + Send + Sync>;

#[derive(Clone)]
pub struct HelpOptions {
    pub width: usize,
    pub case_parity: bool,
    pub flag_order: FlagOrder,
    pub sort_flags: Option<FlagComparator>,
}
impl std::fmt::Debug for HelpOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HelpOptions")
            .field("width", &self.width)
            .field("case_parity", &self.case_parity)
            .field("flag_order", &self.flag_order)
            .finish_non_exhaustive()
    }
}
impl HelpOptions {
    pub fn sort_flags(
        mut self,
        compare: impl Fn(&str, &str) -> std::cmp::Ordering + Send + Sync + 'static,
    ) -> Self {
        self.sort_flags = Some(Arc::new(compare));
        self
    }
}
impl Default for HelpOptions {
    fn default() -> Self {
        Self {
            width: 80,
            case_parity: true,
            flag_order: FlagOrder::default(),
            sort_flags: None,
        }
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
        let _ = writeln!(text, "\n{}", wrap(&program.description, options.width, 0));
    }
    let _ = writeln!(text, "\nUsage: {} <command> [options]", program.name);
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
    let mut flags: Vec<_> = command.flags.iter().filter(|f| !f.hidden).collect();
    if let Some(compare) = &options.sort_flags {
        flags.sort_by(|a, b| compare(&a.name, &b.name));
    } else if matches!(options.flag_order, FlagOrder::Alphabetical) {
        flags.sort_by_key(|flag| {
            (
                !flag
                    .aliases
                    .iter()
                    .any(|a| !a.hidden && a.name.chars().count() == 1),
                flag.name.as_str(),
            )
        });
    }
    let lookup = crate::parse::flag_lookup(
        &command.flags,
        crate::parse::ParseOptions {
            case_parity: options.case_parity,
        },
    );
    let mut text = format!("{bin}\n");
    if !command.description.is_empty() {
        let _ = writeln!(text, "\n{}", wrap(&command.description, options.width, 0));
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
    let _ = writeln!(
        text,
        "\nUsage: {bin}{args}{}{}",
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
    );
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
        flags
            .into_iter()
            .map(|flag| {
                let names = flag
                    .aliases
                    .iter()
                    .filter(|a| !a.hidden)
                    .map(|a| crate::parse::spelling(&a.name))
                    .chain(std::iter::once(format!(
                        "--{}",
                        crate::parse::canonical_flag_name_with(&lookup, flag)
                    )))
                    .chain(flag.negated_name.as_deref().map(crate::parse::spelling))
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
    let rows = program
        .builtins
        .global_flags(
            program.version.is_some(),
            program.completions,
            program.config,
        )
        .into_iter()
        .map(|flag| {
            let mut label = String::new();
            if let Some(short) = flag.short {
                let _ = write!(label, "{short}, ");
            }
            label.push_str(flag.long);
            if let Some(value) = flag.value {
                let _ = write!(label, " <{}>", value.hint());
            }
            (label, flag.description.into())
        })
        .collect();
    entries(text, "Global options", rows, width);
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
    if input.has_default() {
        match &input.default_description {
            Some(crate::schema::DefaultDescription::Text(text)) => {
                notes.push(format!("default: {text}"))
            }
            Some(crate::schema::DefaultDescription::Hidden) => {}
            None => {
                if let Some(default) = input.default_json()
                    && !input.sensitive
                {
                    notes.push(format!("default: {default}"));
                }
            }
        }
    }
    if input.deprecated.is_some() {
        notes.push("deprecated".into());
    }
    if !notes.is_empty() {
        if !description.is_empty() {
            description.push(' ');
        }
        let _ = write!(description, "[{}]", notes.join("; "));
    }
    description
}
fn entries(text: &mut String, title: &str, rows: Vec<(String, String)>, width: usize) {
    if rows.is_empty() {
        return;
    }
    let _ = writeln!(text, "\n{title}:");
    let column = rows
        .iter()
        .map(|(label, _)| visible_width(label))
        .max()
        .unwrap_or(0)
        .min(width / 2);
    for (label, description) in rows {
        if description.is_empty() {
            let _ = writeln!(text, "  {label}");
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
#[cfg(feature = "completion")]
pub fn shell_from_path(path: &str) -> Option<Shell> {
    Shell::from_path(path)
}

pub(crate) fn explain(path: &str, stages: &[StageInfo], action: Option<&ActionInfo>) -> String {
    let mut text = format!("Stages of {path}:\n");
    for stage in stages {
        let _ = writeln!(
            text,
            "  {:<10} {:<5} {}",
            format!("{:?}", stage.kind).to_lowercase(),
            if stage.asynchronous { "async" } else { "sync" },
            stage.handler
        );
    }
    match action {
        Some(action) => {
            let _ = writeln!(
                text,
                "  {:<10} {:<5} {}",
                "action",
                if action.asynchronous { "async" } else { "sync" },
                action.handler
            );
        }
        None => text.push_str("  no action\n"),
    }
    text
}
