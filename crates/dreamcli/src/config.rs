//! Config discovery follows project ancestors, then user and system locations.
use crate::CliError;
use crate::Result;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

pub trait ConfigReader: Send + Sync {
    fn read(&self, path: &Path) -> Result<Option<String>>;
}
type Loader = Arc<dyn Fn(&str) -> Result<Value> + Send + Sync>;
#[derive(Clone)]
pub struct ConfigSettings {
    pub name: String,
    pub paths: Option<Vec<PathBuf>>,
    pub loaders: BTreeMap<String, Loader>,
}
impl ConfigSettings {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            paths: None,
            loaders: BTreeMap::new(),
        }
    }
    pub fn paths(mut self, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        self.paths = Some(paths.into_iter().collect());
        self
    }
    pub fn loader(
        mut self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
        parse: impl Fn(&str) -> Result<Value> + Send + Sync + 'static,
    ) -> Self {
        let parse: Loader = Arc::new(parse);
        for ext in extensions {
            self.loaders
                .insert(ext.into().to_lowercase(), parse.clone());
        }
        self
    }
    pub fn candidates(
        &self,
        cwd: &Path,
        user_dir: Option<&Path>,
        system_dirs: &[PathBuf],
    ) -> Vec<PathBuf> {
        if let Some(paths) = &self.paths {
            return paths.clone();
        }
        let extensions = std::iter::once("json")
            .chain(
                self.loaders
                    .keys()
                    .filter(|s| *s != "json")
                    .map(String::as_str),
            )
            .collect::<Vec<_>>();
        let mut paths = vec![];
        for ancestor in cwd.ancestors() {
            for ext in &extensions {
                paths.push(ancestor.join(format!(".{}rc.{ext}", self.name)));
                paths.push(ancestor.join(format!("{}.{ext}", self.name)));
            }
        }
        for dir in user_dir
            .into_iter()
            .chain(system_dirs.iter().map(PathBuf::as_path))
        {
            for ext in &extensions {
                paths.push(dir.join(&self.name).join(format!("config.{ext}")));
                paths.push(dir.join(format!("{}.{ext}", self.name)));
            }
        }
        paths
    }
}
#[derive(Debug, Clone)]
pub struct FoundConfig {
    pub path: PathBuf,
    pub value: Value,
}
pub fn discover(
    settings: &ConfigSettings,
    reader: &dyn ConfigReader,
    candidates: &[PathBuf],
    explicit: Option<&Path>,
) -> Result<Option<FoundConfig>> {
    let paths = explicit
        .map(|p| vec![p.to_path_buf()])
        .unwrap_or_else(|| candidates.to_vec());
    for path in paths {
        let Some(text) = reader.read(&path)? else {
            if explicit.is_some() {
                return Err(CliError::new(
                    "CONFIG_NOT_FOUND",
                    format!("Config file not found: {}", path.display()),
                ));
            }
            continue;
        };
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("json")
            .to_lowercase();
        let value = if let Some(loader) = settings.loaders.get(&ext) {
            loader(&text)
        } else if ext == "json" {
            serde_json::from_str(&text).map_err(Into::into)
        } else {
            return Err(CliError::new(
                "UNSUPPORTED_CONFIG_FORMAT",
                format!("No config loader for {ext}"),
            ));
        };
        let value = value.map_err(|_| {
            CliError::new(
                "CONFIG_PARSE_ERROR",
                format!("Cannot parse config file {}", path.display()),
            )
        })?;
        if !value.is_object() {
            return Err(CliError::new(
                "CONFIG_PARSE_ERROR",
                "Config must be an object",
            ));
        }
        return Ok(Some(FoundConfig { path, value }));
    }
    Ok(None)
}
/// Native Rust package metadata comes from Cargo, supplied explicitly by the application.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
}
#[macro_export]
macro_rules! cargo_manifest {
    () => {
        $crate::config::Manifest {
            name: Some(env!("CARGO_PKG_NAME").into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            description: Some(env!("CARGO_PKG_DESCRIPTION").into()),
        }
    };
}
