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
//! A type implementing [`Middleware`] or [`Derive`] plugs in with `wrap` and
//! `then`. A [`Pipeline`] plugs into any command with `inherit`, which also
//! registers the pipeline's inputs on that command:
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
//!     .named("upper")
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
//! let stages = show.stages();
//! let kinds: Vec<_> = stages.iter().map(|stage| stage.kind).collect();
//! assert_eq!(kinds, [
//!     fujo::StageKind::Derive,
//!     fujo::StageKind::Middleware,
//!     fujo::StageKind::Tap,
//!     fujo::StageKind::Derive,
//! ]);
//! assert_eq!(stages[0].handler, "upper");
//! let result = Cli::new("demo").command(show).execute(["show", "--profile=ci"], Default::default());
//! assert_eq!(result.stdout, "counted\ntap CI\n2\n");
//! ```
//!
//! A pipeline registered with `share` runs once before dispatch; commands take
//! its state with `shared`. `retry` reruns the later stages with a clone of the
//! state:
//! ```
//! use fujo::{Cli, Command, flag};
//! use std::sync::atomic::{AtomicUsize, Ordering};
//! use std::sync::Arc;
//! let session = fujo::pipeline(flag::string("profile").default("dev".into()))
//!     .derive(|_, profile, ()| Ok(profile.to_uppercase()));
//! let attempts = Arc::new(AtomicUsize::new(0));
//! let count = attempts.clone();
//! let show = Command::new("show")
//!     .inputs(())
//!     .shared(&session)
//!     .retry(1)
//!     .action(move |ctx, (), profile| {
//!         if count.fetch_add(1, Ordering::SeqCst) == 0 {
//!             return Err(fujo::CliError::new("FLAKY", "first attempt"));
//!         }
//!         ctx.out.log(profile);
//!         Ok(())
//!     });
//! let app = Cli::new("demo").share(&session).command(show);
//! let result = app.execute(["show", "--profile=prod"], Default::default());
//! assert_eq!(result.stdout, "PROD\n");
//! assert_eq!(attempts.load(Ordering::SeqCst), 2);
//! ```
//!
//! Only registered inputs reach a typed callback:
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
//! State availability comes from the derivation chain:
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
//! Derived state owns its data:
//! ```compile_fail
//! use fujo::{Command, arg};
//! let command = Command::new("demo").inputs(arg::string("name"))
//!     .derive(|_, name, ()| Ok(name));
//! ```
//!
//! Asynchronous stages after a synchronous middleware are rejected:
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
//! `share` registers flags only, including flags reached through `inherit`:
//! ```compile_fail
//! use fujo::{Command, arg};
//! let shared = fujo::pipeline(arg::string("name")).derive(|_, name, ()| Ok(name.clone()));
//! let command = Command::new("demo").share(&shared);
//! ```
//! ```compile_fail
//! use fujo::{Cli, arg};
//! let named = fujo::pipeline(arg::string("name")).derive(|_, name, ()| Ok(name.clone()));
//! let app = Cli::new("demo").share(&fujo::pipeline(()).inherit(&named));
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
use crate::execution::Step;
use crate::execution::async_middleware_required;
use crate::execution::async_required;
use crate::schema::InputSchema;
use crate::schema::Surface;
#[doc(hidden)]
pub use inputs::__private;
pub use inputs::FlagSet;
pub use inputs::Inherits;
pub use inputs::InputSet;
use serde::Serialize;
use std::any::type_name;
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::task::Poll;
use std::time::Duration;

