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
}
impl Default for Builtins {
    fn default() -> Self {
        Self {
            help: true,
            json: true,
            quiet: true,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct ProgramSchema {
    pub name: String,
    pub version: Option<String>,
    pub description: String,
    pub commands: Vec<CommandSchema>,
    pub default: Option<CommandSchema>,
    pub builtins: Builtins,
    pub completions: bool,
    pub flags: Vec<InputSchema>,
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
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.name.chars().any(char::is_whitespace) {
            return Err(CliError::new(
                "INVALID_SCHEMA",
                "Command names must be non-empty and contain no whitespace",
            ));
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
            for spelling in std::iter::once(flag.name.as_str())
                .chain(flag.aliases.iter().map(|a| a.name.as_str()))
                .chain(flag.negated_name.as_deref())
            {
                if spelling.is_empty()
                    || spelling.chars().any(|c| c.is_whitespace() || c == '=')
                    || spelling.starts_with('-')
                    || !names.insert(spelling)
                {
                    return Err(CliError::new(
                        "DUPLICATE_FLAG",
                        format!("Invalid or colliding flag spelling '{spelling}'"),
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
            let mut effective = command.clone();
            for flag in self.flags.iter().filter(|f| f.propagate) {
                if !effective.flags.iter().any(|f| f.name == flag.name) {
                    effective.flags.insert(0, flag.clone());
                }
            }
            effective.validate()?;
        }
        Ok(())
    }
}
