//! Command-bound inputs and a typed state transformation pipeline.
//!
//! ```
//! use fujo::{Cli, Command, arg, flag};
//! let command = Command::new("build")
//!     .inputs((arg::path("root"), flag::parse::<u16>("jobs").default(4)))
//!     .derive(|_, (root, jobs), ()| Ok((root.clone(), *jobs)))
//!     .action(|ctx, _, (root, jobs)| {
//!         ctx.out.log(format!("{}: {jobs} jobs", root.display()));
//!         Ok(())
//!     });
//! assert!(Cli::new("build").default(command).execute(["."], Default::default()).success());
//! ```
//!
//! Typed scopes expose only output, metadata, and provenance. An unrelated handle
//! cannot be read through the scope, including an optional handle:
//! ```compile_fail
//! use fujo::{Command, flag};
//! let unrelated = flag::string("missing");
//! let command = Command::new("demo").inputs(()).action(move |ctx, (), ()| {
//!     let _ = ctx.get(&unrelated)?;
//!     Ok(())
//! });
//! ```
//!
//! Input types and bundle shape come from registration:
//! ```compile_fail
//! use fujo::{Command, flag};
//! let command = Command::new("demo").inputs(flag::parse::<u16>("port").required())
//!     .action(|_, port: &String, ()| Ok(()));
//! ```
//! ```compile_fail
//! use fujo::{Command, flag};
//! let command = Command::new("demo").inputs((flag::boolean("force"),))
//!     .action(|_, (force, missing), ()| Ok(()));
//! ```
//!
//! State availability is established by the derivation chain:
//! ```compile_fail
//! use fujo::Command;
//! struct Credentials;
//! let command = Command::new("demo").inputs(())
//!     .action(|_, (), credentials: &Credentials| Ok(()));
//! ```
//! ```compile_fail
//! use fujo::{Cli, arg};
//! let app = Cli::new("demo").inputs(arg::string("positional"));
//! ```
//!
//! Derived state cannot retain references into the invocation's input storage:
//! ```compile_fail
//! use fujo::{Command, arg};
//! let command = Command::new("demo").inputs(arg::string("name"))
//!     .derive(|_, name, ()| Ok(name));
//! ```
mod inputs;
use crate::Command;
use crate::Context;
use crate::InputSources;
use crate::Meta;
use crate::Output;
use crate::Result;
use crate::execution::Action;
use crate::execution::AsyncAction;
use crate::execution::LocalFuture;
use crate::schema::Surface;
pub use inputs::FlagSet;
pub use inputs::InputSet;
use std::marker::PhantomData;
use std::sync::Arc;

/// Capabilities available to a typed callback. Values and state are separate,
/// statically bound parameters, with no arbitrary handle or TypeId lookup.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub out: &'a Output,
    pub meta: &'a Meta,
    pub sources: &'a InputSources,
}
fn scope(ctx: &Context) -> Scope<'_> {
    Scope {
        out: &ctx.out,
        meta: &ctx.meta,
        sources: &ctx.inputs.sources,
    }
}