/// Output, metadata, and provenance for a typed callback.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub out: &'a Output,
    pub meta: &'a Meta,
    pub sources: &'a InputSources,
    context: &'a Context,
}
impl Scope<'_> {
    pub fn shared<G, S: Clone + Send + 'static, N>(
        &self,
        pipeline: &Pipeline<G, S, N>,
    ) -> Result<S> {
        read_shared(self.context, pipeline.id())
    }
}
fn scope(ctx: &Context) -> Scope<'_> {
    Scope {
        out: &ctx.out,
        meta: &ctx.meta,
        sources: &ctx.inputs.sources,
        context: ctx,
    }
}
struct Shared<S>(Mutex<HashMap<usize, S>>);
fn read_shared<S: Clone + Send + 'static>(ctx: &Context, id: usize) -> Result<S> {
    ctx.state::<Shared<S>>()
        .ok()
        .and_then(|shared| {
            shared
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&id)
                .cloned()
        })
        .ok_or_else(|| {
            CliError::new(
                "SHARED_STATE_MISSING",
                format!("No shared {} was produced", type_name::<S>()),
            )
            .suggest("Register the pipeline with Cli::share or Command::share")
        })
}
fn store_shared<S: Send + 'static>(ctx: &mut Context, id: usize, value: S) {
    if let Ok(shared) = ctx.state::<Shared<S>>() {
        shared
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, value);
        return;
    }
    ctx.insert(Shared(Mutex::new(HashMap::from([(id, value)]))));
}
pub(crate) fn declare(bucket: &mut Vec<InputSchema>, inputs: &[InputSchema]) {
    for input in inputs {
        if bucket.iter().all(|existing| existing.name != input.name) {
            bucket.push(input.clone());
        }
    }
}

/// Later stages may be synchronous or asynchronous.
pub struct AnyTail;
/// Later stages are synchronous.
pub struct SyncTail;

/// The remaining typed stages and the action. Calling `run` executes them once
/// with the given state; dropping it skips them.
pub struct Next<'a, S> {
    run: Rc<dyn Fn(S) -> Result<()> + 'a>,
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
    Timeout,
    Retry,
    Shared,
}
impl StageInfo {
    pub fn new(kind: StageKind, handler: &'static str, asynchronous: bool) -> Self {
        Self {
            kind,
            handler,
            asynchronous,
        }
    }
}
/// The handler of a command's action and whether that handler is asynchronous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActionInfo {
    pub handler: &'static str,
    pub asynchronous: bool,
}

