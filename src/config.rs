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
    #[serde(skip)]
    pub github_token: Option<String>,
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
            github_token: None,
        }
    }
}

/// Configuration for sensitive credentials, typically stored in `secrets.yml`.
#[derive(Debug, Deserialize, Default)]
pub struct SecretsConfig {
    pub github_token: Option<String>,
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

/// Per-app zsync manifest configuration.
///
/// `true` means "use the downloader URL with `.zsync` appended".
/// A string means "use this exact zsync manifest URL".
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum ZsyncConfig {
    /// Enable zsync by deriving the manifest URL from the resolved download URL.
    Enabled(bool),
    /// Use an explicit zsync manifest URL.
    Url(String),
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "strategy", rename_all = "lowercase")]
pub enum StrategyConfig {
    Forge {
        repository: String,
        #[serde(default = "default_forge_asset_match")]
        asset_match: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        asset_match_regex: Option<String>,
    },
    Direct {
        url: String,
        check_method: CheckMethod,
    },
    Script {
        script_path: String,
    },
}

fn default_forge_asset_match() -> String {
    "*".to_string()
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CheckMethod {
    Etag,
    LastModified,
}
