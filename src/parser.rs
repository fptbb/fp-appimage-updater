use anyhow::{Context, Result};
use glob::glob;
use serde_yaml::Value;
use std::fs;
use std::path::PathBuf;

use crate::config::{AppConfig, GlobalConfig};

const APP_NAME: &str = "fp-appimage-updater";

pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

pub struct AppConfigLoadError {
    pub path: PathBuf,
    pub app_name: Option<String>,
    pub message: String,
}

pub struct AppConfigLoadResult {
    pub apps: Vec<AppConfig>,
    pub errors: Vec<AppConfigLoadError>,
}

impl ConfigPaths {
    /// Resolve paths using the OS-standard project directories.
    pub fn new() -> Result<Self> {
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").expect("HOME not set in environment");
                PathBuf::from(home).join(".config")
            })
            .join(APP_NAME);

        let state_dir = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").expect("HOME not set in environment");
                PathBuf::from(home).join(".local/state")
            })
            .join(APP_NAME);

        Ok(Self {
            config_dir,
            state_dir,
        })
    }

    /// Use an explicit config directory override (from `--config`).
    /// The state directory falls back to the OS default.
    pub fn with_config_dir(config_dir: std::path::PathBuf) -> Result<Self> {
        let state_dir = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").expect("HOME not set in environment");
                PathBuf::from(home).join(".local/state")
            })
            .join(APP_NAME);

        Ok(Self {
            config_dir,
            state_dir,
        })
    }

    pub fn global_config_path(&self) -> PathBuf {
        self.config_dir.join("config.yml")
    }

    pub fn apps_dir(&self) -> PathBuf {
        self.config_dir.join("apps")
    }

    pub fn cache_path(&self) -> PathBuf {
        self.state_dir.join("cache.json")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.state_dir.join("process.lock")
    }
}

pub fn load_global_config(paths: &ConfigPaths) -> Result<GlobalConfig> {
    let config_path = paths.global_config_path();
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read global config: {:?}", config_path))?;
        let config: GlobalConfig = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse global config: {:?}", config_path))?;
        Ok(config)
    } else {
        let default_config = GlobalConfig::default();
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent.join("apps")).with_context(|| {
                format!("Failed to create config directories: {}", parent.display())
            })?;
        }
        let content = serde_yaml::to_string(&default_config)
            .with_context(|| "Failed to serialize default global config")?;
        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write default global config: {:?}", config_path))?;

        Ok(default_config)
    }
}

pub fn load_app_configs(paths: &ConfigPaths) -> Result<AppConfigLoadResult> {
    let mut apps = Vec::new();
    let mut errors = Vec::new();
    let apps_dir = paths.apps_dir();

    if !apps_dir.exists() {
        return Ok(AppConfigLoadResult { apps, errors });
    }

    let pattern = format!("{}/**/*.yml", apps_dir.display());
    for entry in glob(&pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => match parse_app_config(&path) {
                Ok(app) => apps.push(app),
                Err(e) => errors.push(AppConfigLoadError {
                    app_name: infer_name_from_yaml_path(&path),
                    path,
                    message: e.to_string(),
                }),
            },
            Err(e) => errors.push(AppConfigLoadError {
                app_name: None,
                path: paths.apps_dir(),
                message: format!("Glob error: {e}"),
            }),
        }
    }

    Ok(AppConfigLoadResult { apps, errors })
}

fn parse_app_config(path: &std::path::Path) -> Result<AppConfig> {
    let content = fs::read_to_string(path)?;
    let mut app: AppConfig = serde_yaml::from_str(&content)?;
    if let Some(parent) = path.parent() {
        app.config_dir = parent.to_path_buf();
    }
    Ok(app)
}

fn infer_name_from_yaml_path(path: &std::path::Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let value: Value = serde_yaml::from_str(&content).ok()?;
    let map = value.as_mapping()?;
    map.get(&Value::String("name".to_string()))?
        .as_str()
        .map(ToOwned::to_owned)
}
