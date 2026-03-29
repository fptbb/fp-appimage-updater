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
    #[serde(default)]
    pub github_proxy: bool,
    #[serde(default = "default_github_proxy_prefix")]
    pub github_proxy_prefix: GithubProxyPrefixes,
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
            github_proxy: false,
            github_proxy_prefix: GithubProxyPrefixes::Single("https://gh-proxy.com/".to_string()),
        }
    }
}

fn default_respect_rate_limits() -> bool {
    true
}

fn default_github_proxy_prefix() -> GithubProxyPrefixes {
    GithubProxyPrefixes::Single("https://gh-proxy.com/".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_proxy_prefix_accepts_string_or_array() {
        let single: GlobalConfig = serde_yaml::from_str(
            r#"
storage_dir: ~/.local/bin/AppImages
symlink_dir: ~/.local/bin
naming_format: "{name}.AppImage"
manage_desktop_files: true
create_symlinks: false
segmented_downloads: true
respect_rate_limits: true
github_proxy: true
github_proxy_prefix: "https://gh-proxy.com/"
"#,
        )
        .expect("expected string prefix to parse");
        assert!(matches!(
            single.github_proxy_prefix,
            GithubProxyPrefixes::Single(_)
        ));

        let multiple: GlobalConfig = serde_yaml::from_str(
            r#"
storage_dir: ~/.local/bin/AppImages
symlink_dir: ~/.local/bin
naming_format: "{name}.AppImage"
manage_desktop_files: true
create_symlinks: false
segmented_downloads: true
respect_rate_limits: true
github_proxy: true
github_proxy_prefix:
  - "https://corsproxy.io/?"
  - "https://api.allorigins.win/raw?url="
"#,
        )
        .expect("expected prefix array to parse");
        assert!(matches!(
            multiple.github_proxy_prefix,
            GithubProxyPrefixes::Multiple(_)
        ));
    }

    #[test]
    fn app_proxy_prefix_accepts_string_or_array() {
        let single: AppConfig = serde_yaml::from_str(
            r#"
name: demo
github_proxy_prefix: "https://gh-proxy.com/"
strategy:
  strategy: direct
  url: "https://example.com/file.AppImage"
  check_method: etag
"#,
        )
        .expect("expected string prefix to parse");
        assert!(matches!(
            single.github_proxy_prefix,
            Some(GithubProxyPrefixes::Single(_))
        ));

        let multiple: AppConfig = serde_yaml::from_str(
            r#"
name: demo
github_proxy_prefix:
  - "https://corsproxy.io/?"
  - "https://api.allorigins.win/raw?url="
strategy:
  strategy: direct
  url: "https://example.com/file.AppImage"
  check_method: etag
"#,
        )
        .expect("expected prefix array to parse");
        assert!(matches!(
            multiple.github_proxy_prefix,
            Some(GithubProxyPrefixes::Multiple(_))
        ));
    }
}
