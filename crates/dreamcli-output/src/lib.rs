//! Synchronous, writer-based output with shared capture and RAII activity handles.
use dreamcli_error::CliError;
use dreamcli_error::Result;
use serde::Serialize;
use serde_json::Value;
use std::fmt::Display;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,
    #[default]
    Normal,
    Verbose,
}
#[derive(Debug, Clone, Default)]
pub struct OutputOptions {
    pub json: bool,
    pub tty: bool,
    pub verbosity: Verbosity,
    pub color: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capture {
    pub stdout: String,
    pub stderr: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActivityEvent {
    Start {
        label: String,
    },
    Update {
        label: String,
        current: u64,
        total: u64,
    },
    Finish {
        label: String,
        success: bool,
    },
}
struct State {
    capture: Capture,
    stdout: Option<Box<dyn Write + Send>>,
    stderr: Option<Box<dyn Write + Send>>,
    error: Option<CliError>,
    exit_code: i32,
    events: Vec<ActivityEvent>,
}
#[derive(Clone)]
pub struct Output {
    options: OutputOptions,
    state: Arc<Mutex<State>>,
}
impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}
impl Output {
    pub fn capture(options: OutputOptions) -> Self {
        Self {
            options,
            state: Arc::new(Mutex::new(State {
                capture: Capture::default(),
                stdout: None,
                stderr: None,
                error: None,
                exit_code: 0,
                events: vec![],
            })),
        }
    }
    /// All writes finish before this method returns; writer errors reach `finish`.
    pub fn with_writers(
        stdout: impl Write + Send + 'static,
        stderr: impl Write + Send + 'static,
        options: OutputOptions,
    ) -> Self {
        let output = Self::capture(options);
        {
            let mut state = output.state.lock().expect("output lock");
            state.stdout = Some(Box::new(stdout));
            state.stderr = Some(Box::new(stderr));
        }
        output
    }
    pub fn options(&self) -> &OutputOptions {
        &self.options
    }
    pub fn with_options(&self, options: OutputOptions) -> Self {
        Self {
            options,
            state: self.state.clone(),
        }
    }
    pub fn captured(&self) -> Capture {
        self.state.lock().expect("output lock").capture.clone()
    }
    pub fn events(&self) -> Vec<ActivityEvent> {
        self.state.lock().expect("output lock").events.clone()
    }
    pub fn exit_code(&self) -> i32 {
        self.state.lock().expect("output lock").exit_code
    }
    pub fn set_exit_code(&self, code: i32) {
        self.state.lock().expect("output lock").exit_code = code;
    }
    pub fn finish(&self) -> Result<()> {
        self.state
            .lock()
            .expect("output lock")
            .error
            .clone()
            .map_or(Ok(()), Err)
    }
    pub fn is_tty(&self) -> bool {
        self.options.tty
    }
    pub fn log(&self, message: impl Display) {
        self.write(&format!("{message}\n"), self.options.json);
    }
    pub fn info(&self, message: impl Display) {
        if self.options.verbosity != Verbosity::Quiet {
            self.write(&format!("{message}\n"), self.options.json);
        }
    }
    pub fn status(&self, message: impl Display) {
        if self.options.verbosity != Verbosity::Quiet {
            self.write(&format!("{message}\n"), true);
        }
    }
    pub fn debug(&self, message: impl Display) {
        if self.options.verbosity == Verbosity::Verbose {
            self.write(&format!("{message}\n"), true);
        }
    }
    pub fn warn(&self, message: impl Display) {
        self.write(&format!("{message}\n"), true);
    }
    pub fn error(&self, message: impl Display) {
        self.write(&format!("{message}\n"), true);
    }
    pub fn json(&self, value: &impl Serialize) -> Result<()> {
        self.write(&format!("{}\n", serde_json::to_string(value)?), false);
        self.finish()
    }
    pub fn raw_stdout(&self, text: &str) {
        self.write(text, false);
    }
    pub fn render_error(&self, error: &CliError) {
        if self.options.json {
            let _ = self.json(error);
        } else {
            self.error(&error.message);
            if let Some(suggest) = &error.suggest {
                self.error(format!("Hint: {suggest}"));
            }
        }
    }
    pub fn color(&self, code: u8, text: impl Display) -> String {
        if self.options.color && self.options.tty && !self.options.json {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
    fn write(&self, text: &str, stderr: bool) {
        let mut state = self.state.lock().expect("output lock");
        if stderr {
            state.capture.stderr.push_str(text);
        } else {
            state.capture.stdout.push_str(text);
        }
        let writer = if stderr {
            &mut state.stderr
        } else {
            &mut state.stdout
        };
        if let Some(writer) = writer
            && let Err(error) = writer
                .write_all(text.as_bytes())
                .and_then(|_| writer.flush())
        {
            state.error.get_or_insert_with(|| error.into());
        }
    }
    fn event(&self, event: ActivityEvent) {
        self.state.lock().expect("output lock").events.push(event);
    }
    pub fn table(&self, rows: &[Value], columns: &[Column], format: TableFormat) -> Result<()> {
        if self.options.json || format == TableFormat::Json {
            return self.json(&rows);
        }
        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|col| display_value(row.get(&col.key).unwrap_or(&Value::Null)))
                    .collect()
            })
            .collect();
        match format {
            TableFormat::Csv | TableFormat::Tsv => {
                let sep = if format == TableFormat::Csv {
                    ','
                } else {
                    '\t'
                };
                let escape = |text: &str| {
                    if text.contains([sep, '"', '\n', '\r']) {
                        format!("\"{}\"", text.replace('"', "\"\""))
                    } else {
                        text.to_string()
                    }
                };
                self.raw_stdout(&format!(
                    "{}\n",
                    columns
                        .iter()
                        .map(|c| escape(&c.header))
                        .collect::<Vec<_>>()
                        .join(&sep.to_string())
                ));
                for row in cells {
                    self.raw_stdout(&format!(
                        "{}\n",
                        row.iter()
                            .map(|c| escape(c))
                            .collect::<Vec<_>>()
                            .join(&sep.to_string())
                    ));
                }
            }
            _ => {
                let widths: Vec<_> = columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        cells
                            .iter()
                            .map(|r| visible_width(&r[i]))
                            .fold(visible_width(&c.header), usize::max)
                    })
                    .collect();
                let line = |row: &[String]| {
                    row.iter()
                        .enumerate()
                        .map(|(i, c)| {
                            format!(
                                "{}{}",
                                c,
                                " ".repeat(widths[i].saturating_sub(visible_width(c)))
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("  ")
                        .trim_end()
                        .to_string()
                };
                self.raw_stdout(&format!(
                    "{}\n",
                    line(&columns.iter().map(|c| c.header.clone()).collect::<Vec<_>>())
                ));
                for row in cells {
                    self.raw_stdout(&format!("{}\n", line(&row)));
                }
            }
        }
        self.finish()
    }
    pub fn spinner(&self, label: impl Into<String>) -> Spinner {
        let label = label.into();
        self.event(ActivityEvent::Start {
            label: label.clone(),
        });
        if self.activity_visible() {
            self.write(&format!("{label}…\r"), true);
        }
        Spinner {
            output: self.clone(),
            label,
            finished: false,
        }
    }
    pub fn progress(&self, label: impl Into<String>, total: u64) -> Progress {
        let label = label.into();
        self.event(ActivityEvent::Start {
            label: label.clone(),
        });
        Progress {
            output: self.clone(),
            label,
            total,
            current: 0,
            finished: false,
        }
    }
    fn activity_visible(&self) -> bool {
        self.options.tty && !self.options.json && self.options.verbosity != Verbosity::Quiet
    }
}
#[derive(Debug, Clone)]
pub struct Column {
    pub key: String,
    pub header: String,
}
impl Column {
    pub fn new(key: impl Into<String>, header: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            header: header.into(),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TableFormat {
    #[default]
    Pretty,
    Csv,
    Tsv,
    Json,
}
pub struct Spinner {
    output: Output,
    label: String,
    finished: bool,
}
impl Spinner {
    pub fn update(&mut self, label: impl Into<String>) {
        self.label = label.into();
        if self.output.activity_visible() {
            self.output
                .write(&format!("\r\x1b[2K{}…", self.label), true);
        }
    }
    pub fn succeed(mut self, message: impl Into<String>) {
        self.finish(message.into(), true);
    }
    pub fn fail(mut self, message: impl Into<String>) {
        self.finish(message.into(), false);
    }
    fn finish(&mut self, label: String, success: bool) {
        self.finished = true;
        self.output.event(ActivityEvent::Finish {
            label: label.clone(),
            success,
        });
        if self.output.activity_visible() {
            self.output.write(&format!("\r\x1b[2K{label}\n"), true);
        }
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
    finished: bool,
}
impl Progress {
    pub fn update(&mut self, current: u64) {
        self.current = current.min(self.total);
        self.output.event(ActivityEvent::Update {
            label: self.label.clone(),
            current: self.current,
            total: self.total,
        });
        if self.output.activity_visible() {
            self.output.write(
                &format!("\r\x1b[2K{} {}/{}", self.label, self.current, self.total),
                true,
            );
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
        self.output.event(ActivityEvent::Finish {
            label: label.clone(),
            success,
        });
        if self.output.activity_visible() {
            self.output.write(&format!("\r\x1b[2K{label}\n"), true);
        }
    }
}
impl Drop for Progress {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(self.label.clone(), false);
        }
    }
}
fn display_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}
pub fn visible_width(text: &str) -> usize {
    let mut visible = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            visible.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.peek() == Some(&'\\')) {
                        if c == '\x1b' {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    UnicodeWidthStr::width(visible.as_str())
}
pub fn osc8(label: &str, url: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn json_routes_human_messages_to_stderr() {
        let out = Output::capture(OutputOptions {
            json: true,
            ..Default::default()
        });
        out.log("hello");
        out.json(&json!({"ok":true})).unwrap();
        assert_eq!(
            out.captured(),
            Capture {
                stdout: "{\"ok\":true}\n".into(),
                stderr: "hello\n".into()
            }
        );
    }
    #[test]
    fn quiet_keeps_logs_warnings_and_errors() {
        let out = Output::capture(OutputOptions {
            verbosity: Verbosity::Quiet,
            ..Default::default()
        });
        out.log("data");
        out.info("info");
        out.status("status");
        out.warn("warning");
        assert_eq!(
            out.captured(),
            Capture {
                stdout: "data\n".into(),
                stderr: "warning\n".into()
            }
        );
    }
    #[test]
    fn activities_clean_up_on_drop_and_are_silent_when_piped() {
        let out = Output::capture(Default::default());
        {
            let _spinner = out.spinner("work");
            let mut progress = out.progress("files", 3);
            progress.update(1);
            progress.done("done");
        }
        assert_eq!(out.captured(), Capture::default());
        assert_eq!(out.events().len(), 5);
    }
    #[test]
    fn width_handles_unicode_and_escape_sequences() {
        assert_eq!(visible_width("\x1b[31m你好\x1b[0m"), 4);
        assert_eq!(visible_width(&osc8("hi", "https://example.com")), 2);
    }
    #[test]
    fn csv_quotes_delimiters_and_newlines() {
        let out = Output::capture(Default::default());
        out.table(
            &[json!({"name":"a,\"b\"\nc"})],
            &[Column::new("name", "Name")],
            TableFormat::Csv,
        )
        .unwrap();
        assert_eq!(out.captured().stdout, "Name\n\"a,\"\"b\"\"\nc\"\n");
    }
    #[test]
    fn writer_failures_are_not_reported_as_success() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("broken writer"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let out = Output::with_writers(Broken, std::io::sink(), Default::default());
        out.log("data");
        assert_eq!(out.finish().unwrap_err().code, "IO_ERROR");
    }
}
