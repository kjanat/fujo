use crate::Output;
use crate::State;
use crate::lock;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::thread::{self};
use std::time::Duration;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub(super) const TICK: Duration = Duration::from_millis(80);

pub(crate) struct Activities {
    pub(super) active: Vec<Line>,
    next_id: u64,
    drawn: bool,
    pub(crate) line_start: bool,
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
pub(super) struct Line {
    pub(super) id: u64,
    pub(super) label: String,
    pub(super) kind: Kind,
}
pub(super) enum Kind {
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
    pub(crate) fn clear_activity(&mut self) {
        if self.activities.drawn {
            self.emit("\r\x1b[2K", true, false);
            self.activities.drawn = false;
        }
    }
    pub(crate) fn render_activity(&mut self) {
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
    pub(super) fn begin_activity(&mut self, label: String, kind: Kind) -> u64 {
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

pub(super) struct Ticker {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}
impl Ticker {
    pub(super) fn start(output: &Output, id: Option<u64>) -> Option<Self> {
        id.and_then(|id| {
            output.state().stderr.as_ref()?;
            let state = Arc::downgrade(&output.state);
            let (stop, stopped) = mpsc::channel();
            match thread::Builder::new()
                .name("terminal-spinner".into())
                .spawn(move || {
                    while matches!(
                        stopped.recv_timeout(TICK),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        let Some(state) = state.upgrade() else { break };
                        let mut state = lock(&state);
                        if state.error.is_some() {
                            break;
                        }
                        state.tick(id);
                    }
                }) {
                Ok(thread) => Some(Ticker { stop, thread }),
                Err(error) => {
                    output.state().error.get_or_insert_with(|| error.into());
                    None
                }
            }
        })
    }
    pub(super) fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
    }
}
