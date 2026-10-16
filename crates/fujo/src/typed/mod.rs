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
//! Stages run in declaration order. A middleware receives the state derived by
//! the stages before it and wraps every stage after it, including the action:
//! ```
//! use fujo::{Cli, Command, arg};
//! let command = Command::new("build")
//!     .inputs(arg::path("root"))
//!     .derive(|_, root, ()| Ok(root.display().to_string()))
//!     .middleware(|ctx, _, label, next| {
//!         ctx.out.log(format!("start {label}"));
//!         let result = next.run(label);
//!         ctx.out.log("finish");
//!         result
//!     })
//!     .derive(|_, _, label| Ok(label.len()))
//!     .action(|ctx, _, length| {
//!         ctx.out.log(length);
//!         Ok(())
//!     });
//! let result = Cli::new("build").default(command).execute(["src"], Default::default());
//! assert_eq!(result.stdout, "start src\n3\nfinish\n");
//! ```
//!
//! Stages are values. A type implementing [`Middleware`] or [`Derive`] plugs in
//! with `wrap` and `then`, and a [`Pipeline`] built once plugs into any command
//! with `inherit`:
//! ```
//! use fujo::typed::Next;
//! use fujo::{Cli, Command, InputSet, Middleware, Scope, flag};
//! struct Count;
//! impl<I: InputSet, S> Middleware<I, S> for Count {
//!     fn call<'a>(&'a self, ctx: Scope<'a>, _: I::Values<'a>, state: S, next: Next<'a, S>)
//!     -> fujo::Result<()> {
//!         ctx.out.log("counted");
//!         next.run(state)
//!     }
//! }
//! let shared = fujo::pipeline(flag::string("profile").default("dev".into()))
//!     .derive(|_, profile, ()| Ok(profile.to_uppercase()))
//!     .wrap(Count);
//! let show = Command::new("show")
//!     .inputs(())
//!     .inherit(&shared)
//!     .tap(|ctx, (), profile| {
//!         ctx.out.log(format!("tap {profile}"));
//!         Ok(())
//!     })
//!     .map(|profile| profile.len())
//!     .action(|ctx, (), length| {
//!         ctx.out.log(length);
//!         Ok(())
//!     });
//! let kinds: Vec<_> = show.stages().iter().map(|stage| stage.kind).collect();
//! assert_eq!(kinds, [
//!     fujo::StageKind::Derive,
//!     fujo::StageKind::Middleware,
//!     fujo::StageKind::Tap,
//!     fujo::StageKind::Derive,
//! ]);
//! let app = Cli::new("demo").inputs(flag::string("profile").default("dev".into())).command(show);
//! let result = app.execute(["show"], Default::default());
//! assert_eq!(result.stdout, "counted\ntap DEV\n3\n");
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
//!
//! A synchronous middleware wraps only synchronous stages. Asynchronous stages
//! after it are rejected at compile time:
//! ```compile_fail
//! use fujo::Command;
//! let command = Command::new("demo").inputs(())
//!     .middleware(|_, (), state, next| next.run(state))
//!     .action_async(async |_, (), ()| Ok(()));
//! ```
//! ```compile_fail
//! use fujo::Command;
//! let command = Command::new("demo").inputs(())
//!     .middleware(|_, (), state, next| next.run(state))
//!     .derive_async(async |_, (), ()| Ok(1_u8));
//! ```
//! ```compile_fail
//! use fujo::Command;
//! let command = Command::new("demo").inputs(())
//!     .middleware(|_, (), state, next| next.run(state))
//!     .middleware_async(async |_, (), state, next| next.run(state).await);
//! ```
//!
//! A pipeline carries its tail into the command that inherits it, and a command
//! with a synchronous middleware cannot inherit a pipeline:
//! ```compile_fail
//! use fujo::Command;
//! let shared = fujo::pipeline(()).middleware(|_, (), state, next| next.run(state));
//! let command = Command::new("demo").inputs(()).inherit(&shared)
//!     .action_async(async |_, (), ()| Ok(()));
//! ```
//! ```compile_fail
//! use fujo::Command;
//! let shared = fujo::pipeline(()).derive(|_, (), ()| Ok(1_u8));
//! let command = Command::new("demo").inputs(())
//!     .middleware(|_, (), state, next| next.run(state))
//!     .inherit(&shared);
//! ```
//!
//! A continuation runs at most once:
//! ```compile_fail
//! use fujo::Command;
//! let command = Command::new("demo").inputs(())
//!     .middleware(|_, (), state, next| {
//!         next.run(state)?;
//!         next.run(state)
//!     });
//! ```
mod inputs;
use crate::CliError;
use crate::Command;
use crate::Context;
use crate::InputSources;
use crate::Meta;
use crate::Output;
use crate::Result;
use crate::execution::Action;
use crate::execution::AsyncAction;
use crate::execution::LocalFuture;
use crate::execution::async_middleware_required;
use crate::execution::async_required;
use crate::schema::Surface;
#[doc(hidden)]
pub use inputs::__private;
pub use inputs::FlagSet;
pub use inputs::InputSet;
use serde::Serialize;
use std::any::type_name;
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

