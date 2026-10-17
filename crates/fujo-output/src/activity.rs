use super::ActivityEvent;
use super::Output;

#[cfg(feature = "terminal-activity")]
mod terminal;
#[cfg(feature = "terminal-activity")]
pub(super) use terminal::Activities;
#[cfg(feature = "terminal-activity")]
use terminal::Kind;
#[cfg(feature = "terminal-activity")]
use terminal::Ticker;

impl Output {
    pub fn spinner(&self, label: impl Into<String>) -> Spinner {
        let label = label.into();
        let id = self.begin_activity(&label, None);
        #[cfg(feature = "terminal-activity")]
        let ticker = Ticker::start(self, id);
        Spinner {
            output: self.clone(),
            label,
            id,
            #[cfg(feature = "terminal-activity")]
            ticker,
            finished: false,
        }
    }
    pub fn progress(&self, label: impl Into<String>, total: u64) -> Progress {
        let label = label.into();
        let id = self.begin_activity(&label, Some(total));
        Progress {
            output: self.clone(),
            label,
            total,
            current: 0,
            id,
            finished: false,
        }
    }
    fn begin_activity(&self, label: &str, total: Option<u64>) -> Option<u64> {
        self.event(ActivityEvent::Start {
            label: label.into(),
        });
        #[cfg(feature = "terminal-activity")]
        {
            self.activity_visible().then(|| {
                self.state().begin_activity(
                    label.into(),
                    total.map_or(Kind::Spinner(0), |total| Kind::Progress {
                        current: 0,
                        total,
                    }),
                )
            })
        }
        #[cfg(not(feature = "terminal-activity"))]
        {
            let _ = total;
            None
        }
    }
    fn end_activity(&self, _id: Option<u64>, label: String, success: bool) {
        let mut state = self.state();
        state.events.push(ActivityEvent::Finish {
            label: label.clone(),
            success,
        });
        #[cfg(feature = "terminal-activity")]
        if let Some(id) = _id {
            state.clear_activity();
            state.activities.active.retain(|line| line.id != id);
            let symbol = if success { "✓" } else { "✗" };
            state.write(&format!("{symbol} {label}\n"), true);
        }
    }
}

pub struct Spinner {
    output: Output,
    label: String,
    id: Option<u64>,
    #[cfg(feature = "terminal-activity")]
    ticker: Option<Ticker>,
    finished: bool,
}
impl std::fmt::Debug for Spinner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spinner")
            .field("output", &self.output)
            .field("label", &self.label)
            .field("id", &self.id)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}
impl Spinner {
    pub fn update(&mut self, label: impl Into<String>) {
        self.label = label.into();
        #[cfg(feature = "terminal-activity")]
        {
            let mut state = self.output.state();
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
    }
    pub fn succeed(mut self, message: impl Into<String>) {
        self.finish(message.into(), true);
    }
    pub fn fail(mut self, message: impl Into<String>) {
        self.finish(message.into(), false);
    }
    fn finish(&mut self, label: String, success: bool) {
        #[cfg(feature = "terminal-activity")]
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

#[derive(Debug)]
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
        let mut state = self.output.state();
        state.events.push(ActivityEvent::Update {
            label: self.label.clone(),
            current: self.current,
            total: self.total,
        });
        #[cfg(feature = "terminal-activity")]
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

#[cfg(all(test, feature = "terminal-activity"))]
mod tests {
    use super::terminal::TICK;
    use super::*;
    use crate::Capture;
    use crate::OutputOptions;
    use crate::Verbosity;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::time::Duration;
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
        progress.update(1);
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
        assert!(text.contains("\r\x1b[2KName\none\nfiles ["));
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
        let _ = rx.try_iter().count();
        assert_eq!(
            rx.recv_timeout(TICK * 2),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        drop(spinner);
        assert_eq!(out.events().len(), 2);
    }

    #[test]
    fn a_panicking_timer_write_poisons_the_lock_without_breaking_completion() {
        struct PanicsAfterStart {
            writes: usize,
            panicked: mpsc::Sender<()>,
        }
        impl Write for PanicsAfterStart {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.writes += 1;
                if self.writes != 2 {
                    return Ok(bytes.len());
                }
                self.panicked.send(()).unwrap();
                panic!("terminal writer panicked on a background tick");
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (tx, rx) = mpsc::channel();
        let out = Output::with_writers(
            std::io::sink(),
            PanicsAfterStart {
                writes: 0,
                panicked: tx,
            },
            tty(),
        );
        let spinner = out.spinner("work");
        rx.recv_timeout(Duration::from_secs(3))
            .expect("timer did not write");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !out.state.is_poisoned() {
            assert!(Instant::now() < deadline, "lock was not poisoned");
            std::thread::sleep(Duration::from_millis(5));
        }
        spinner.succeed("done");
        assert_eq!(out.events().len(), 2);
        assert!(out.captured().stderr.ends_with("✓ done\n"));
        drop(out.spinner("again"));
        assert_eq!(out.events().len(), 4);
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

    #[test]
    fn raw_stderr_clears_the_activity_line_and_redraws_after_a_newline() {
        let buffer = Buffer::default();
        let out = Output::with_writers(std::io::sink(), buffer.clone(), tty());
        let progress = out.progress("files", 2);
        assert!(
            buffer
                .text()
                .ends_with("files [                    ]   0% 0/2")
        );
        out.raw_stderr("Name?\n> ");
        assert!(buffer.text().ends_with("\r\x1b[2KName?\n> "));
        out.raw_stderr("Ada\n");
        assert!(
            buffer
                .text()
                .ends_with("> Ada\nfiles [                    ]   0% 0/2")
        );
        progress.done("done");
        assert_eq!(out.captured().stderr, "Name?\n> Ada\n✓ done\n");
    }

    #[test]
    fn handles_expose_their_state_through_debug() {
        let out = Output::capture(Default::default());
        let spinner = out.spinner("work");
        assert!(format!("{spinner:?}").starts_with("Spinner { output: Output {"));
        assert!(format!("{:?}", out.progress("items", 2)).contains("total: 2"));
    }
}
