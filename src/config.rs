use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use anyhow::{Result, bail};

/// Proxy URL prefix or prefixes to route GitHub API requests.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum GithubProxyPrefixes {
    /// A single proxy URL prefix string.
    Single(String),
    /// A list of proxy URL prefix strings tried in order.
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

/// Global configuration for fp-appimage-updater.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct GlobalConfig {
    /// Directory where downloaded AppImage binaries are saved.
    pub storage_dir: String,
    /// Directory where symlinks to active AppImages are created.
    pub symlink_dir: String,
    /// Naming format for AppImage files. Supports `{name}` placeholder.
    pub naming_format: String,
    /// Extract and integrate desktop shortcuts (.desktop) and application icons.
    pub manage_desktop_files: bool,
    /// Automatically create binary symlinks in the symlink directory.
    pub create_symlinks: bool,
    /// Use segmented multi-connection HTTP range downloads for faster speed.
    #[serde(default)]
    pub segmented_downloads: bool,
    /// Show all configured apps in check/update output, including up-to-date ones.
    #[serde(default)]
    pub show_all: bool,
    /// Skip checking or updating apps when a rate limit window is active.
    #[serde(default = "default_respect_rate_limits")]
    pub respect_rate_limits: bool,
    /// Fall back to a proxy if direct GitHub API requests are rate limited.
    #[serde(default = "default_github_proxy")]
    pub github_proxy: bool,
    /// Proxy URL prefix or list of prefixes to try when routing GitHub API queries.
    #[serde(default = "default_github_proxy_prefix")]
    pub github_proxy_prefix: GithubProxyPrefixes,
    /// Custom base URL for the GitHub API.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub github_release_api_url: Option<String>,
    /// Custom base URL for GitHub web pages.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub github_release_web_url: Option<String>,
    /// Custom base URL for the GitLab API.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gitlab_release_api_url: Option<String>,
    /// Custom base URL for GitLab web pages.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gitlab_release_web_url: Option<String>,
    /// Automatically check and update the fp-appimage-updater CLI binary itself.
    #[serde(default)]
    pub auto_self_update: bool,
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
            show_all: false,
            respect_rate_limits: true,
            github_proxy: true,
            github_proxy_prefix: GithubProxyPrefixes::Single("all".to_string()),
            github_release_api_url: None,
            github_release_web_url: None,
            gitlab_release_api_url: None,
            gitlab_release_web_url: None,
            auto_self_update: false,
            github_token: None,
        }
    }
}

pub fn ensure_safe_path_component(value: &str, label: &str) -> Result<()> {
    let mut components = std::path::Path::new(value).components();
    match components.next() {
        Some(std::path::Component::Normal(_))
            if components.next().is_none() && !value.contains('\0') =>
        {
            Ok(())
        }
        _ => bail!(
            "Invalid {} '{}': must be a single path component without separators",
            label,
            value
        ),
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

/// Individual application recipe configuration.
#[derive(Debug, Deserialize, Clone)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct AppConfig {
    #[serde(skip)]
    pub config_dir: PathBuf,

    /// Unique name of the application.
    pub name: String,
    /// Skip checking or updating this application.
    pub ignore: Option<bool>,
    /// Zsync delta updates configuration.
    pub zsync: Option<ZsyncConfig>,
    /// Per-app override: Extract and integrate desktop entries and icons.
    pub integration: Option<bool>,
    /// Per-app override: Create binary symlink in symlink directory.
    pub create_symlink: Option<bool>,
    /// Per-app override: Use segmented downloads.
    pub segmented_downloads: Option<bool>,
    /// Per-app override: Respect API rate limits.
    pub respect_rate_limits: Option<bool>,
    /// Per-app override: Fall back to a proxy if GitHub API rate limits are hit.
    pub github_proxy: Option<bool>,
    /// Per-app override: Proxy URL prefix or list of prefixes to try.
    pub github_proxy_prefix: Option<GithubProxyPrefixes>,
    /// Per-app override: Custom storage directory.
    pub storage_dir: Option<String>,
    /// Per-app override: Custom AppImage binary filename format.
    pub naming_format: Option<String>,
    /// Wildcard pattern to locate a specific AppImage inside a downloaded .zip.
    pub inner_asset_match: Option<String>,
    /// Update resolution strategy.
    pub strategy: StrategyConfig,
}

/// Per-app zsync manifest configuration.
///
/// `true` means "use the downloader URL with `.zsync` appended".
/// A string means "use this exact zsync manifest URL".
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum ZsyncConfig {
    /// Enable zsync by deriving the manifest URL from the resolved download URL.
    Enabled(bool),
    /// Use an explicit zsync manifest URL.
    Url(String),
}

/// Strategy used to locate and download updates.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "strategy", rename_all = "lowercase")]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum StrategyConfig {
    /// Download from a forge release repository (e.g., GitHub, GitLab, Gitea, Forgejo).
    Forge {
        /// URL to the git repository (e.g., https://github.com/user/repo).
        repository: String,
        /// Glob pattern to match the release asset filename (defaults to `*`).
        #[serde(default = "default_forge_asset_match")]
        asset_match: String,
        /// Regular expression pattern to match the release asset filename.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        asset_match_regex: Option<String>,
    },
    /// Download from a direct URL.
    Direct {
        /// Static URL to download the AppImage binary directly.
        url: String,
        /// HTTP validation method used to detect updates (etag or last-modified).
        check_method: CheckMethod,
    },
    /// Download using an external custom resolution script.
    Script {
        /// Path to the local executable script (relative to the recipe file).
        script_path: String,
    },
}

fn default_forge_asset_match() -> String {
    "*".to_string()
}

/// HTTP method used to detect changes in direct downloads.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum CheckMethod {
    /// Use the HTTP ETag header.
    Etag,
    /// Use the HTTP Last-Modified header.
    LastModified,
}
