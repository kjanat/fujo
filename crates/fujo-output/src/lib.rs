mod activity;
pub use activity::Progress;
pub use activity::Spinner;
use fujo_error::CliError;
use fujo_error::Result;
use serde::Serialize;
use serde_json::Value;
use std::fmt::Display;
use std::fmt::Write as _;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
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
    #[cfg(feature = "terminal-activity")]
    activities: activity::Activities,
}
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
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
                #[cfg(feature = "terminal-activity")]
                activities: activity::Activities::default(),
            })),
        }
    }
    pub fn with_writers(
        stdout: impl Write + Send + 'static,
        stderr: impl Write + Send + 'static,
        options: OutputOptions,
    ) -> Self {
        let output = Self::capture(options);
        {
            let mut state = output.state();
            state.stdout = Some(Box::new(stdout));
            state.stderr = Some(Box::new(stderr));
        }
        output
    }
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
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
        self.state().capture.clone()
    }
    pub fn events(&self) -> Vec<ActivityEvent> {
        self.state().events.clone()
    }
    pub fn report(&self) -> (Capture, Vec<ActivityEvent>) {
        let state = self.state();
        (state.capture.clone(), state.events.clone())
    }
    pub fn exit_code(&self) -> i32 {
        self.state().exit_code
    }
    pub fn set_exit_code(&self, code: i32) {
        self.state().exit_code = code;
    }
    pub fn finish(&self) -> Result<()> {
        self.state().error.clone().map_or(Ok(()), Err)
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
    pub fn raw_stderr(&self, text: &str) {
        self.write(text, true);
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
        if self.options.color && !self.options.json {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
    fn write(&self, text: &str, stderr: bool) {
        self.state().write(text, stderr);
    }
    fn event(&self, event: ActivityEvent) {
        self.state().events.push(event);
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
        let mut text = String::new();
        match format {
            TableFormat::Csv | TableFormat::Tsv => {
                let (sep, joiner) = if format == TableFormat::Csv {
                    (',', ",")
                } else {
                    ('\t', "\t")
                };
                let escape = |text: &str| {
                    if text.contains([sep, '"', '\n', '\r']) {
                        format!("\"{}\"", text.replace('"', "\"\""))
                    } else {
                        text.to_string()
                    }
                };
                let _ = writeln!(
                    text,
                    "{}",
                    columns
                        .iter()
                        .map(|c| escape(&c.header))
                        .collect::<Vec<_>>()
                        .join(joiner)
                );
                for row in cells {
                    let _ = writeln!(
                        text,
                        "{}",
                        row.iter()
                            .map(|c| escape(c))
                            .collect::<Vec<_>>()
                            .join(joiner)
                    );
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
                let _ = writeln!(
                    text,
                    "{}",
                    line(&columns.iter().map(|c| c.header.clone()).collect::<Vec<_>>())
                );
                for row in cells {
                    let _ = writeln!(text, "{}", line(&row));
                }
            }
        }
        self.raw_stdout(&text);
        self.finish()
    }
    #[cfg(feature = "terminal-activity")]
    fn activity_visible(&self) -> bool {
        self.options.tty && !self.options.json && self.options.verbosity != Verbosity::Quiet
    }
}
impl State {
    fn write(&mut self, text: &str, stderr: bool) {
        if text.is_empty() {
            return;
        }
        #[cfg(feature = "terminal-activity")]
        self.clear_activity();
        self.emit(text, stderr, true);
        #[cfg(feature = "terminal-activity")]
        {
            self.activities.line_start = text.ends_with('\n');
            self.render_activity();
        }
    }
    fn emit(&mut self, text: &str, stderr: bool, retain: bool) {
        if retain {
            if stderr {
                self.capture.stderr.push_str(text);
            } else {
                self.capture.stdout.push_str(text);
            }
        }
        let writer = if stderr {
            &mut self.stderr
        } else {
            &mut self.stdout
        };
        if let Some(writer) = writer
            && let Err(error) = writer
                .write_all(text.as_bytes())
                .and_then(|_| writer.flush())
        {
            self.error.get_or_insert_with(|| error.into());
        }
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
    use std::panic::AssertUnwindSafe;
    use std::panic::catch_unwind;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
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
        let (capture, events) = out.report();
        assert_eq!(capture, Capture::default());
        assert_eq!(events.len(), 5);
        assert_eq!(out.captured(), capture);
        assert_eq!(out.events().len(), events.len());
    }
    #[test]
    fn width_handles_unicode_and_escape_sequences() {
        assert_eq!(visible_width("\x1b[31m你好\x1b[0m"), 4);
        assert_eq!(visible_width(&osc8("hi", "https://example.com")), 2);
    }
    #[test]
    fn osc8_wraps_the_label_in_a_hyperlink_sequence() {
        assert_eq!(
            osc8("docs", "https://example.com/a b"),
            "\x1b]8;;https://example.com/a b\x1b\\docs\x1b]8;;\x1b\\"
        );
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
    fn tsv_quotes_tabs_but_not_commas() {
        let out = Output::capture(Default::default());
        out.table(
            &[
                json!({"name":"a,b","size":"1\t2"}),
                json!({"name":null,"size":3}),
            ],
            &[Column::new("name", "Name"), Column::new("size", "Size")],
            TableFormat::Tsv,
        )
        .unwrap();
        assert_eq!(out.captured().stdout, "Name\tSize\na,b\t\"1\t2\"\n\t3\n");
    }
    #[test]
    fn json_table_format_writes_the_rows_as_json() {
        let out = Output::capture(Default::default());
        let rows = [json!({"name":"a"}), json!({"name":"b"})];
        out.table(&rows, &[Column::new("name", "Name")], TableFormat::Json)
            .unwrap();
        assert_eq!(
            out.captured().stdout,
            "[{\"name\":\"a\"},{\"name\":\"b\"}]\n"
        );
    }
    #[test]
    fn pretty_table_is_one_write() {
        struct Counting(Arc<Mutex<Vec<String>>>);
        impl Write for Counting {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(bytes).into_owned());
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let writes = Arc::new(Mutex::new(vec![]));
        let out = Output::with_writers(
            Counting(writes.clone()),
            std::io::sink(),
            Default::default(),
        );
        out.table(
            &[json!({"name":"one","n":1}), json!({"name":"two","n":22})],
            &[Column::new("name", "Name"), Column::new("n", "N")],
            TableFormat::Pretty,
        )
        .unwrap();
        assert_eq!(
            *writes.lock().unwrap(),
            vec!["Name  N\none   1\ntwo   22\n".to_string()]
        );
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
    #[test]
    fn a_poisoned_lock_keeps_the_output_usable() {
        struct PanicsOnce(Arc<AtomicBool>);
        impl Write for PanicsOnce {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.0.swap(false, Ordering::SeqCst) {
                    panic!("writer panics on the first write");
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let out = Output::with_writers(
            PanicsOnce(Arc::new(AtomicBool::new(true))),
            std::io::sink(),
            Default::default(),
        );
        assert!(catch_unwind(AssertUnwindSafe(|| out.log("first"))).is_err());
        assert!(out.state.is_poisoned());
        out.log("second");
        out.set_exit_code(3);
        assert_eq!(out.exit_code(), 3);
        assert_eq!(out.captured().stdout, "first\nsecond\n");
        assert_eq!(out.report().0.stdout, "first\nsecond\n");
        out.finish().unwrap();
    }
}