/// Later stages may be synchronous or asynchronous.
pub struct AnyTail;
/// A synchronous middleware wraps the remaining stages, so they are synchronous.
pub struct SyncTail;

/// The remaining typed stages and the action. Calling `run` executes them once
/// with the given state; dropping it short-circuits them.
pub struct Next<'a, S> {
    run: Box<dyn FnOnce(S) -> Result<()> + 'a>,
}
impl<S> Next<'_, S> {
    pub fn run(self, state: S) -> Result<()> {
        (self.run)(state)
    }
}
/// The asynchronous counterpart of [`Next`]; await `run` at most once.
pub struct AsyncNext<'a, S> {
    run: Continuation<'a, S>,
}
impl<'a, S: 'a> AsyncNext<'a, S> {
    pub fn run(self, state: S) -> LocalFuture<'a, Result<()>> {
        self.run.call(state)
    }
}

/// One stage of a command's pipeline, in execution order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageInfo {
    pub kind: StageKind,
    pub handler: &'static str,
    pub asynchronous: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    Derive,
    Tap,
    Middleware,
    Catch,
    Finally,
}
impl StageInfo {
    pub(crate) fn new(kind: StageKind, handler: &'static str, asynchronous: bool) -> Self {
        Self {
            kind,
            handler,
            asynchronous,
        }
    }
}

/// A synchronous state transformation usable with `then`. Closures of the
/// matching shape implement it.
pub trait Derive<I: InputSet, S>: Send + Sync + 'static {
    type Output: 'static;
    fn call<'a>(&'a self, ctx: Scope<'a>, values: I::Values<'a>, state: S) -> Result<Self::Output>;
}
impl<I: InputSet, S, T: 'static, F> Derive<I, S> for F
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync + 'static,
{
    type Output = T;
    fn call<'a>(&'a self, ctx: Scope<'a>, values: I::Values<'a>, state: S) -> Result<T> {
        self(ctx, values, state)
    }
}
/// An asynchronous state transformation usable with `then_async`.
pub trait AsyncDerive<I: InputSet, S: 'static>: Send + Sync + 'static {
    type Output: 'static;
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        values: I::Values<'a>,
        state: S,
    ) -> LocalFuture<'a, Result<Self::Output>>;
}
impl<I: InputSet, S: 'static, T: 'static, F> AsyncDerive<I, S> for F
where
    F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync + 'static,
{
    type Output = T;
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        values: I::Values<'a>,
        state: S,
    ) -> LocalFuture<'a, Result<T>> {
        Box::pin(async move { self(ctx, values, state).await })
    }
}
/// A synchronous stage wrapping everything after it, usable with `wrap`.
pub trait Middleware<I: InputSet, S>: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        values: I::Values<'a>,
        state: S,
        next: Next<'a, S>,
    ) -> Result<()>;
}
impl<I: InputSet, S, F> Middleware<I, S> for F
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, S, Next<'a, S>) -> Result<()> + Send + Sync + 'static,
{
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        values: I::Values<'a>,
        state: S,
        next: Next<'a, S>,
    ) -> Result<()> {
        self(ctx, values, state, next)
    }
}
/// An asynchronous stage wrapping everything after it, usable with `wrap_async`.
pub trait AsyncMiddleware<I: InputSet, S: 'static>: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        values: I::Values<'a>,
        state: S,
        next: AsyncNext<'a, S>,
    ) -> LocalFuture<'a, Result<()>>;
}
impl<I: InputSet, S: 'static, F> AsyncMiddleware<I, S> for F
where
    F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, S, AsyncNext<'a, S>) -> Result<()>
        + Send
        + Sync
        + 'static,
{
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        values: I::Values<'a>,
        state: S,
        next: AsyncNext<'a, S>,
    ) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move { self(ctx, values, state, next).await })
    }
}

