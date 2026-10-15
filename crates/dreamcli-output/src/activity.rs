//! A shared terminal line, with handle-owned spinner timers and logical events.
use super::ActivityEvent;
use super::Output;
use super::State;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::thread::{self};
use std::time::Duration;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TICK: Duration = Duration::from_millis(80);

pub(super) struct Activities {
    active: Vec<Line>,
    next_id: u64,
    drawn: bool,
    pub(super) line_start: bool,
}
impl Default for Activities {
    fn default() -> Self {
        Self {
            active: vec![],
            next_id: 0,
            drawn: false,
            line_start: true,
        }
    }
}
struct Line {
    id: u64,
    label: String,
    kind: Kind,
}
enum Kind {
    Spinner(usize),
    Progress { current: u64, total: u64 },
}
impl Line {
    fn render(&self) -> String {
        match self.kind {
            Kind::Spinner(frame) => format!("{} {}", FRAMES[frame], self.label),
            Kind::Progress { current, total } => {
                let percent = if total == 0 {
                    100
                } else {
                    (u128::from(current) * 100 / u128::from(total)) as usize
                };
                let filled = percent / 5;
                format!(
                    "{} [{}{}] {percent:>3}% {current}/{total}",
                    self.label,
                    "=".repeat(filled),
                    " ".repeat(20 - filled)
                )
            }
        }
    }
}
impl State {
    pub(super) fn clear_activity(&mut self) {
        if self.activities.drawn {
            self.emit("\r\x1b[2K", true, false);
            self.activities.drawn = false;
        }
    }
    pub(super) fn render_activity(&mut self) {
        if !self.activities.line_start || self.error.is_some() {
            return;
        }
        if let Some(line) = self.activities.active.last() {
            let text = line.render();
            self.clear_activity();
            self.emit(&text, true, false);
            self.activities.drawn = true;
        }
    }
    fn begin_activity(&mut self, label: String, kind: Kind) -> u64 {
        let id = self.activities.next_id;
        self.activities.next_id += 1;
        self.activities.active.push(Line { id, label, kind });
        self.render_activity();
        id
    }
    fn tick(&mut self, id: u64) {
        if let Some(line) = self.activities.active.iter_mut().find(|line| line.id == id)
            && let Kind::Spinner(frame) = &mut line.kind
        {
            *frame = (*frame + 1) % FRAMES.len();
            if self
                .activities
                .active
                .last()
                .is_some_and(|line| line.id == id)
            {
                self.render_activity();
            }
        }
    }
}

impl Output {
    /// Animate on a TTY every 80 ms, even while the handler is blocked doing work.
    /// JSON, quiet, and non-TTY outputs remain silent. Capture-only outputs have
    /// no timer; use `events()` to inspect the logical lifecycle in tests.
    pub fn spinner(&self, label: impl Into<String>) -> Spinner {
        let label = label.into();
        let id = self.begin_activity(&label, Kind::Spinner(0));
        let ticker = id.and_then(|id| {
            self.state.lock().expect("output lock").stderr.as_ref()?;
            let state = Arc::downgrade(&self.state);
            let (stop, stopped) = mpsc::channel();
            match thread::Builder::new()
                .name("terminal-spinner".into())
                .spawn(move || {
                    while matches!(
                        stopped.recv_timeout(TICK),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        let Some(state) = state.upgrade() else { break };
                        let mut state = state.lock().expect("output lock");
                        if state.error.is_some() {
                            break;
                        }
                        state.tick(id);
                    }
                }) {
                Ok(thread) => Some(Ticker { stop, thread }),
                Err(error) => {
                    self.state
                        .lock()
                        .expect("output lock")
                        .error
                        .get_or_insert_with(|| error.into());
                    None
                }
            }
        });
        Spinner {
            output: self.clone(),
            label,
            id,
            ticker,
            finished: false,
        }
    }
    /// Render a bounded bar with a percentage and item count on every update.
    pub fn progress(&self, label: impl Into<String>, total: u64) -> Progress {
        let label = label.into();
        let id = self.begin_activity(&label, Kind::Progress { current: 0, total });
        Progress {
            output: self.clone(),
            label,
            total,
            current: 0,
            id,
            finished: false,
        }
    }
    fn begin_activity(&self, label: &str, kind: Kind) -> Option<u64> {
        self.event(ActivityEvent::Start {
            label: label.into(),
        });
        self.activity_visible().then(|| {
            self.state
                .lock()
                .expect("output lock")
                .begin_activity(label.into(), kind)
        })
    }
    fn end_activity(&self, id: Option<u64>, label: String, success: bool) {
        let mut state = self.state.lock().expect("output lock");
        state.events.push(ActivityEvent::Finish {
            label: label.clone(),
            success,
        });
        if let Some(id) = id {
            state.clear_activity();
            state.activities.active.retain(|line| line.id != id);
            let symbol = if success { "✓" } else { "✗" };
            state.write(&format!("{symbol} {label}\n"), true);
        }
    }
}