/// Builder retaining registration and state types until an action is bound.
/// Each derive consumes the preceding state and returns its successor. Return a
/// struct or tuple containing earlier state when subsequent stages need both.
pub struct TypedCommand<I: InputSet, S: 'static = ()> {
    command: Command,
    inputs: Arc<I>,
    state: Arc<dyn StateFactory<S>>,
}
impl Command {
    /// Register a complete input bundle and bind its shape to subsequent callbacks.
    pub fn inputs<I: InputSet>(mut self, inputs: I) -> TypedCommand<I> {
        inputs.visit(&mut |input| match input.surface {
            Surface::Flag => self.schema.flags.push(input),
            Surface::Arg => self.schema.args.push(input),
        });
        TypedCommand {
            command: self,
            inputs: Arc::new(inputs),
            state: Arc::new(Initial),
        }
    }
}
impl<I: InputSet, S: 'static> TypedCommand<I, S> {
    pub fn derive<T: 'static, F>(self, derive: F) -> TypedCommand<I, T>
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync + 'static,
    {
        TypedCommand {
            command: self.command,
            inputs: self.inputs.clone(),
            state: Arc::new(Derived {
                inputs: self.inputs,
                previous: self.state,
                handler: derive,
                output: PhantomData,
            }),
        }
    }
    pub fn derive_async<T: 'static, F>(self, derive: F) -> TypedCommand<I, T>
    where
        F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync + 'static,
    {
        TypedCommand {
            command: self.command,
            inputs: self.inputs.clone(),
            state: Arc::new(AsyncDerived {
                inputs: self.inputs,
                previous: self.state,
                handler: derive,
                output: PhantomData,
            }),
        }
    }
    pub fn action<F>(mut self, action: F) -> Command
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync + 'static,
    {
        if self.state.is_async() {
            self.command.action = Some(Action::Async(Arc::new(BoundSync {
                inputs: self.inputs,
                state: self.state,
                handler: action,
            })));
            self.command
        } else {
            self.command.action(move |ctx| {
                let state = self.state.sync(ctx)?;
                action(scope(ctx), self.inputs.read(&ctx.inputs)?, &state)
            })
        }
    }
    pub fn action_async<F>(mut self, action: F) -> Command
    where
        F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync + 'static,
    {
        self.command.action = Some(Action::Async(Arc::new(BoundAsync {
            inputs: self.inputs,
            state: self.state,
            handler: action,
        })));
        self.command
    }
}

trait StateFactory<S>: Send + Sync {
    fn is_async(&self) -> bool;
    fn sync(&self, ctx: &Context) -> Result<S>;
    fn build<'a>(&'a self, ctx: &'a Context) -> LocalFuture<'a, Result<S>>;
}
struct Initial;
impl StateFactory<()> for Initial {
    fn is_async(&self) -> bool {
        false
    }
    fn sync(&self, _: &Context) -> Result<()> {
        Ok(())
    }
    fn build<'a>(&'a self, _: &'a Context) -> LocalFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
struct Derived<I, S, T, F> {
    inputs: Arc<I>,
    previous: Arc<dyn StateFactory<S>>,
    handler: F,
    output: PhantomData<fn() -> T>,
}
impl<I: InputSet, S: 'static, T: 'static, F> StateFactory<T> for Derived<I, S, T, F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync,
{
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn sync(&self, ctx: &Context) -> Result<T> {
        let previous = self.previous.sync(ctx)?;
        (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, previous)
    }
    fn build<'a>(&'a self, ctx: &'a Context) -> LocalFuture<'a, Result<T>> {
        Box::pin(async move {
            let previous = self.previous.build(ctx).await?;
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, previous)
        })
    }
}
struct AsyncDerived<I, S, T, F> {
    inputs: Arc<I>,
    previous: Arc<dyn StateFactory<S>>,
    handler: F,
    output: PhantomData<fn() -> T>,
}
impl<I: InputSet, S: 'static, T: 'static, F> StateFactory<T> for AsyncDerived<I, S, T, F>
where
    F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync,
{
    fn is_async(&self) -> bool {
        true
    }
    fn sync(&self, _: &Context) -> Result<T> {
        Err(crate::execution::async_required())
    }
    fn build<'a>(&'a self, ctx: &'a Context) -> LocalFuture<'a, Result<T>> {
        Box::pin(async move {
            let previous = self.previous.build(ctx).await?;
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, previous).await
        })
    }
}
struct BoundSync<I, S, F> {
    inputs: Arc<I>,
    state: Arc<dyn StateFactory<S>>,
    handler: F,
}
impl<I: InputSet, S: 'static, F> AsyncAction for BoundSync<I, S, F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync,
{
    fn call<'a>(&'a self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            let state = self.state.build(ctx).await?;
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &state)
        })
    }
}
struct BoundAsync<I, S, F> {
    inputs: Arc<I>,
    state: Arc<dyn StateFactory<S>>,
    handler: F,
}
impl<I: InputSet, S: 'static, F> AsyncAction for BoundAsync<I, S, F>
where
    F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync,
{
    fn call<'a>(&'a self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            let state = self.state.build(ctx).await?;
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &state).await
        })
    }
}