struct Tap<F>(F);
impl<I: InputSet, S: 'static, F> Derive<I, S> for Tap<F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, &S) -> Result<()> + Send + Sync + 'static,
{
    type Output = S;
    fn call<'a>(&'a self, ctx: Scope<'a>, values: I::Values<'a>, state: S) -> Result<S> {
        (self.0)(ctx, values, &state)?;
        Ok(state)
    }
}
struct AsyncTap<F>(F);
impl<I: InputSet, S: 'static, F> AsyncDerive<I, S> for AsyncTap<F>
where
    F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, &S) -> Result<()> + Send + Sync + 'static,
{
    type Output = S;
    fn call<'a>(
        &'a self,
        ctx: Scope<'a>,
        values: I::Values<'a>,
        state: S,
    ) -> LocalFuture<'a, Result<S>> {
        Box::pin(async move {
            (self.0)(ctx, values, &state).await?;
            Ok(state)
        })
    }
}
struct Map<F>(F);
impl<I: InputSet, S, T: 'static, F> Derive<I, S> for Map<F>
where
    F: Fn(S) -> T + Send + Sync + 'static,
{
    type Output = T;
    fn call<'a>(&'a self, _: Scope<'a>, _: I::Values<'a>, state: S) -> Result<T> {
        Ok((self.0)(state))
    }
}

