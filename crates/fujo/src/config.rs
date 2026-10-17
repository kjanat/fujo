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
type Loader = Arc<dyn Fn(&Path, &str) -> Result<Value> + Send + Sync>;
#[derive(Debug, Clone, Copy)]
pub enum Platform {
    Linux,
    MacOs,
    Windows,
    Other,
}
#[derive(Debug, Clone, Default)]
pub struct ConfigDirectories {
    pub user: Vec<PathBuf>,
    pub system: Vec<PathBuf>,
}
pub fn config_directories(
    platform: Platform,
    mut env: impl FnMut(&str) -> Option<String>,
) -> ConfigDirectories {
    let mut dirs = ConfigDirectories::default();
    match platform {
        Platform::Windows => {
            if let Some(dir) = env("APPDATA")
                .map(PathBuf::from)
                .or_else(|| env("USERPROFILE").map(|h| PathBuf::from(h).join("AppData/Roaming")))
            {
                dirs.user.push(dir);
            }
        }
        _ => {
            let home = env("HOME").map(PathBuf::from);
            if let Some(dir) = env("XDG_CONFIG_HOME")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|h| h.join(".config")))
            {
                dirs.user.push(dir);
            }
            if matches!(platform, Platform::MacOs)
                && let Some(home) = home
            {
                dirs.user.push(home.join("Library/Application Support"));
            }
            if matches!(platform, Platform::Linux | Platform::MacOs) {
                dirs.system.push(PathBuf::from("/etc"));
            }
        }
    }
    dirs
}
#[derive(Clone)]
pub struct ConfigSettings {
    pub name: String,
    pub paths: Option<Vec<PathBuf>>,
    pub loaders: BTreeMap<String, Loader>,
    pub base_dir: Option<PathBuf>,
}
impl ConfigSettings {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            paths: None,
            loaders: BTreeMap::new(),
            base_dir: None,
        }
    }
    pub fn paths(mut self, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        self.paths = Some(paths.into_iter().collect());
        self
    }
    pub fn loader(
        self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
        parse: impl Fn(&str) -> Result<Value> + Send + Sync + 'static,
    ) -> Self {
        self.path_loader(extensions, move |_, text| parse(text))
    }
    pub fn path_loader(
        mut self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
        parse: impl Fn(&Path, &str) -> Result<Value> + Send + Sync + 'static,
    ) -> Self {
        let parse: Loader = Arc::new(parse);
        for ext in extensions {
            self.loaders
                .insert(ext.into().to_lowercase(), parse.clone());
        }
        self
    }
    pub fn base_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.base_dir = Some(dir.into());
        self
    }
    pub fn candidates(
        &self,
        cwd: &Path,
        user_dir: Option<&Path>,
        system_dirs: &[PathBuf],
    ) -> Vec<PathBuf> {
        self.candidates_in(
            cwd,
            &user_dir
                .into_iter()
                .map(Path::to_path_buf)
                .collect::<Vec<_>>(),
            system_dirs,
        )
    }
    pub fn candidates_in(
        &self,
        cwd: &Path,
        user_dirs: &[PathBuf],
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
        for ancestor in self.base_dir.as_deref().unwrap_or(cwd).ancestors() {
            for ext in &extensions {
                paths.push(ancestor.join(format!(".{}.{ext}", self.name)));
                paths.push(ancestor.join(format!("{}.config.{ext}", self.name)));
                paths.push(
                    ancestor
                        .join(".config")
                        .join(format!("{}.{ext}", self.name)),
                );
            }
        }
        for dir in user_dirs
            .iter()
            .map(PathBuf::as_path)
            .chain(system_dirs.iter().map(PathBuf::as_path))
        {
            for ext in &extensions {
                paths.push(dir.join(&self.name).join(format!("config.{ext}")));
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
            loader(&path, &text)
        } else if ext == "json" {
            serde_json::from_str(&text).map_err(Into::into)
        } else {
            return Err(CliError::new(
                "UNSUPPORTED_CONFIG_FORMAT",
                format!("No config loader for {ext}"),
            ));
        };
        let value = value.map_err(|e| {
            CliError::new(
                "CONFIG_PARSE_ERROR",
                format!("Cannot parse config file {}: {}", path.display(), e.message),
            )
        })?;
        if !value.is_object() {
            return Err(CliError::new(
                "CONFIG_PARSE_ERROR",
                format!("Config file {} must be an object", path.display()),
            ));
        }
        return Ok(Some(FoundConfig { path, value }));
    }
    Ok(None)
}
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
