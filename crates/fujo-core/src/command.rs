use crate::CliError;
use crate::Result;
use crate::schema::InputSchema;
use crate::schema::StdinConsume;
use crate::schema::Surface;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Builtins {
    pub help: bool,
    pub json: bool,
    pub quiet: bool,
    pub explain: bool,
}
impl Default for Builtins {
    fn default() -> Self {
        Self {
            help: true,
            json: true,
            quiet: true,
            explain: false,
        }
    }
}
impl Builtins {
    pub const fn none() -> Self {
        Self {
            help: false,
            json: false,
            quiet: false,
            explain: false,
        }
    }

    pub fn global_flags(&self, version: bool, completions: bool, config: bool) -> Vec<GlobalFlag> {
        [
            self.help.then_some(GlobalFlag {
                long: "--help",
                short: Some("-h"),
                value: None,
                description: "Show help",
            }),
            version.then_some(GlobalFlag {
                long: "--version",
                short: Some("-V"),
                value: None,
                description: "Show version",
            }),
            self.json.then_some(GlobalFlag {
                long: "--json",
                short: None,
                value: None,
                description: "Write structured JSON",
            }),
            self.quiet.then_some(GlobalFlag {
                long: "--quiet",
                short: Some("-q"),
                value: None,
                description: "Suppress status and informational output",
            }),
            self.explain.then_some(GlobalFlag {
                long: "--explain",
                short: None,
                value: None,
                description: "List the stages of a command without running it",
            }),
            completions.then_some(GlobalFlag {
                long: "--completions",
                short: None,
                value: Some(GlobalValue::Shell),
                description: "Generate shell completions",
            }),
            config.then_some(GlobalFlag {
                long: "--config",
                short: None,
                value: Some(GlobalValue::Path),
                description: "Read configuration from this file",
            }),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}
pub const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "powershell"];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalValue {
    Shell,
    Path,
}
impl GlobalValue {
    pub fn hint(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Path => "path",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalFlag {
    pub long: &'static str,
    pub short: Option<&'static str>,
    pub value: Option<GlobalValue>,
    pub description: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProgramSchema {
    pub name: String,
    pub version: Option<String>,
    /// Version printed in completion headers, independent of the version flag.
    pub completion_version: Option<String>,
    pub description: String,
    pub commands: Vec<CommandSchema>,
    pub default: Option<CommandSchema>,
    pub builtins: Builtins,
    pub completions: bool,
    pub config: bool,
    pub flags: Vec<InputSchema>,
}

impl ProgramSchema {
    /// Construct a schema without runtime-provided flags.
    ///
    /// ```
    /// use fujo_core::{arg, flag};
    /// use fujo_core::command::{CommandSchema, ProgramSchema};
    /// let program = ProgramSchema::new("formatter").completion_version("1.2.3")
    ///     .default(CommandSchema::new("format")
    ///         .flag(flag::boolean("help").alias("h").into_schema())
    ///         .arg(arg::path("files").variadic().optional().into_schema()));
    /// program.validate().unwrap();
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            completion_version: None,
            description: String::new(),
            commands: vec![],
            default: None,
            builtins: Builtins::none(),
            completions: false,
            config: false,
            flags: vec![],
        }
    }
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
    pub fn completion_version(mut self, version: impl Into<String>) -> Self {
        self.completion_version = Some(version.into());
        self
    }
    pub fn builtins(mut self, builtins: Builtins) -> Self {
        self.builtins = builtins;
        self
    }
    pub fn completions(mut self, enabled: bool) -> Self {
        self.completions = enabled;
        self
    }
    pub fn config(mut self, enabled: bool) -> Self {
        self.config = enabled;
        self
    }
    pub fn flag(mut self, flag: InputSchema) -> Self {
        self.flags.push(flag);
        self
    }
    pub fn command(mut self, command: CommandSchema) -> Self {
        self.commands.push(command);
        self
    }
    pub fn default(mut self, command: CommandSchema) -> Self {
        self.default = Some(command);
        self
    }
    pub fn validate(&self) -> Result<()> {
        let root = CommandSchema::new(&self.name);
        let root = with_flags(root, &self.flags);
        root.validate()?;
        if let Some(default) = &self.default {
            with_flags(default.clone(), &self.flags).validate()?;
        }
        let mut tree = root;
        tree.flags = self
            .flags
            .iter()
            .cloned()
            .map(InputSchema::propagated)
            .collect();
        tree.commands = self.commands.clone();
        tree.validate()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Example {
    pub command: String,
    pub description: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSchema {
    pub name: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub hidden: bool,
    pub flags: Vec<InputSchema>,
    pub args: Vec<InputSchema>,
    pub commands: Vec<CommandSchema>,
    pub examples: Vec<Example>,
}
impl CommandSchema {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            aliases: vec![],
            hidden: false,
            flags: vec![],
            args: vec![],
            commands: vec![],
            examples: vec![],
        }
    }
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }
    pub fn flag(mut self, flag: InputSchema) -> Self {
        self.flags.push(flag);
        self
    }
    pub fn arg(mut self, arg: InputSchema) -> Self {
        self.args.push(arg);
        self
    }
    pub fn command(mut self, command: CommandSchema) -> Self {
        self.commands.push(command);
        self
    }
    pub fn example(mut self, command: impl Into<String>, description: impl Into<String>) -> Self {
        self.examples.push(Example {
            command: command.into(),
            description: description.into(),
        });
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.name.chars().any(char::is_whitespace) {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Command names must be non-empty and contain no whitespace",
            ));
        }
        for (i, input) in self.flags.iter().chain(&self.args).enumerate() {
            if self
                .flags
                .iter()
                .chain(&self.args)
                .take(i)
                .any(|earlier| earlier.conflicts_with(input))
            {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    format!("Input '{}' has conflicting declarations", input.name),
                )
                .suggest("Reuse clones of the same input handle when composing declarations"));
            }
        }
        for input in self.flags.iter().chain(&self.args) {
            for target in input.conflicts.iter().chain(&input.requires) {
                if (target.name == input.name && target.surface == input.surface)
                    || !self
                        .flags
                        .iter()
                        .chain(&self.args)
                        .any(|other| other.name == target.name && other.surface == target.surface)
                {
                    return Err(CliError::new(
                        "INVALID_SCHEMA",
                        format!(
                            "Invalid relationship target '{}' for {}",
                            target.name,
                            input.display_name()
                        ),
                    ));
                }
            }
        }
        let mut names = HashSet::new();
        for flag in &self.flags {
            flag.validate_definition()?;
            if flag.surface != Surface::Flag {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "A positional input was registered as a flag",
                ));
            }
            for spelling in flag.spellings() {
                if !names.insert(spelling) {
                    return Err(CliError::new(
                        "INVALID_SCHEMA",
                        format!("Colliding flag spelling '{spelling}'"),
                    ));
                }
            }
        }
        let mut optional = false;
        let mut arg_names = HashSet::new();
        for (i, arg) in self.args.iter().enumerate() {
            arg.validate_definition()?;
            if arg.surface != Surface::Arg || !arg_names.insert(&arg.name) {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "Invalid or duplicated argument",
                ));
            }
            if arg.variadic && i + 1 != self.args.len() {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "A variadic argument must be last",
                ));
            }
            if optional && arg.required {
                return Err(CliError::new(
                    "INVALID_SCHEMA",
                    "Required arguments cannot follow optional arguments",
                ));
            }
            optional |= !arg.required;
        }
        let consumers: Vec<_> = self
            .flags
            .iter()
            .chain(&self.args)
            .filter_map(|s| s.stdin)
            .collect();
        if consumers.len() > 1
            && consumers
                .iter()
                .any(|s| s.consume == StdinConsume::Exclusive)
        {
            return Err(CliError::new(
                "DUPLICATE_STDIN_INPUT",
                "Multiple stdin inputs must all use broadcast consumption",
            ));
        }
        let mut commands = HashSet::new();
        for command in &self.commands {
            for name in std::iter::once(&command.name).chain(&command.aliases) {
                if !commands.insert(name) {
                    return Err(CliError::new(
                        "INVALID_SCHEMA",
                        format!("Duplicate command '{name}'"),
                    ));
                }
            }
            let propagated: Vec<_> = self.flags.iter().filter(|f| f.propagate).cloned().collect();
            with_flags(command.clone(), &propagated).validate()?;
        }
        Ok(())
    }
}
pub fn with_flags(mut schema: CommandSchema, inherited: &[InputSchema]) -> CommandSchema {
    let mut flags: Vec<InputSchema> = vec![];
    for input in inherited {
        let existing = schema
            .flags
            .iter_mut()
            .chain(flags.iter_mut())
            .find(|existing| existing.name == input.name && !existing.conflicts_with(input));
        if let Some(existing) = existing {
            // Sharing a clone at a narrower scope must not stop propagation.
            existing.declaration.propagate |= input.propagate;
        } else {
            flags.push(input.clone());
        }
    }
    flags.extend(schema.flags);
    schema.flags = flags;
    schema
}
