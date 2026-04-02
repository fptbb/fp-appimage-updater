use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum GithubProxyPrefixes {
    Single(String),
    Multiple(Vec<String>),
}

impl GithubProxyPrefixes {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::Single(prefix) => vec![prefix],
            Self::Multiple(prefixes) => prefixes,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GlobalConfig {
    pub storage_dir: String,
    pub symlink_dir: String,
    pub naming_format: String,
    pub manage_desktop_files: bool,
    pub create_symlinks: bool,
    #[serde(default)]
    pub segmented_downloads: bool,
    #[serde(default = "default_respect_rate_limits")]
    pub respect_rate_limits: bool,
    #[serde(default = "default_github_proxy")]
    pub github_proxy: bool,
    #[serde(default = "default_github_proxy_prefix")]
    pub github_proxy_prefix: GithubProxyPrefixes,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub github_release_api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub github_release_web_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gitlab_release_api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gitlab_release_web_url: Option<String>,
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
            respect_rate_limits: true,
            github_proxy: true,
            github_proxy_prefix: GithubProxyPrefixes::Single("all".to_string()),
            github_release_api_url: None,
            github_release_web_url: None,
            gitlab_release_api_url: None,
            gitlab_release_web_url: None,
        }
    }
}

fn default_respect_rate_limits() -> bool {
    true
}

fn default_github_proxy() -> bool {
    true
}

fn default_github_proxy_prefix() -> GithubProxyPrefixes {
    GithubProxyPrefixes::Single("all".to_string())
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
    pub respect_rate_limits: Option<bool>,
    pub github_proxy: Option<bool>,
    pub github_proxy_prefix: Option<GithubProxyPrefixes>,
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
