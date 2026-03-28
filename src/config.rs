use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GlobalConfig {
    pub storage_dir: String,
    pub symlink_dir: String,
    pub naming_format: String,
    pub manage_desktop_files: bool,
    pub create_symlinks: bool,
    #[serde(default)]
    pub segmented_downloads: bool,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            storage_dir: "~/.local/bin/AppImages".to_string(),
            symlink_dir: "~/.local/bin".to_string(),
            naming_format: "{name}.AppImage".to_string(),
            manage_desktop_files: true,
            create_symlinks: false,
            segmented_downloads: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(skip)]
    pub config_dir: PathBuf,

    pub name: String,
    pub zsync: Option<ZsyncConfig>,
    pub integration: Option<bool>,
    pub create_symlink: Option<bool>,
    pub segmented_downloads: Option<bool>,
    pub storage_dir: Option<String>,
    pub strategy: StrategyConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum ZsyncConfig {
    Enabled(bool),
    Url(String),
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "strategy", rename_all = "lowercase")]
pub enum StrategyConfig {
    Forge {
        repository: String,
        asset_match: String,
    },
    Direct {
        url: String,
        check_method: CheckMethod,
    },
    Script {
        script_path: String,
    },
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CheckMethod {
    Etag,
    LastModified,
}
