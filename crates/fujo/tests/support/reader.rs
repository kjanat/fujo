use fujo::Result;
use fujo::config::ConfigReader;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default)]
pub struct MapReader(pub BTreeMap<PathBuf, String>);
impl ConfigReader for MapReader {
    fn read(&self, path: &Path) -> Result<Option<String>> {
        Ok(self.0.get(path).cloned())
    }
}