/// A chain of typed stages over a carrier: a [`Command`] awaiting its action, or
/// `()` for a reusable [`Pipeline`]. Each derive consumes the preceding state and
/// returns its successor; each middleware receives the state at its position and
/// wraps the stages after it.
pub struct Chain<C, I: InputSet, S: 'static = (), M = AnyTail> {
    carrier: C,
    inputs: Arc<I>,
    state: Arc<dyn Stage<S>>,
    tail: PhantomData<fn() -> M>,
}
pub type TypedCommand<I, S = (), M = AnyTail> = Chain<Command, I, S, M>;
/// Stages shared between commands. Build one with [`pipeline`] and attach it
/// with `inherit`; its inputs must be registered on every command that inherits it.
pub type Pipeline<I, S = (), M = AnyTail> = Chain<(), I, S, M>;
pub fn pipeline<I: InputSet>(inputs: I) -> Pipeline<I> {
    Chain {
        carrier: (),
        inputs: Arc::new(inputs),
        state: Arc::new(Initial),
        tail: PhantomData,
    }
}
impl Command {
    /// Register a complete input bundle and bind its shape to subsequent callbacks.
    pub fn inputs<I: InputSet>(mut self, inputs: I) -> TypedCommand<I> {
        inputs.visit(&mut |input| match input.surface {
            Surface::Flag => self.schema.flags.push(input),
            Surface::Arg => self.schema.args.push(input),
        });
        Chain {
            carrier: self,
            inputs: Arc::new(inputs),
            state: Arc::new(Initial),
            tail: PhantomData,
        }
    }
}
impl<C, I: InputSet, S: 'static, M> Chain<C, I, S, M> {
    fn stage<T: 'static, N>(self, state: Arc<dyn Stage<T>>) -> Chain<C, I, T, N> {
        Chain {
            carrier: self.carrier,
            inputs: self.inputs,
            state,
            tail: PhantomData,
        }
    }
    fn derived<D: Derive<I, S>>(
        self,
        kind: StageKind,
        handler: &'static str,
        derive: D,
    ) -> Chain<C, I, D::Output, M> {
        let stage = Arc::new(Derived {
            info: StageInfo::new(kind, handler, false),
            inputs: self.inputs.clone(),
            previous: self.state.clone(),
            handler: derive,
        });
        self.stage(stage)
    }
    pub fn then<D: Derive<I, S>>(self, derive: D) -> Chain<C, I, D::Output, M> {
        self.derived(StageKind::Derive, type_name::<D>(), derive)
    }
    pub fn derive<T: 'static, F>(self, derive: F) -> Chain<C, I, T, M>
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync + 'static,
    {
        self.then(derive)
    }
    pub fn map<T: 'static, F>(self, map: F) -> Chain<C, I, T, M>
    where
        F: Fn(S) -> T + Send + Sync + 'static,
    {
        self.derived(StageKind::Derive, type_name::<F>(), Map(map))
    }
    pub fn tap<F>(self, tap: F) -> Chain<C, I, S, M>
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, &S) -> Result<()> + Send + Sync + 'static,
    {
        self.derived(StageKind::Tap, type_name::<F>(), Tap(tap))
    }
    pub fn wrap<W: Middleware<I, S>>(self, middleware: W) -> Chain<C, I, S, SyncTail> {
        let stage = Arc::new(Wrapped {
            info: StageInfo::new(StageKind::Middleware, type_name::<W>(), false),
            inputs: self.inputs.clone(),
            previous: self.state.clone(),
            handler: middleware,
        });
        self.stage(stage)
    }
    pub fn middleware<F>(self, middleware: F) -> Chain<C, I, S, SyncTail>
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, S, Next<'a, S>) -> Result<()>
            + Send
            + Sync
            + 'static,
    {
        self.wrap(middleware)
    }
    pub fn catch<F>(self, catch: F) -> Chain<C, I, S, M>
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, CliError) -> Result<()> + Send + Sync + 'static,
    {
        let stage = Arc::new(Caught {
            info: StageInfo::new(StageKind::Catch, type_name::<F>(), false),
            inputs: self.inputs.clone(),
            previous: self.state.clone(),
            handler: catch,
        });
        self.stage(stage)
    }
    pub fn finally<F>(self, finally: F) -> Chain<C, I, S, M>
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, &Result<()>) + Send + Sync + 'static,
    {
        let stage = Arc::new(Finalized {
            info: StageInfo::new(StageKind::Finally, type_name::<F>(), false),
            inputs: self.inputs.clone(),
            previous: self.state.clone(),
            handler: finally,
        });
        self.stage(stage)
    }
    pub fn stages(&self) -> Vec<StageInfo> {
        let mut stages = vec![];
        self.state.describe(&mut stages);
        stages
    }
}
impl<C, I: InputSet, S: 'static> Chain<C, I, S, AnyTail> {
    fn derived_async<D: AsyncDerive<I, S>>(
        self,
        kind: StageKind,
        handler: &'static str,
        derive: D,
    ) -> Chain<C, I, D::Output, AnyTail> {
        let stage = Arc::new(AsyncDerived {
            info: StageInfo::new(kind, handler, true),
            inputs: self.inputs.clone(),
            previous: self.state.clone(),
            handler: derive,
        });
        self.stage(stage)
    }
    pub fn then_async<D: AsyncDerive<I, S>>(self, derive: D) -> Chain<C, I, D::Output, AnyTail> {
        self.derived_async(StageKind::Derive, type_name::<D>(), derive)
    }
    pub fn derive_async<T: 'static, F>(self, derive: F) -> Chain<C, I, T, AnyTail>
    where
        F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, S) -> Result<T> + Send + Sync + 'static,
    {
        self.then_async(derive)
    }
    pub fn tap_async<F>(self, tap: F) -> Chain<C, I, S, AnyTail>
    where
        F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, &S) -> Result<()> + Send + Sync + 'static,
    {
        self.derived_async(StageKind::Tap, type_name::<F>(), AsyncTap(tap))
    }
    pub fn wrap_async<W: AsyncMiddleware<I, S>>(self, middleware: W) -> Chain<C, I, S, AnyTail> {
        let stage = Arc::new(AsyncWrapped {
            info: StageInfo::new(StageKind::Middleware, type_name::<W>(), true),
            inputs: self.inputs.clone(),
            previous: self.state.clone(),
            handler: middleware,
        });
        self.stage(stage)
    }
    pub fn middleware_async<F>(self, middleware: F) -> Chain<C, I, S, AnyTail>
    where
        F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, S, AsyncNext<'a, S>) -> Result<()>
            + Send
            + Sync
            + 'static,
    {
        self.wrap_async(middleware)
    }
}
impl<C, I: InputSet> Chain<C, I, (), AnyTail> {
    /// Run a shared pipeline after the stages declared so far. The pipeline's
    /// state and tail become this chain's.
    pub fn inherit<G: InputSet, S: 'static, N>(
        self,
        pipeline: &Pipeline<G, S, N>,
    ) -> Chain<C, I, S, N> {
        let stage = Arc::new(Inherited {
            previous: self.state.clone(),
            pipeline: pipeline.state.clone(),
        });
        self.stage(stage)
    }
}
impl<I: InputSet, S: 'static, M> Chain<Command, I, S, M> {
    pub fn action<F>(mut self, action: F) -> Command
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync + 'static,
    {
        self.state.describe(&mut self.carrier.stages);
        if self.state.is_async() {
            self.carrier.action = Some(Action::Async(Arc::new(BoundSync {
                inputs: self.inputs,
                state: self.state,
                handler: action,
            })));
            self.carrier
        } else {
            self.carrier.action(move |ctx| {
                let ctx: &Context = ctx;
                let handler = &action;
                let inputs = &self.inputs;
                let next = Continuation::Sync(Box::new(move |state: S| {
                    handler(scope(ctx), inputs.read(&ctx.inputs)?, &state)
                }));
                match self.state.compose(ctx, next) {
                    Continuation::Sync(run) => run(()),
                    Continuation::Async(_) => Err(async_required()),
                }
            })
        }
    }
}
impl<I: InputSet, S: 'static> Chain<Command, I, S, AnyTail> {
    pub fn action_async<F>(mut self, action: F) -> Command
    where
        F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync + 'static,
    {
        self.state.describe(&mut self.carrier.stages);
        self.carrier.action = Some(Action::Async(Arc::new(BoundAsync {
            inputs: self.inputs,
            state: self.state,
            handler: action,
        })));
        self.carrier
    }
}

