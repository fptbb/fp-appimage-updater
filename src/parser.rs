use anyhow::{Context, Result};
use directories::ProjectDirs;
use glob::glob;
use std::fs;
use std::path::PathBuf;

use crate::config::{AppConfig, GlobalConfig};

const APP_NAME: &str = "fp-appimage-updater";

pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl ConfigPaths {
    /// Resolve paths using the OS-standard project directories.
    pub fn new() -> Result<Self> {
        let proj_dirs = ProjectDirs::from("", "", APP_NAME)
            .context("Could not determine project directories")?;

        let config_dir = proj_dirs.config_dir().to_path_buf();
        let state_dir = proj_dirs.state_dir()
            .unwrap_or_else(|| proj_dirs.data_local_dir())
            .to_path_buf();

        Ok(Self { config_dir, state_dir })
    }

    /// Use an explicit config directory override (from `--config`).
    /// The state directory falls back to the OS default.
    pub fn with_config_dir(config_dir: std::path::PathBuf) -> Result<Self> {
        let proj_dirs = ProjectDirs::from("", "", APP_NAME)
            .context("Could not determine project directories")?;

        let state_dir = proj_dirs.state_dir()
            .unwrap_or_else(|| proj_dirs.data_local_dir())
            .to_path_buf();

        Ok(Self { config_dir, state_dir })
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
            fs::create_dir_all(parent.join("apps"))
                .with_context(|| format!("Failed to create config directories: {}", parent.display()))?;
        }
        let content = serde_yaml::to_string(&default_config)
            .with_context(|| "Failed to serialize default global config")?;
        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write default global config: {:?}", config_path))?;
        
        Ok(default_config)
    }
}

pub fn load_app_configs(paths: &ConfigPaths) -> Result<Vec<AppConfig>> {
    let mut apps = Vec::new();
    let apps_dir = paths.apps_dir();
    
    if !apps_dir.exists() {
        return Ok(apps);
    }

    let pattern = format!("{}/**/*.yml", apps_dir.display());
    for entry in glob(&pattern).expect("Failed to read glob pattern") {
        match entry {
            Ok(path) => {
                match parse_app_config(&path) {
                    Ok(app) => apps.push(app),
                    Err(e) => eprintln!("Warning: Failed to parse app config at {:?}: {}", path, e),
                }
            }
            Err(e) => eprintln!("Warning: Glob error: {:?}", e),
        }
    }

    Ok(apps)
}

fn parse_app_config(path: &std::path::Path) -> Result<AppConfig> {
    let content = fs::read_to_string(path)?;
    let mut app: AppConfig = serde_yaml::from_str(&content)?;
    if let Some(parent) = path.parent() {
        app.config_dir = parent.to_path_buf();
    }
    Ok(app)
}
