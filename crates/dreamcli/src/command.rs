use crate::CliError;
use crate::Output;
use crate::Result;
use crate::resolve::InputSources;
use crate::resolve::Resolved;
use crate::schema::ArgMarker;
use crate::schema::FlagMarker;
use crate::schema::Input;
use crate::schema::Presence;
pub use dreamcli_core::command::CommandSchema;
pub use dreamcli_core::command::Example;
use serde::Serialize;
use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Meta {
    pub name: String,
    pub version: Option<String>,
    pub command: String,
}

/// Resolved inputs, typed application state, output channel, and invocation metadata.
pub struct Context {
    pub inputs: Resolved,
    pub out: Output,
    pub meta: Meta,
    state: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}
impl Context {
    pub(crate) fn new(inputs: Resolved, out: Output, meta: Meta) -> Self {
        Self {
            inputs,
            out,
            meta,
            state: HashMap::new(),
        }
    }
    pub fn get<T: Clone + Send + Sync + 'static, P: Presence<T>, S>(
        &self,
        input: &Input<T, P, S>,
    ) -> Result<P::Output> {
        self.inputs.get(input)
    }
    pub fn get_ref<T: 'static, P: Presence<T>, S>(
        &self,
        input: &Input<T, P, S>,
    ) -> Result<P::Ref<'_>> {
        self.inputs.get_ref(input)
    }
    pub fn sources(&self) -> &InputSources {
        &self.inputs.sources
    }
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) {
        self.state.insert(TypeId::of::<T>(), Box::new(value));
    }
    pub fn state<T: Send + Sync + 'static>(&self) -> Result<&T> {
        self.state
            .get(&TypeId::of::<T>())
            .and_then(|v| v.downcast_ref())
            .ok_or_else(|| {
                CliError::new(
                    "MISSING_CONTEXT",
                    format!("Context does not contain {}", std::any::type_name::<T>()),
                )
            })
    }
}
pub(crate) type Action = Arc<dyn Fn(&mut Context) -> Result<()> + Send + Sync>;
pub(crate) type Middleware =
    Arc<dyn for<'a> Fn(&mut Context, Next<'a>) -> Result<()> + Send + Sync>;

/// Calling `run` executes the remainder once; dropping it short-circuits the action.
pub struct Next<'a> {
    pub(crate) steps: &'a [Middleware],
    pub(crate) action: Option<&'a Action>,
}
impl Next<'_> {
    pub fn run(self, ctx: &mut Context) -> Result<()> {
        if let Some((first, rest)) = self.steps.split_first() {
            first(
                ctx,
                Next {
                    steps: rest,
                    action: self.action,
                },
            )
        } else if let Some(action) = self.action {
            action(ctx)
        } else {
            Err(CliError::new("NO_ACTION", "Command has no action"))
        }
    }
}

#[derive(Clone)]
pub struct Command {
    pub(crate) schema: CommandSchema,
    pub(crate) children: Vec<Command>,
    pub(crate) steps: Vec<Middleware>,
    pub(crate) action: Option<Action>,
    pub(crate) interactive: Option<crate::resolve::Interactive>,
}
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Command")
            .field("schema", &self.schema)
            .finish_non_exhaustive()
    }
}
pub fn command(name: &str) -> Command {
    Command {
        schema: CommandSchema {
            name: name.into(),
            description: String::new(),
            aliases: vec![],
            hidden: false,
            flags: vec![],
            args: vec![],
            commands: vec![],
            examples: vec![],
        },
        children: vec![],
        steps: vec![],
        action: None,
        interactive: None,
    }
}
pub fn group(name: &str) -> Command {
    command(name)
}
impl Command {
    pub fn new(name: impl AsRef<str>) -> Self {
        command(name.as_ref())
    }
    pub fn schema(&self) -> &CommandSchema {
        &self.schema
    }
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.schema.description = description.into();
        self
    }
    pub fn description(self, description: impl Into<String>) -> Self {
        self.describe(description)
    }
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.schema.aliases.push(alias.into());
        self
    }
    pub fn hidden(mut self) -> Self {
        self.schema.hidden = true;
        self
    }
    pub fn flag<T, P>(mut self, flag: Input<T, P, FlagMarker>) -> Self {
        self.schema.flags.push(flag.into_schema());
        self
    }
    pub fn arg<T, P>(mut self, arg: Input<T, P, ArgMarker>) -> Self {
        self.schema.args.push(arg.into_schema());
        self
    }
    pub fn command(mut self, command: Command) -> Self {
        self.schema.commands.push(command.schema.clone());
        self.children.push(command);
        self
    }
    pub fn example(mut self, command: impl Into<String>, description: impl Into<String>) -> Self {
        self.schema.examples.push(Example {
            command: command.into(),
            description: description.into(),
        });
        self
    }
    pub fn action(
        mut self,
        action: impl Fn(&mut Context) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.action = Some(Arc::new(action));
        self
    }
    pub fn interactive(
        mut self,
        interactive: impl Fn(
            &Resolved,
        ) -> Result<
            std::collections::BTreeMap<String, Option<crate::prompt::Prompt>>,
        > + Send
        + Sync
        + 'static,
    ) -> Self {
        self.interactive = Some(Arc::new(interactive));
        self
    }
    pub fn middleware(
        mut self,
        middleware: impl for<'a> Fn(&mut Context, Next<'a>) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.steps.push(Arc::new(middleware));
        self
    }
    pub fn derive<T: Send + Sync + 'static>(
        self,
        derive: impl Fn(&Context) -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        self.middleware(move |ctx, next| {
            let value = derive(ctx)?;
            ctx.insert(value);
            next.run(ctx)
        })
    }
    pub fn validate(&self) -> Result<()> {
        self.schema.validate()
    }
}