/// A synchronous state transformation usable with `then`.
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
/// `()` for a reusable [`Pipeline`].
pub struct Chain<C, I, S: 'static = (), M = AnyTail> {
    carrier: C,
    inputs: Arc<I>,
    declared: Arc<[InputSchema]>,
    state: Arc<dyn Stage<S>>,
    tail: PhantomData<fn() -> M>,
}
impl<C: Clone, I, S: 'static, M> Clone for Chain<C, I, S, M> {
    fn clone(&self) -> Self {
        Self {
            carrier: self.carrier.clone(),
            inputs: self.inputs.clone(),
            declared: self.declared.clone(),
            state: self.state.clone(),
            tail: PhantomData,
        }
    }
}
impl<C, I, S: 'static, M> std::fmt::Debug for Chain<C, I, S, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chain")
            .field("stages", &self.stages())
            .finish_non_exhaustive()
    }
}
pub type TypedCommand<I, S = (), M = AnyTail> = Chain<Command, I, S, M>;
/// Stages shared between commands through `inherit` or `share`.
pub type Pipeline<I, S = (), M = AnyTail> = Chain<(), I, S, M>;
pub fn pipeline<I: InputSet>(inputs: I) -> Pipeline<I> {
    chain((), inputs)
}
fn chain<C, I: InputSet>(carrier: C, inputs: I) -> Chain<C, I> {
    let mut declared = vec![];
    inputs.visit(&mut |input| declared.push(input));
    Chain {
        carrier,
        inputs: Arc::new(inputs),
        declared: declared.into(),
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
        chain(self, inputs)
    }
    pub(crate) fn declare(&mut self, inputs: &[InputSchema]) {
        let (flags, args): (Vec<_>, Vec<_>) = inputs
            .iter()
            .cloned()
            .partition(|input| matches!(input.surface, Surface::Flag));
        declare(&mut self.schema.flags, &flags);
        declare(&mut self.schema.args, &args);
    }
}
impl<C, I, S: 'static, M> Chain<C, I, S, M> {
    fn stage<T: 'static, N>(self, state: Arc<dyn Stage<T>>) -> Chain<C, I, T, N> {
        Chain {
            carrier: self.carrier,
            inputs: self.inputs,
            declared: self.declared,
            state,
            tail: PhantomData,
        }
    }
    pub fn stages(&self) -> Vec<StageInfo> {
        let mut stages = vec![];
        self.state.describe(&mut stages);
        stages
    }
    /// Label the stage declared last; `stages()` and `--explain` show the label
    /// in place of the handler's type name.
    pub fn named(self, label: &'static str) -> Chain<C, I, S, M> {
        let stage = Arc::new(Named {
            previous: self.state.clone(),
            label,
        });
        self.stage(stage)
    }
    pub(crate) fn id(&self) -> usize {
        Arc::as_ptr(&self.state).cast::<()>().addr()
    }
}
impl<C, I: InputSet, S: 'static, M> Chain<C, I, S, M> {
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
    /// Rerun every later stage and the action after a failure, up to `retries`
    /// more times, with a clone of the state. The last error is returned. A
    /// `timeout` declared before `retry` bounds all attempts together; one
    /// declared after it bounds each attempt.
    pub fn retry(self, retries: usize) -> Chain<C, I, S, M>
    where
        S: Clone,
    {
        let stage = Arc::new(Retried {
            info: StageInfo::new(StageKind::Retry, type_name::<S>(), false),
            previous: self.state.clone(),
            retries,
        });
        self.stage(stage)
    }
}
impl<C, I: InputSet, M> Chain<C, I, (), M> {
    /// Take a clone of the state a pipeline registered with `Cli::share` or
    /// `Command::share` produced for this invocation.
    pub fn shared<G, S: Clone + Send + 'static, N>(
        self,
        pipeline: &Pipeline<G, S, N>,
    ) -> Chain<C, I, S, M> {
        let stage = Arc::new(SharedStage {
            info: StageInfo::new(StageKind::Shared, type_name::<S>(), false),
            previous: self.state.clone(),
            id: pipeline.id(),
            state: PhantomData,
        });
        self.stage(stage)
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
    /// Fail with `TIMEOUT` and drop the later stages, including any `finally`
    /// declared after this one, when `sleep(limit)` completes first. The
    /// application supplies the timer, such as `tokio::time::sleep`. Later stages
    /// that block without awaiting complete before the timer is polled.
    pub fn timeout<F, Fut>(self, limit: Duration, sleep: F) -> Chain<C, I, S, AnyTail>
    where
        F: Fn(Duration) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let stage = Arc::new(TimedOut {
            info: StageInfo::new(StageKind::Timeout, type_name::<F>(), true),
            previous: self.state.clone(),
            limit,
            sleep,
        });
        self.stage(stage)
    }
}
impl<C, I> Chain<C, I, (), AnyTail> {
    fn inherited<G, S: 'static, N>(self, pipeline: &Pipeline<G, S, N>) -> Chain<C, I, S, N> {
        let stage = Arc::new(Inherited {
            previous: self.state.clone(),
            pipeline: pipeline.state.clone(),
        });
        self.stage(stage)
    }
}
impl<I> Chain<(), I, (), AnyTail> {
    /// Run a shared pipeline after the stages declared so far. The pipeline's
    /// state, tail, and inputs become this pipeline's.
    pub fn inherit<G, S: 'static, N>(
        self,
        pipeline: &Pipeline<G, S, N>,
    ) -> Pipeline<Inherits<I, G>, S, N> {
        let mut declared = self.declared.to_vec();
        declare(&mut declared, &pipeline.declared);
        Chain {
            carrier: (),
            inputs: Arc::new(Inherits {
                inputs: self.inputs,
                inherited: PhantomData,
            }),
            declared: declared.into(),
            state: Arc::new(Inherited {
                previous: self.state,
                pipeline: pipeline.state.clone(),
            }),
            tail: PhantomData,
        }
    }
}
impl<I> Chain<Command, I, (), AnyTail> {
    /// Run a shared pipeline after the stages declared so far. The pipeline's
    /// state and tail become this command's, and its inputs are registered on
    /// the command.
    pub fn inherit<G, S: 'static, N>(
        mut self,
        pipeline: &Pipeline<G, S, N>,
    ) -> TypedCommand<I, S, N> {
        self.carrier.declare(&pipeline.declared);
        self.inherited(pipeline)
    }
}
impl<I: InputSet, S: 'static, M> Chain<Command, I, S, M> {
    pub fn action<F>(mut self, action: F) -> Command
    where
        F: for<'a> Fn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync + 'static,
    {
        self.state.describe(&mut self.carrier.typed);
        self.carrier.action_info = Some(ActionInfo {
            handler: type_name::<F>(),
            asynchronous: false,
        });
        let bound = Arc::new(BoundSync {
            inputs: self.inputs,
            state: self.state,
            handler: action,
        });
        self.carrier.action = Some(if bound.state.is_async() {
            Action::Async(bound)
        } else {
            Action::Sync(Arc::new(move |ctx: &mut Context| bound.run(ctx)))
        });
        self.carrier
    }
}
impl<I: InputSet, S: 'static> Chain<Command, I, S, AnyTail> {
    pub fn action_async<F>(mut self, action: F) -> Command
    where
        F: for<'a> AsyncFn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync + 'static,
    {
        self.state.describe(&mut self.carrier.typed);
        self.carrier.action_info = Some(ActionInfo {
            handler: type_name::<F>(),
            asynchronous: true,
        });
        self.carrier.action = Some(Action::Async(Arc::new(BoundAsync {
            inputs: self.inputs,
            state: self.state,
            handler: action,
        })));
        self.carrier
    }
}