enum Continuation<'a, S> {
    Sync(Box<dyn FnOnce(S) -> Result<()> + 'a>),
    Async(Box<dyn FnOnce(S) -> LocalFuture<'a, Result<()>> + 'a>),
}
impl<'a, S: 'a> Continuation<'a, S> {
    fn call(self, state: S) -> LocalFuture<'a, Result<()>> {
        match self {
            Self::Sync(run) => Box::pin(async move { run(state) }),
            Self::Async(run) => run(state),
        }
    }
}

/// A stage transforms the continuation of its output into a continuation of the
/// initial state, so each stage wraps everything declared after it.
trait Stage<S>: Send + Sync {
    fn is_async(&self) -> bool;
    fn describe(&self, stages: &mut Vec<StageInfo>);
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()>;
}
struct Initial;
impl Stage<()> for Initial {
    fn is_async(&self) -> bool {
        false
    }
    fn describe(&self, _: &mut Vec<StageInfo>) {}
    fn compose<'a>(&'a self, _: &'a Context, next: Continuation<'a, ()>) -> Continuation<'a, ()> {
        next
    }
}
struct Inherited<S> {
    previous: Arc<dyn Stage<()>>,
    pipeline: Arc<dyn Stage<S>>,
}
impl<S: 'static> Stage<S> for Inherited<S> {
    fn is_async(&self) -> bool {
        self.previous.is_async() || self.pipeline.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        self.pipeline.describe(stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        self.previous.compose(ctx, self.pipeline.compose(ctx, next))
    }
}
struct Derived<I, S, D> {
    info: StageInfo,
    inputs: Arc<I>,
    previous: Arc<dyn Stage<S>>,
    handler: D,
}
impl<I: InputSet, S: 'static, D: Derive<I, S>> Stage<D::Output> for Derived<I, S, D> {
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        stages.push(self.info.clone());
    }
    fn compose<'a>(
        &'a self,
        ctx: &'a Context,
        next: Continuation<'a, D::Output>,
    ) -> Continuation<'a, ()> {
        let derive = move |state: S| {
            self.handler
                .call(scope(ctx), self.inputs.read(&ctx.inputs)?, state)
        };
        self.previous.compose(
            ctx,
            match next {
                Continuation::Sync(next) => {
                    Continuation::Sync(Box::new(move |state| next(derive(state)?)))
                }
                Continuation::Async(next) => Continuation::Async(Box::new(move |state| {
                    Box::pin(async move { next(derive(state)?).await })
                })),
            },
        )
    }
}
struct AsyncDerived<I, S, D> {
    info: StageInfo,
    inputs: Arc<I>,
    previous: Arc<dyn Stage<S>>,
    handler: D,
}
impl<I: InputSet, S: 'static, D: AsyncDerive<I, S>> Stage<D::Output> for AsyncDerived<I, S, D> {
    fn is_async(&self) -> bool {
        true
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        stages.push(self.info.clone());
    }
    fn compose<'a>(
        &'a self,
        ctx: &'a Context,
        next: Continuation<'a, D::Output>,
    ) -> Continuation<'a, ()> {
        self.previous.compose(
            ctx,
            Continuation::Async(Box::new(move |state| {
                Box::pin(async move {
                    let value = self
                        .handler
                        .call(scope(ctx), self.inputs.read(&ctx.inputs)?, state)
                        .await?;
                    next.call(value).await
                })
            })),
        )
    }
}
struct Wrapped<I, S, W> {
    info: StageInfo,
    inputs: Arc<I>,
    previous: Arc<dyn Stage<S>>,
    handler: W,
}
impl<I: InputSet, S: 'static, W: Middleware<I, S>> Stage<S> for Wrapped<I, S, W> {
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        stages.push(self.info.clone());
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        self.previous.compose(
            ctx,
            Continuation::Sync(match next {
                Continuation::Sync(run) => Box::new(move |state| {
                    self.handler.call(
                        scope(ctx),
                        self.inputs.read(&ctx.inputs)?,
                        state,
                        Next { run },
                    )
                }),
                Continuation::Async(_) => Box::new(|_| Err(async_middleware_required())),
            }),
        )
    }
}
struct AsyncWrapped<I, S, W> {
    info: StageInfo,
    inputs: Arc<I>,
    previous: Arc<dyn Stage<S>>,
    handler: W,
}
impl<I: InputSet, S: 'static, W: AsyncMiddleware<I, S>> Stage<S> for AsyncWrapped<I, S, W> {
    fn is_async(&self) -> bool {
        true
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        stages.push(self.info.clone());
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        self.previous.compose(
            ctx,
            Continuation::Async(Box::new(move |state| {
                Box::pin(async move {
                    self.handler
                        .call(
                            scope(ctx),
                            self.inputs.read(&ctx.inputs)?,
                            state,
                            AsyncNext { run: next },
                        )
                        .await
                })
            })),
        )
    }
}
struct Caught<I, S, F> {
    info: StageInfo,
    inputs: Arc<I>,
    previous: Arc<dyn Stage<S>>,
    handler: F,
}
impl<I: InputSet, S: 'static, F> Stage<S> for Caught<I, S, F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, CliError) -> Result<()> + Send + Sync,
{
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        stages.push(self.info.clone());
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        let catch = move |result: Result<()>| match result {
            Err(error) => (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, error),
            ok => ok,
        };
        self.previous.compose(
            ctx,
            match next {
                Continuation::Sync(run) => {
                    Continuation::Sync(Box::new(move |state| catch(run(state))))
                }
                Continuation::Async(run) => Continuation::Async(Box::new(move |state| {
                    Box::pin(async move { catch(run(state).await) })
                })),
            },
        )
    }
}
struct Finalized<I, S, F> {
    info: StageInfo,
    inputs: Arc<I>,
    previous: Arc<dyn Stage<S>>,
    handler: F,
}
impl<I: InputSet, S: 'static, F> Stage<S> for Finalized<I, S, F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, &Result<()>) + Send + Sync,
{
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        stages.push(self.info.clone());
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        let finish = move |result: Result<()>| {
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &result);
            result
        };
        self.previous.compose(
            ctx,
            match next {
                Continuation::Sync(run) => {
                    Continuation::Sync(Box::new(move |state| finish(run(state))))
                }
                Continuation::Async(run) => Continuation::Async(Box::new(move |state| {
                    Box::pin(async move { finish(run(state).await) })
                })),
            },
        )
    }
}
struct BoundSync<I, S, F> {
    inputs: Arc<I>,
    state: Arc<dyn Stage<S>>,
    handler: F,
}
impl<I: InputSet, S: 'static, F> AsyncAction for BoundSync<I, S, F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync,
{
    fn call<'a>(&'a self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>> {
        let ctx: &'a Context = ctx;
        let next = Continuation::Sync(Box::new(move |state: S| {
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &state)
        }));
        self.state.compose(ctx, next).call(())
    }
}
struct BoundAsync<I, S, F> {
    inputs: Arc<I>,
    state: Arc<dyn Stage<S>>,
    handler: F,
}
impl<I: InputSet, S: 'static, F> AsyncAction for BoundAsync<I, S, F>
where
    F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync,
{
    fn call<'a>(&'a self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>> {
        let ctx: &'a Context = ctx;
        let next = Continuation::Async(Box::new(move |state: S| {
            Box::pin(async move {
                (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &state).await
            })
        }));
        self.state.compose(ctx, next).call(())
    }
}