struct Ticker {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}
impl Ticker {
    fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
    }
}

/// A spinner whose timer stops and joins before completion or `Drop` returns.
pub struct Spinner {
    output: Output,
    label: String,
    id: Option<u64>,
    ticker: Option<Ticker>,
    finished: bool,
}
impl Spinner {
    pub fn update(&mut self, label: impl Into<String>) {
        self.label = label.into();
        let mut state = self.output.state.lock().expect("output lock");
        if let Some(line) = state
            .activities
            .active
            .iter_mut()
            .find(|line| Some(line.id) == self.id)
        {
            line.label.clone_from(&self.label);
            state.render_activity();
        }
    }
    pub fn succeed(mut self, message: impl Into<String>) {
        self.finish(message.into(), true);
    }
    pub fn fail(mut self, message: impl Into<String>) {
        self.finish(message.into(), false);
    }
    fn finish(&mut self, label: String, success: bool) {
        if let Some(ticker) = self.ticker.take() {
            ticker.stop();
        }
        self.finished = true;
        self.output.end_activity(self.id, label, success);
    }
}
impl Drop for Spinner {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(self.label.clone(), false);
        }
    }
}

pub struct Progress {
    output: Output,
    label: String,
    total: u64,
    current: u64,
    id: Option<u64>,
    finished: bool,
}
impl Progress {
    pub fn update(&mut self, current: u64) {
        self.current = current.min(self.total);
        let mut state = self.output.state.lock().expect("output lock");
        state.events.push(ActivityEvent::Update {
            label: self.label.clone(),
            current: self.current,
            total: self.total,
        });
        if let Some(line) = state
            .activities
            .active
            .iter_mut()
            .find(|line| Some(line.id) == self.id)
        {
            line.kind = Kind::Progress {
                current: self.current,
                total: self.total,
            };
            state.render_activity();
        }
    }
    pub fn increment(&mut self, delta: u64) {
        self.update(self.current.saturating_add(delta));
    }
    pub fn done(mut self, message: impl Into<String>) {
        self.finish(message.into(), true);
    }
    pub fn fail(mut self, message: impl Into<String>) {
        self.finish(message.into(), false);
    }
    fn finish(&mut self, label: String, success: bool) {
        self.finished = true;
        self.output.end_activity(self.id, label, success);
    }
}
impl Drop for Progress {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(self.label.clone(), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Capture;
    use crate::OutputOptions;
    use crate::Verbosity;
    use std::io::Write;
    use std::sync::Mutex;
    use std::time::Instant;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Buffer {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }
    fn tty() -> OutputOptions {
        OutputOptions {
            tty: true,
            ..Default::default()
        }
    }

    #[test]
    fn spinner_ticks_while_handler_waits_and_stops_before_finish_or_drop_returns() {
        struct Notify(mpsc::Sender<String>);
        impl Write for Notify {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0
                    .send(String::from_utf8_lossy(bytes).into_owned())
                    .unwrap();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        for completion in ["success", "failure", "drop"] {
            let (tx, rx) = mpsc::channel();
            let out = Output::with_writers(std::io::sink(), Notify(tx), tty());
            let spinner = out.spinner("waiting");
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let text = rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("spinner did not tick");
                if text.contains("⠙ waiting") {
                    break;
                }
            }
            match completion {
                "success" => spinner.succeed("finished"),
                "failure" => spinner.fail("failed"),
                _ => drop(spinner),
            }
            let remaining: String = rx.try_iter().collect();
            assert!(remaining.ends_with('\n'));
            assert_eq!(
                rx.recv_timeout(TICK * 2),
                Err(mpsc::RecvTimeoutError::Timeout)
            );
            assert_eq!(out.events().len(), 2);
            assert!(!out.captured().stderr.contains('\x1b'));
            assert!(!out.captured().stderr.contains('⠙'));
            out.finish().unwrap();
        }
    }

    #[test]
    fn status_tables_and_partial_writes_preserve_the_shared_line() {
        let buffer = Buffer::default();
        let out = Output::with_writers(buffer.clone(), buffer.clone(), tty());
        let mut progress = out.progress("files", 2);
        out.status("message");
        out.table(
            &[serde_json::json!({"name":"one"})],
            &[crate::Column::new("name", "Name")],
            crate::TableFormat::Pretty,
        )
        .unwrap();
        out.raw_stdout("partial");
        progress.update(1); // Must not paint over a partial application line.
        assert!(buffer.text().ends_with("partial"));
        out.raw_stdout(" line\n");
        assert!(
            buffer
                .text()
                .ends_with("files [==========          ]  50% 1/2")
        );
        progress.done("done");
        let text = buffer.text();
        assert!(text.contains("\r\x1b[2Kmessage\nfiles ["));
        assert!(text.contains("\r\x1b[2KName\nfiles ["));
        assert!(text.ends_with("\r\x1b[2K✓ done\n"));
        assert_eq!(
            out.captured(),
            Capture {
                stdout: "Name\none\npartial line\n".into(),
                stderr: "message\n✓ done\n".into()
            }
        );
    }

    #[test]
    fn errors_from_timer_writes_reach_finish_and_stop_further_ticks() {
        struct FailsAfterStart {
            first: bool,
            failed: mpsc::Sender<()>,
        }
        impl Write for FailsAfterStart {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if std::mem::take(&mut self.first) {
                    return Ok(bytes.len());
                }
                self.failed.send(()).unwrap();
                Err(std::io::Error::other("terminal disconnected"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (tx, rx) = mpsc::channel();
        let out = Output::with_writers(
            std::io::sink(),
            FailsAfterStart {
                first: true,
                failed: tx,
            },
            tty(),
        );
        let spinner = out.spinner("work");
        rx.recv_timeout(Duration::from_secs(3))
            .expect("timer did not write");
        assert_eq!(out.finish().unwrap_err().code, "IO_ERROR");
        // Drain the failed tick (clear + frame); later ticks must not keep writing.
        let _ = rx.try_iter().count();
        assert_eq!(
            rx.recv_timeout(TICK * 2),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        drop(spinner);
        assert_eq!(out.events().len(), 2);
    }

    #[test]
    fn nested_handles_restore_outer_activity_and_clamp_progress_without_overflow() {
        let buffer = Buffer::default();
        let out = Output::with_writers(std::io::sink(), buffer.clone(), tty());
        let mut outer = out.progress("outer", u64::MAX);
        let inner = out.progress("inner", 0);
        outer.increment(u64::MAX);
        assert!(
            buffer
                .text()
                .ends_with("inner [====================] 100% 0/0")
        );
        inner.done("inner done");
        assert!(buffer.text().ends_with(&format!(
            "outer [====================] 100% {0}/{0}",
            u64::MAX
        )));
        outer.increment(1);
        drop(outer);
        assert!(buffer.text().ends_with("✗ outer\n"));
        assert_eq!(out.events().len(), 6);
    }

    #[test]
    fn json_quiet_and_piped_outputs_never_render_activity_frames() {
        for options in [
            OutputOptions::default(),
            OutputOptions {
                json: true,
                ..tty()
            },
            OutputOptions {
                verbosity: Verbosity::Quiet,
                ..tty()
            },
        ] {
            let buffer = Buffer::default();
            let out = Output::with_writers(buffer.clone(), buffer.clone(), options);
            let mut spinner = out.spinner("hidden");
            assert!(spinner.ticker.is_none());
            spinner.update("still hidden");
            let mut progress = out.progress("hidden", 1);
            progress.increment(1);
            progress.done("done");
            spinner.succeed("done");
            assert!(buffer.text().is_empty());
            assert_eq!(out.events().len(), 5);
        }
        let out = Output::capture(tty());
        assert!(out.spinner("capture has no timer").ticker.is_none());
    }
}
