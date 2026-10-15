use crate::CliError;
use crate::Command;
use crate::Context;
use crate::Output;
use crate::Result;
use crate::command::CommandSchema;
use crate::command::Meta;
use crate::command::Next;
#[cfg(feature = "completion")]
use crate::completion::Shell;
#[cfg(feature = "completion")]
use crate::completion::{self};
use crate::config::ConfigSettings;
use crate::config::Manifest;
use crate::config::{self};
use crate::execution::Action;
use crate::execution::AsyncNext;
use crate::execution::Step;
use crate::help;
use crate::output::Verbosity;
use crate::parse::Occurrence;
use crate::parse::ParseOptions;
use crate::parse::{self};
use crate::resolve::{self};
use crate::runtime::NativeRuntime;
use crate::runtime::RunOptions;
use crate::runtime::RunResult;
use crate::runtime::Runtime;
use crate::schema::Input;
use crate::schema::InputSchema;
pub use dreamcli_core::command::Builtins;
pub use dreamcli_core::command::ProgramSchema;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

/// Hooks are native trait methods; implementations may hold their own synchronized state.
pub trait Plugin: Send + Sync {
    fn before_parse(&self, _argv: &mut Vec<String>) -> Result<()> {
        Ok(())
    }
    fn before_execute(&self, _context: &mut Context) -> Result<()> {
        Ok(())
    }
    fn after_execute(&self, _context: &Context) -> Result<()> {
        Ok(())
    }
    fn on_error(&self, _error: &CliError) {}
}
#[derive(Clone)]
pub struct Cli {
    schema: ProgramSchema,
    commands: Vec<Command>,
    default: Option<Command>,
    plugins: Vec<Arc<dyn Plugin>>,
    config: Option<ConfigSettings>,
}
impl std::fmt::Debug for Cli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cli")
            .field("schema", &self.schema)
            .finish_non_exhaustive()
    }
}
pub fn cli(name: &str) -> Cli {
    Cli {
        schema: ProgramSchema {
            name: name.into(),
            version: None,
            description: String::new(),
            commands: vec![],
            default: None,
            builtins: Builtins::default(),
            completions: false,
            flags: vec![],
        },
        commands: vec![],
        default: None,
        plugins: vec![],
        config: None,
    }
}
impl Cli {
    pub fn new(name: impl AsRef<str>) -> Self {
        cli(name.as_ref())
    }
    pub fn schema(&self) -> &ProgramSchema {
        &self.schema
    }
    pub fn definition(&self) -> serde_json::Value {
        crate::json_schema::definition(&self.schema)
    }
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.schema.description = description.into();
        self
    }
    pub fn description(self, description: impl Into<String>) -> Self {
        self.describe(description)
    }
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.schema.version = Some(version.into());
        self
    }
    pub fn manifest(mut self, manifest: Manifest) -> Self {
        if let Some(name) = manifest.name {
            self.schema.name = name;
        }
        if let Some(version) = manifest.version {
            self.schema.version = Some(version);
        }
        if let Some(description) = manifest.description {
            self.schema.description = description;
        }
        self
    }
    pub fn command(mut self, command: Command) -> Self {
        self.schema.commands.push(command.schema.clone());
        self.commands.push(command);
        self
    }
    pub fn default(mut self, command: Command) -> Self {
        self.schema.default = Some(command.schema.clone());
        self.default = Some(command);
        self
    }
    pub fn flag<T, P>(mut self, flag: Input<T, P>) -> Self {
        self.schema.flags.push(flag.into_schema());
        self
    }
    /// Register reusable CLI-wide flags. Include the same bundle in a command's
    /// `.inputs((globals, locals))` to bind them to its typed handler.
    pub fn inputs<I: crate::typed::FlagSet>(mut self, inputs: I) -> Self {
        inputs.visit(&mut |input| self.schema.flags.push(input));
        self
    }
    pub fn builtins(mut self, builtins: Builtins) -> Self {
        self.schema.builtins = builtins;
        self
    }
    #[cfg(feature = "completion")]
    pub fn completions(mut self) -> Self {
        self.schema.completions = true;
        self
    }
    pub fn config(mut self, settings: ConfigSettings) -> Self {
        self.config = Some(settings);
        self
    }
    pub fn plugin(mut self, plugin: impl Plugin + 'static) -> Self {
        self.plugins.push(Arc::new(plugin));
        self
    }
    pub fn validate(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        let mut reserved = vec![];
        if self.schema.builtins.help {
            reserved.extend(["help", "h"]);
        }
        if self.schema.builtins.json {
            reserved.push("json");
        }
        if self.schema.builtins.quiet {
            reserved.extend(["quiet", "q"]);
        }
        if self.schema.version.is_some() {
            reserved.extend(["version", "V"]);
        }
        if self.schema.completions {
            reserved.push("completions");
        }
        if self.config.is_some() {
            reserved.push("config");
        }
        fn check(command: &CommandSchema, reserved: &[&str]) -> Result<()> {
            command.validate()?;
            for flag in &command.flags {
                for name in std::iter::once(&flag.name)
                    .chain(flag.aliases.iter().map(|a| &a.name))
                    .chain(flag.negated_name.iter())
                {
                    if reserved.contains(&name.as_str()) {
                        return Err(CliError::new(
                            "RESERVED_FLAG",
                            format!("Flag '{name}' is owned by the CLI"),
                        ));
                    }
                }
            }
            for child in &command.commands {
                check(child, reserved)?;
            }
            Ok(())
        }
        let root = CommandSchema {
            name: self.schema.name.clone(),
            description: self.schema.description.clone(),
            aliases: vec![],
            hidden: false,
            flags: self.schema.flags.clone(),
            args: vec![],
            commands: vec![],
            examples: vec![],
        };
        check(&root, &reserved)?;
        for command in &self.schema.commands {
            for name in std::iter::once(&command.name).chain(&command.aliases) {
                if !names.insert(name) || (self.schema.completions && name == "completions") {
                    return Err(CliError::new(
                        "INVALID_SCHEMA",
                        format!("Duplicate or reserved command '{name}'"),
                    ));
                }
            }
            check(&with_flags(command.clone(), &self.schema.flags), &reserved)?;
        }
        if let Some(default) = &self.schema.default {
            check(&with_flags(default.clone(), &self.schema.flags), &reserved)?;
        }
        Ok(())
    }
    /// Process-free execution: all environment, stdin, prompts, and filesystem data are injected.
    pub fn execute(
        &self,
        argv: impl IntoIterator<Item = impl Into<String>>,
        options: RunOptions,
    ) -> RunResult {
        self.execute_inner(
            argv.into_iter().map(Into::into).collect(),
            options,
            None,
            None,
        )
    }
    /// Execute async actions and middleware with injected capabilities.
    /// The caller chooses an executor; futures may hold local, non-Send values.
    pub async fn execute_async(
        &self,
        argv: impl IntoIterator<Item = impl Into<String>>,
        options: RunOptions,
    ) -> RunResult {
        self.execute_inner_async(
            argv.into_iter().map(Into::into).collect(),
            options,
            None,
            None,
        )
        .await
    }
    /// Await native execution. Stdio, prompts, and filesystem preparation remain synchronous.
    pub async fn run_async(&self) -> ExitCode {
        self.run_os_with_async(
            std::env::args_os().skip(1),
            RunOptions::default(),
            &NativeRuntime,
        )
        .await
        .exit_code()
    }
    pub async fn run_with_async(
        &self,
        argv: impl IntoIterator<Item = impl Into<String>>,
        options: RunOptions,
        runtime: &dyn Runtime,
    ) -> RunResult {
        self.execute_inner_async(
            argv.into_iter().map(Into::into).collect(),
            options,
            Some(runtime),
            None,
        )
        .await
    }
    pub async fn run_os_with_async(
        &self,
        argv: impl IntoIterator<Item = impl Into<std::ffi::OsString>>,
        options: RunOptions,
        runtime: &dyn Runtime,
    ) -> RunResult {
        let (argv, error) = decode_os_args(argv);
        self.execute_inner_async(argv, options, Some(runtime), error)
            .await
    }
    /// Run with native stdio, named environment reads, and filesystem capabilities.
    /// Returning `ExitCode` lets an application's `main` perform normal Rust cleanup.
    pub fn run(&self) -> ExitCode {
        self.run_os_with(
            std::env::args_os().skip(1),
            RunOptions::default(),
            &NativeRuntime,
        )
        .exit_code()
    }
    pub fn run_with(
        &self,
        argv: impl IntoIterator<Item = impl Into<String>>,
        options: RunOptions,
        runtime: &dyn Runtime,
    ) -> RunResult {
        self.execute_inner(
            argv.into_iter().map(Into::into).collect(),
            options,
            Some(runtime),
            None,
        )
    }
    /// OS arguments are decoded explicitly; invalid Unicode becomes a parse error.
    pub fn run_os_with(
        &self,
        argv: impl IntoIterator<Item = impl Into<std::ffi::OsString>>,
        options: RunOptions,
        runtime: &dyn Runtime,
    ) -> RunResult {
        let (decoded, error) = decode_os_args(argv);
        self.execute_inner(decoded, options, Some(runtime), error)
    }
    fn execute_inner(
        &self,
        argv: Vec<String>,
        options: RunOptions,
        runtime: Option<&dyn Runtime>,
        initial_error: Option<CliError>,
    ) -> RunResult {
        let Prepared {
            output,
            resolved,
            pending,
        } = self.prepare(argv, options, runtime, initial_error);
        let run = pending.and_then(|pending| {
            if let Some(mut pending) = pending {
                if crate::execution::requires_async(&pending.steps, pending.action) {
                    return Err(crate::execution::async_required());
                }
                for plugin in &self.plugins {
                    plugin.before_execute(&mut pending.context)?;
                }
                Next {
                    steps: &pending.steps,
                    action: pending.action,
                }
                .run(&mut pending.context)?;
                for plugin in &self.plugins {
                    plugin.after_execute(&pending.context)?;
                }
            }
            Ok(())
        });
        self.finish_run(output, resolved, run)
    }
    async fn execute_inner_async(
        &self,
        argv: Vec<String>,
        options: RunOptions,
        runtime: Option<&dyn Runtime>,
        initial_error: Option<CliError>,
    ) -> RunResult {
        let Prepared {
            output,
            resolved,
            pending,
        } = self.prepare(argv, options, runtime, initial_error);
        let run = async {
            if let Some(mut pending) = pending? {
                crate::execution::validate_async(&pending.steps, pending.action)?;
                for plugin in &self.plugins {
                    plugin.before_execute(&mut pending.context)?;
                }
                AsyncNext {
                    steps: &pending.steps,
                    action: pending.action,
                }
                .run(&mut pending.context)
                .await?;
                for plugin in &self.plugins {
                    plugin.after_execute(&pending.context)?;
                }
            }
            Ok(())
        }
        .await;
        self.finish_run(output, resolved, run)
    }
    fn prepare(
        &self,
        mut argv: Vec<String>,
        mut options: RunOptions,
        runtime: Option<&dyn Runtime>,
        initial_error: Option<CliError>,
    ) -> Prepared<'_> {
        let mut failure = initial_error;
        options.help.case_parity = options.parse.case_parity;
        for plugin in &self.plugins {
            if failure.is_some() {
                break;
            }
            if let Err(error) = plugin.before_parse(&mut argv) {
                failure = Some(error);
                break;
            }
        }
        let mode_error = strip_modes(&mut argv, &self.schema.builtins, &mut options);
        if let Some(runtime) = runtime {
            options.output.tty = runtime.stdout_is_tty();
            options.output.color = options.output.tty && runtime.env("NO_COLOR").is_none();
        }
        let output = options
            .channel
            .as_ref()
            .map(|channel| channel.with_options(options.output.clone()))
            .unwrap_or_else(|| {
                runtime
                    .map(|r| r.output(options.output.clone()))
                    .unwrap_or_else(|| Output::capture(options.output.clone()))
            });
        let mut resolved = None;
        let run = (|| -> Result<Option<Pending<'_>>> {
            if let Some(error) = failure {
                return Err(error);
            }
            self.validate()?;
            if self.schema.version.is_some()
                && (parse::includes_before_separator(&argv, "--version")
                    || parse::includes_before_separator(&argv, "-V"))
            {
                output.raw_stdout(&format!(
                    "{}\n",
                    self.schema.version.as_deref().expect("version")
                ));
                return Ok(None);
            }
            if self.schema.builtins.help && argv.first().is_some_and(|s| s == "help") {
                argv.remove(0);
                argv.push("--help".into());
            }
            let help = self.schema.builtins.help
                && (parse::includes_before_separator(&argv, "--help")
                    || parse::includes_before_separator(&argv, "-h"));
            #[cfg(feature = "completion")]
            if self.schema.completions {
                if let Some(index) = argv
                    .iter()
                    .take_while(|s| *s != "--")
                    .position(|s| s == "--completions" || s.starts_with("--completions="))
                {
                    let shell = argv[index]
                        .split_once('=')
                        .map(|(_, s)| s.to_owned())
                        .or_else(|| argv.get(index + 1).filter(|s| !s.starts_with('-')).cloned());
                    let shell = shell.map(|s| s.parse()).unwrap_or_else(|| {
                        Ok(runtime
                            .and_then(|r| r.env("SHELL"))
                            .as_deref()
                            .and_then(help::shell_from_path)
                            .unwrap_or(Shell::Bash))
                    })?;
                    output.raw_stdout(&completion::generate_with_options(
                        &self.schema,
                        shell,
                        options.parse,
                    ));
                    return Ok(None);
                }
                if argv.first().is_some_and(|s| s == "completions") {
                    if help {
                        output.raw_stdout("Usage: completions <bash|zsh|fish|powershell>\n");
                        return Ok(None);
                    }
                    let shell = argv
                        .get(1)
                        .ok_or_else(|| {
                            CliError::parse("MISSING_VALUE", "Specify a completion shell")
                        })?
                        .parse()?;
                    if argv.len() != 2 {
                        return Err(CliError::parse(
                            "UNEXPECTED_POSITIONAL",
                            "Unexpected completion argument",
                        ));
                    }
                    output.raw_stdout(&completion::generate_with_options(
                        &self.schema,
                        shell,
                        options.parse,
                    ));
                    return Ok(None);
                }
            }
            if help {
                argv = parse::strip_before_separator(
                    &parse::strip_before_separator(&argv, "--help"),
                    "-h",
                );
            }
            let explicit_config = if self.config.is_some() {
                match extract_config(&mut argv) {
                    Ok(path) => path,
                    Err(_) if help => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            if !help && let Some(error) = mode_error {
                return Err(error);
            }
            let invocation = match self.invocation(&argv, options.parse) {
                Err(error) if help && error.code == "UNKNOWN_FLAG" => None,
                result => result?,
            };
            let Some(invocation) = invocation else {
                if options.output.json {
                    output.json(&self.definition())?;
                } else {
                    output.raw_stdout(&help::root_help(&self.schema, &options.help));
                }
                return Ok(None);
            };
            if help
                || (invocation.command.action.is_none()
                    && !invocation.command.children.is_empty()
                    && invocation.argv.is_empty())
            {
                if options.output.json {
                    if invocation.path.is_empty() {
                        output.json(&self.definition())?;
                    } else {
                        output.json(&crate::json_schema::command_definition(&invocation.schema))?;
                    }
                    return Ok(None);
                }
                output.raw_stdout(&help::command_help(
                    &invocation.schema,
                    &format!(
                        "{}{}",
                        self.schema.name,
                        if invocation.path.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", invocation.path)
                        }
                    ),
                    &options.help,
                    Some(&self.schema),
                ));
                return Ok(None);
            }
            let parsed = parse::parse(&invocation.schema, &invocation.argv, options.parse)?;
            if let Some(runtime) = runtime {
                for input in invocation
                    .schema
                    .flags
                    .iter()
                    .chain(&invocation.schema.args)
                {
                    for name in &input.env_vars {
                        if !options.resolve.env.contains_key(name)
                            && let Some(value) = runtime.env(name)
                        {
                            options.resolve.env.insert(name.clone(), value);
                        }
                    }
                }
                if options.resolve.filesystem.is_none() {
                    options.resolve.filesystem = Some(runtime.filesystem());
                }
                #[cfg(feature = "terminal-prompts")]
                if options.resolve.prompter.is_none() && runtime.stdin_is_tty() {
                    options.resolve.prompter = Some(Arc::new(crate::prompt::TerminalPrompter));
                }
                if options.resolve.stdin.is_none()
                    && !runtime.stdin_is_tty()
                    && needs_stdin(&invocation.schema, &parsed)
                {
                    options.resolve.stdin = Some(runtime.read_stdin()?);
                }
                if let Some(settings) = &self.config
                    && (options.resolve.config.is_null() || explicit_config.is_some())
                {
                    let dirs = runtime.config_directories();
                    let paths = settings.candidates_in(&runtime.cwd()?, &dirs.user, &dirs.system);
                    if let Some(found) = config::discover(
                        settings,
                        runtime.reader(),
                        &paths,
                        explicit_config.as_deref().map(Path::new),
                    )? {
                        options.resolve.config = found.value;
                    }
                }
            } else if explicit_config.is_some() {
                return Err(CliError::new(
                    "NO_CONFIG_READER",
                    "Use run_with to load a config file, or inject parsed config into execute",
                ));
            }
            if options.resolve.interactive.is_none() {
                options.resolve.interactive = invocation.command.interactive.clone();
            }
            let inputs = resolve::resolve(&invocation.schema, &parsed, &options.resolve)?;
            for warning in &inputs.deprecations {
                output.warn(warning);
            }
            resolved = Some(inputs.clone());
            let context = Context::new(
                inputs,
                output.clone(),
                Meta {
                    name: self.schema.name.clone(),
                    version: self.schema.version.clone(),
                    command: if invocation.path.is_empty() {
                        invocation.command.schema.name.clone()
                    } else {
                        invocation.path
                    },
                },
            );
            Ok(Some(Pending {
                context,
                steps: invocation.steps,
                action: invocation.command.action.as_ref(),
            }))
        })();
        Prepared {
            output,
            resolved,
            pending: run,
        }
    }
    fn finish_run(
        &self,
        output: Output,
        resolved: Option<crate::Resolved>,
        run: Result<()>,
    ) -> RunResult {
        let error = run.err().or_else(|| output.finish().err());
        if let Some(error) = &error {
            for plugin in &self.plugins {
                plugin.on_error(error);
            }
            output.render_error(error);
        }
        let capture = output.captured();
        RunResult {
            exit_code: error.as_ref().map_or(output.exit_code(), |e| e.exit_code),
            stdout: capture.stdout,
            stderr: capture.stderr,
            error,
            activities: output.events(),
            skipped_checks: resolved
                .as_ref()
                .map(|r| r.skipped_checks.clone())
                .unwrap_or_default(),
            resolved,
        }
    }
    fn invocation<'a>(
        &'a self,
        argv: &[String],
        options: ParseOptions,
    ) -> Result<Option<Invocation<'a>>> {
        let mut rest = argv.to_vec();
        let mut children = self.commands.as_slice();
        let mut selected = self.default.as_ref();
        let mut inherited = self.schema.flags.clone();
        let mut steps = vec![];
        let mut path = String::new();
        let mut matched = false;
        loop {
            let mut flags = inherited.clone();
            if let Some(command) = selected {
                flags.extend(command.schema.flags.clone());
            }
            // Arity-aware scanning prevents a flag operand from being mistaken for a command.
            for child in children {
                flags.extend(child.schema.flags.clone());
            }
            let index = command_index(&rest, &flags, options);
            let Some(index) = index else {
                break;
            };
            let name = &rest[index];
            if let Some(command) = children
                .iter()
                .find(|c| c.schema.name == *name || c.schema.aliases.contains(name))
            {
                rest.remove(index);
                matched = true;
                if !path.is_empty() {
                    path.push(' ');
                }
                path.push_str(&command.schema.name);
                if let Some(parent) = selected.filter(|_| !path.eq(&command.schema.name)) {
                    inherited.extend(parent.schema.flags.iter().filter(|f| f.propagate).cloned());
                    steps.extend(parent.steps.clone());
                }
                selected = Some(command);
                children = &command.children;
                if children.is_empty() {
                    break;
                }
            } else if selected
                .is_some_and(|c| !c.schema.args.is_empty() || (matched && c.action.is_some()))
            {
                break;
            } else {
                let suggestion = parse::suggest_command(
                    name,
                    children
                        .iter()
                        .filter(|c| !c.schema.hidden)
                        .map(|c| c.schema.name.as_str()),
                );
                let mut error =
                    CliError::parse("UNKNOWN_COMMAND", format!("Unknown command '{name}'"));
                if let Some(suggestion) = suggestion {
                    error = error.suggest(suggestion);
                }
                return Err(error);
            }
        }
        let Some(command) = selected else {
            if rest.iter().any(|s| s.starts_with('-') && s != "--") {
                return Err(CliError::parse("UNKNOWN_FLAG", "Unknown root flag"));
            }
            return Ok(None);
        };
        steps.extend(command.steps.clone());
        Ok(Some(Invocation {
            command,
            schema: with_flags(command.schema.clone(), &inherited),
            argv: rest,
            path,
            steps,
        }))
    }
}
struct Invocation<'a> {
    command: &'a Command,
    schema: CommandSchema,
    argv: Vec<String>,
    path: String,
    steps: Vec<Step>,
}
fn with_flags(mut schema: CommandSchema, inherited: &[InputSchema]) -> CommandSchema {
    let mut flags: Vec<_> = inherited
        .iter()
        .filter(|f| !schema.flags.iter().any(|local| local.name == f.name))
        .cloned()
        .collect();
    flags.extend(schema.flags);
    schema.flags = flags;
    schema
}
fn command_index(argv: &[String], flags: &[InputSchema], options: ParseOptions) -> Option<usize> {
    let lookup = parse::flag_lookup(flags, options);
    let mut i = 0;
    while let Some(raw) = argv.get(i) {
        if raw == "--" {
            return None;
        }
        if let Some(raw) = raw.strip_prefix("--") {
            if !raw.contains('=')
                && lookup
                    .get(raw)
                    .is_some_and(|(s, negated)| s.expects_value() && !negated)
            {
                i += 1;
            }
        } else if raw.starts_with('-') && raw != "-" {
            for (offset, c) in raw[1..].char_indices() {
                if lookup
                    .get(&c.to_string())
                    .is_some_and(|(s, _)| s.expects_value())
                {
                    if offset + c.len_utf8() == raw.len() - 1 {
                        i += 1;
                    }
                    break;
                }
            }
        } else {
            return Some(i);
        }
        i += 1;
    }
    None
}
fn strip_modes(
    argv: &mut Vec<String>,
    builtins: &Builtins,
    options: &mut RunOptions,
) -> Option<CliError> {
    let mut error = None;
    let mut past = false;
    argv.retain(|arg| {
        if arg == "--" {
            past = true;
        }
        if past {
            return true;
        }
        let (name, value) = arg
            .split_once('=')
            .map(|(n, v)| (n, Some(v)))
            .unwrap_or((arg, None));
        let target = if builtins.json && name == "--json" {
            Some(true)
        } else if builtins.quiet && (name == "--quiet" || arg == "-q") {
            Some(false)
        } else {
            None
        };
        let Some(json) = target else {
            return true;
        };
        let value = match value {
            None | Some("true" | "1") => true,
            Some("false" | "0") => false,
            _ => {
                error = Some(CliError::parse(
                    "INVALID_VALUE",
                    format!("Invalid boolean value for {name}"),
                ));
                false
            }
        };
        if json {
            options.output.json = value;
        } else {
            options.output.verbosity = if value {
                Verbosity::Quiet
            } else {
                Verbosity::Normal
            };
        }
        false
    });
    error
}
fn extract_config(argv: &mut Vec<String>) -> Result<Option<String>> {
    let mut i = 0;
    let mut path = None;
    while i < argv.len() {
        if argv[i] == "--" {
            break;
        }
        if let Some(value) = argv[i].strip_prefix("--config=") {
            path = Some(value.into());
            argv.remove(i);
        } else if argv[i] == "--config" {
            argv.remove(i);
            if i >= argv.len() || argv[i].starts_with('-') {
                return Err(CliError::parse("MISSING_VALUE", "--config requires a path"));
            }
            path = Some(argv.remove(i));
        } else {
            i += 1;
        }
    }
    Ok(path)
}
fn needs_stdin(schema: &CommandSchema, parsed: &parse::Parsed) -> bool {
    schema.flags.iter().chain(&schema.args).any(|input| {
        let Some(binding) = input.stdin else {
            return false;
        };
        let occurrences = match input.surface {
            crate::schema::Surface::Flag => &parsed.flag_occurrences,
            crate::schema::Surface::Arg => &parsed.arg_occurrences,
        }
        .get(&input.name);
        occurrences.is_some_and(|o| o.iter().any(|o| matches!(o, Occurrence::Stdin)))
            || (binding.reads_missing() && occurrences.is_none_or(Vec::is_empty))
    })
}

struct Pending<'a> {
    context: Context,
    steps: Vec<Step>,
    action: Option<&'a Action>,
}
struct Prepared<'a> {
    output: Output,
    resolved: Option<crate::Resolved>,
    pending: Result<Option<Pending<'a>>>,
}
fn decode_os_args(
    argv: impl IntoIterator<Item = impl Into<std::ffi::OsString>>,
) -> (Vec<String>, Option<CliError>) {
    let mut decoded = vec![];
    let mut error = None;
    for argument in argv {
        match argument.into().into_string() {
            Ok(argument) => decoded.push(argument),
            Err(_) => {
                error.get_or_insert_with(|| {
                    CliError::parse("INVALID_UTF8", "CLI arguments must be valid UTF-8")
                });
            }
        }
    }
    (decoded, error)
}