enum Continuation<'a, S> {
    Sync(Rc<dyn Fn(S) -> Result<()> + 'a>),
    Async(Rc<dyn Fn(S) -> LocalFuture<'a, Result<()>> + 'a>),
}
impl<S> Clone for Continuation<'_, S> {
    fn clone(&self) -> Self {
        match self {
            Self::Sync(run) => Self::Sync(run.clone()),
            Self::Async(run) => Self::Async(run.clone()),
        }
    }
}
impl<'a, S: 'a> Continuation<'a, S> {
    fn sync(run: impl Fn(S) -> Result<()> + 'a) -> Self {
        Self::Sync(Rc::new(run))
    }
    fn asynchronous(run: impl Fn(S) -> LocalFuture<'a, Result<()>> + 'a) -> Self {
        Self::Async(Rc::new(run))
    }
    fn call(&self, state: S) -> LocalFuture<'a, Result<()>> {
        match self {
            Self::Sync(run) => {
                let run = run.clone();
                Box::pin(async move { run(state) })
            }
            Self::Async(run) => run(state),
        }
    }
    fn call_sync(self, state: S) -> Result<()> {
        match self {
            Self::Sync(run) => run(state),
            Self::Async(_) => Err(async_required()),
        }
    }
    fn before<T: 'a>(self, prepare: impl Fn(T) -> Result<S> + Copy + 'a) -> Continuation<'a, T> {
        match self {
            Self::Sync(run) => Continuation::sync(move |state| run(prepare(state)?)),
            Self::Async(run) => Continuation::asynchronous(move |state| {
                let run = run.clone();
                Box::pin(async move { run(prepare(state)?).await })
            }),
        }
    }
    fn after(self, finish: impl Fn(Result<()>) -> Result<()> + Copy + 'a) -> Self {
        match self {
            Self::Sync(run) => Self::sync(move |state| finish(run(state))),
            Self::Async(run) => Self::asynchronous(move |state| {
                let run = run.clone();
                Box::pin(async move { finish(run(state).await) })
            }),
        }
    }
}

