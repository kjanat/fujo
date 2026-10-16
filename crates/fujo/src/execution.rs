//! Executor-independent invocation of synchronous and asynchronous callbacks.
use crate::CliError;
use crate::Context;
use crate::Result;
use crate::command::Middleware;
use crate::command::Next;
use crate::command::SyncAction;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A local future: an application chooses its executor and thread policy.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub(crate) trait AsyncAction: Send + Sync {
    fn call<'a>(&'a self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>>;
}
impl<F> AsyncAction for F
where
    F: for<'a> AsyncFn(&'a mut Context) -> Result<()> + Send + Sync,
{
    fn call<'a>(&'a self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>> {
        Box::pin(self(ctx))
    }
}
pub(crate) trait AsyncMiddleware: Send + Sync {
    fn call<'a>(&'a self, ctx: &'a mut Context, next: AsyncNext<'a>)
    -> LocalFuture<'a, Result<()>>;
}
impl<F> AsyncMiddleware for F
where
    F: for<'a> AsyncFn(&'a mut Context, AsyncNext<'a>) -> Result<()> + Send + Sync,
{
    fn call<'a>(
        &'a self,
        ctx: &'a mut Context,
        next: AsyncNext<'a>,
    ) -> LocalFuture<'a, Result<()>> {
        Box::pin(self(ctx, next))
    }
}
#[derive(Clone)]
pub(crate) enum Action {
    Sync(SyncAction),
    Async(Arc<dyn AsyncAction>),
}
#[derive(Clone)]
pub(crate) enum Step {
    Middleware(Middleware),
    Derive(SyncAction),
    Async(Arc<dyn AsyncMiddleware>),
}
pub(crate) fn async_required() -> CliError {
    CliError::new(
        "ASYNC_REQUIRED",
        "This command requires asynchronous execution",
    )
    .suggest("Use execute_async, run_with_async, or run_async and await the result")
}
pub(crate) fn async_middleware_required() -> CliError {
    CliError::new(
        "ASYNC_MIDDLEWARE_REQUIRED",
        "Synchronous middleware cannot wrap asynchronous work",
    )
    .suggest("Use middleware_async(async |ctx, next| { next.run(ctx).await })")
}
pub(crate) fn requires_async(steps: &[Step], action: Option<&Action>) -> bool {
    matches!(action, Some(Action::Async(_))) || steps.iter().any(|s| matches!(s, Step::Async(_)))
}
pub(crate) fn validate_async(steps: &[Step], action: Option<&Action>) -> Result<()> {
    let mut asynchronous = matches!(action, Some(Action::Async(_)));
    for step in steps.iter().rev() {
        match step {
            Step::Async(_) => asynchronous = true,
            Step::Middleware(_) if asynchronous => return Err(async_middleware_required()),
            _ => {}
        }
    }
    Ok(())
}

/// A consuming asynchronous continuation. Dropping it short-circuits the chain.
/// Synchronous actions and derives also work under asynchronous middleware.
///
/// ```compile_fail
/// use fujo::Command;
/// let command = Command::new("demo").middleware_async(async |ctx, next| {
///     next.run(ctx).await?;
///     next.run(ctx).await
/// });
/// ```
pub struct AsyncNext<'a> {
    pub(crate) steps: &'a [Step],
    pub(crate) action: Option<&'a Action>,
}
impl<'a> AsyncNext<'a> {
    pub fn run(self, ctx: &'a mut Context) -> LocalFuture<'a, Result<()>> {
        Box::pin(async move {
            if let Some((first, rest)) = self.steps.split_first() {
                let next = AsyncNext {
                    steps: rest,
                    action: self.action,
                };
                match first {
                    Step::Async(middleware) => middleware.call(ctx, next).await,
                    Step::Derive(derive) => {
                        derive(ctx)?;
                        next.run(ctx).await
                    }
                    Step::Middleware(middleware) => middleware(
                        ctx,
                        Next {
                            steps: rest,
                            action: self.action,
                        },
                    ),
                }
            } else {
                match self.action {
                    Some(Action::Sync(action)) => action(ctx),
                    Some(Action::Async(action)) => action.call(ctx).await,
                    None => Err(CliError::new("NO_ACTION", "Command has no action")),
                }
            }
        })
    }
}
