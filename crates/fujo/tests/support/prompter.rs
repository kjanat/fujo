use fujo::RunOptions;
use fujo::prompt::Answer;
use fujo::prompt::TestPrompter;
use std::sync::Arc;

pub fn prompter(answers: impl IntoIterator<Item = Answer>) -> Arc<TestPrompter> {
    Arc::new(TestPrompter::new(answers))
}
pub fn prompted(answers: impl IntoIterator<Item = Answer>) -> RunOptions {
    let mut options = RunOptions::default();
    options.resolve.prompter = Some(prompter(answers));
    options
}
