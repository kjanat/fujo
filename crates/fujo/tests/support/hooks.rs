use fujo::CliError;
use fujo::Context;
use fujo::Plugin;
use fujo::Result;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub struct Hooks {
    pub events: Arc<Mutex<Vec<String>>>,
    pub fail: &'static str,
}
impl Hooks {
    fn record(&self, stage: &str) -> Result<()> {
        self.events.lock().unwrap().push(stage.into());
        if self.fail == stage {
            Err(CliError::new("PLUGIN_FAILURE", stage))
        } else {
            Ok(())
        }
    }
}
impl Plugin for Hooks {
    fn before_parse(&self, _: &mut Vec<String>) -> Result<()> {
        self.record("parse")
    }
    fn before_execute(&self, _: &mut Context) -> Result<()> {
        self.record("before")
    }
    fn after_execute(&self, _: &Context) -> Result<()> {
        self.record("after")
    }
    fn on_error(&self, _: &CliError) {
        self.events.lock().unwrap().push("error".into());
    }
}
