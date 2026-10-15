use std::future::Future;
use std::future::poll_fn;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
pub fn run<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return result;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "test future did not finish within five seconds"
        );
        std::thread::park_timeout(remaining);
    }
}
pub async fn yield_once() {
    let mut yielded = false;
    poll_fn(move |ctx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            ctx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}