trait Stage<S>: Send + Sync {
    fn is_async(&self) -> bool;
    fn describe(&self, stages: &mut Vec<StageInfo>);
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()>;
}
fn describe_after<S>(previous: &dyn Stage<S>, info: &StageInfo, stages: &mut Vec<StageInfo>) {
    previous.describe(stages);
    stages.push(info.clone());
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
struct Named<S> {
    previous: Arc<dyn Stage<S>>,
    label: &'static str,
}
impl<S: 'static> Stage<S> for Named<S> {
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        self.previous.describe(stages);
        if let Some(last) = stages.last_mut() {
            last.handler = self.label;
        }
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        self.previous.compose(ctx, next)
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
        describe_after(&*self.previous, &self.info, stages);
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
        self.previous.compose(ctx, next.before(derive))
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
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(
        &'a self,
        ctx: &'a Context,
        next: Continuation<'a, D::Output>,
    ) -> Continuation<'a, ()> {
        self.previous.compose(
            ctx,
            Continuation::asynchronous(move |state| {
                let next = next.clone();
                Box::pin(async move {
                    let value = self
                        .handler
                        .call(scope(ctx), self.inputs.read(&ctx.inputs)?, state)
                        .await?;
                    next.call(value).await
                })
            }),
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
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        self.previous.compose(
            ctx,
            match next {
                Continuation::Sync(run) => Continuation::sync(move |state| {
                    self.handler.call(
                        scope(ctx),
                        self.inputs.read(&ctx.inputs)?,
                        state,
                        Next { run: run.clone() },
                    )
                }),
                Continuation::Async(_) => Continuation::sync(|_| Err(async_middleware_required())),
            },
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
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        self.previous.compose(
            ctx,
            Continuation::asynchronous(move |state| {
                let next = next.clone();
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
            }),
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
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        let catch = move |result: Result<()>| match result {
            Err(error) => (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, error),
            ok => ok,
        };
        self.previous.compose(ctx, next.after(catch))
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
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        let finish = move |result: Result<()>| {
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &result);
            result
        };
        self.previous.compose(ctx, next.after(finish))
    }
}
struct Retried<S> {
    info: StageInfo,
    previous: Arc<dyn Stage<S>>,
    retries: usize,
}
impl<S: Clone + 'static> Stage<S> for Retried<S> {
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        let retries = self.retries;
        self.previous.compose(
            ctx,
            match next {
                Continuation::Sync(run) => Continuation::sync(move |state: S| {
                    for _ in 0..retries {
                        if run(state.clone()).is_ok() {
                            return Ok(());
                        }
                    }
                    run(state)
                }),
                Continuation::Async(run) => Continuation::asynchronous(move |state: S| {
                    let run = run.clone();
                    Box::pin(async move {
                        for _ in 0..retries {
                            if run(state.clone()).await.is_ok() {
                                return Ok(());
                            }
                        }
                        run(state).await
                    })
                }),
            },
        )
    }
}
struct TimedOut<S, F> {
    info: StageInfo,
    previous: Arc<dyn Stage<S>>,
    limit: Duration,
    sleep: F,
}
impl<S: 'static, F, Fut> Stage<S> for TimedOut<S, F>
where
    F: Fn(Duration) -> Fut + Send + Sync,
    Fut: Future<Output = ()> + 'static,
{
    fn is_async(&self) -> bool {
        true
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        self.previous.compose(
            ctx,
            Continuation::asynchronous(move |state| {
                let next = next.clone();
                Box::pin(async move {
                    let mut work = next.call(state);
                    let mut sleep = pin!((self.sleep)(self.limit));
                    std::future::poll_fn(|cx| {
                        if let Poll::Ready(result) = work.as_mut().poll(cx) {
                            return Poll::Ready(result);
                        }
                        sleep.as_mut().poll(cx).map(|()| Err(timed_out(self.limit)))
                    })
                    .await
                })
            }),
        )
    }
}
fn timed_out(limit: Duration) -> CliError {
    CliError::new("TIMEOUT", format!("Exceeded the {limit:.2?} limit"))
}
struct SharedStage<S> {
    info: StageInfo,
    previous: Arc<dyn Stage<()>>,
    id: usize,
    state: PhantomData<fn() -> S>,
}
impl<S: Clone + Send + 'static> Stage<S> for SharedStage<S> {
    fn is_async(&self) -> bool {
        self.previous.is_async()
    }
    fn describe(&self, stages: &mut Vec<StageInfo>) {
        describe_after(&*self.previous, &self.info, stages);
    }
    fn compose<'a>(&'a self, ctx: &'a Context, next: Continuation<'a, S>) -> Continuation<'a, ()> {
        let id = self.id;
        self.previous
            .compose(ctx, next.before(move |()| read_shared::<S>(ctx, id)))
    }
}
fn produce<'a, S: 'a>(
    stage: &'a dyn Stage<S>,
    ctx: &'a Context,
) -> (Rc<RefCell<Option<S>>>, Continuation<'a, ()>) {
    let slot = Rc::new(RefCell::new(None));
    let sink = slot.clone();
    let next = Continuation::sync(move |state| {
        sink.replace(Some(state));
        Ok(())
    });
    (slot, stage.compose(ctx, next))
}
fn store<S: Send + 'static>(
    ctx: &mut Context,
    id: usize,
    slot: Rc<RefCell<Option<S>>>,
) -> Result<()> {
    let Some(value) = slot.take() else {
        return Err(CliError::new(
            "SHARED_STATE_MISSING",
            format!(
                "The shared {} pipeline ended before producing its state",
                type_name::<S>()
            ),
        )
        .suggest("Call next.run in every middleware of the pipeline"));
    };
    store_shared(ctx, id, value);
    Ok(())
}
struct SharedStep<S> {
    id: usize,
    stage: Arc<dyn Stage<S>>,
}
impl<S: Send + 'static> SharedStep<S> {
    fn run_sync(&self, ctx: &mut Context) -> Result<()> {
        let slot = {
            let shared: &Context = ctx;
            let (slot, run) = produce(&*self.stage, shared);
            run.call_sync(())?;
            slot
        };
        store(ctx, self.id, slot)
    }
}
impl<S: Send + 'static> crate::execution::AsyncStep for SharedStep<S> {
    fn call<'a>(
        &'a self,
        ctx: &'a mut Context,
        next: crate::execution::AsyncNext<'a>,
    ) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            let slot = {
                let shared: &Context = ctx;
                let (slot, run) = produce(&*self.stage, shared);
                run.call(()).await?;
                slot
            };
            store(ctx, self.id, slot)?;
            next.run(ctx).await
        })
    }
}
impl<G, S: Send + 'static, M> Pipeline<G, S, M> {
    pub(crate) fn declared(&self) -> &[InputSchema] {
        &self.declared
    }
    pub(crate) fn step(&self) -> Step {
        let step = Arc::new(SharedStep {
            id: self.id(),
            stage: self.state.clone(),
        });
        if step.stage.is_async() {
            Step::Async(step)
        } else {
            Step::Derive(Arc::new(move |ctx: &mut Context| step.run_sync(ctx)))
        }
    }
}
struct BoundSync<I, S, F> {
    inputs: Arc<I>,
    state: Arc<dyn Stage<S>>,
    handler: F,
}
impl<I: InputSet, S: 'static, F> BoundSync<I, S, F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync,
{
    fn compose<'a>(&'a self, ctx: &'a Context) -> Continuation<'a, ()> {
        let next = Continuation::sync(move |state: S| {
            (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &state)
        });
        self.state.compose(ctx, next)
    }
    fn run(&self, ctx: &mut Context) -> Result<()> {
        self.compose(ctx).call_sync(())
    }
}
impl<I: InputSet, S: 'static, F> AsyncAction for BoundSync<I, S, F>
where
    F: for<'a> Fn(Scope<'a>, I::Values<'a>, &'a S) -> Result<()> + Send + Sync,
{
    fn call<'a>(&'a self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>> {
        self.compose(ctx).call(())
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
        let next = Continuation::asynchronous(move |state: S| {
            Box::pin(async move {
                (self.handler)(scope(ctx), self.inputs.read(&ctx.inputs)?, &state).await
            })
        });
        self.state.compose(ctx, next).call(())
    }
}
