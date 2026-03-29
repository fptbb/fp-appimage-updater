use fp_appimage_updater::commands::helpers::github_proxy_prefixes;
use fp_appimage_updater::config::{
    AppConfig, CheckMethod, GithubProxyPrefixes, GlobalConfig, StrategyConfig,
};
use std::path::PathBuf;

fn app_config(prefix: Option<GithubProxyPrefixes>) -> AppConfig {
    AppConfig {
        config_dir: PathBuf::new(),
        name: "demo".to_string(),
        zsync: None,
        integration: None,
        create_symlink: None,
        segmented_downloads: None,
        respect_rate_limits: None,
        github_proxy: None,
        github_proxy_prefix: prefix,
        storage_dir: None,
        strategy: StrategyConfig::Direct {
            url: "https://example.com/file.AppImage".to_string(),
            check_method: CheckMethod::Etag,
        },
    }
}

#[test]
fn github_proxy_prefix_all_expands_to_all_supported_prefixes() {
    let app = app_config(Some(GithubProxyPrefixes::Single("all".to_string())));
    let global = GlobalConfig::default();

    let prefixes = github_proxy_prefixes(&app, &global);
    assert_eq!(
        prefixes,
        vec![
            "https://gh-proxy.com/".to_string(),
            "https://corsproxy.io/?".to_string(),
            "https://api.allorigins.win/raw?url=".to_string(),
        ]
    );
}

#[test]
fn github_proxy_prefix_all_in_array_expands_to_all_supported_prefixes() {
    let app = app_config(Some(GithubProxyPrefixes::Multiple(vec!["all".to_string()])));
    let global = GlobalConfig::default();

    let prefixes = github_proxy_prefixes(&app, &global);
    assert_eq!(
        prefixes,
        vec![
            "https://gh-proxy.com/".to_string(),
            "https://corsproxy.io/?".to_string(),
            "https://api.allorigins.win/raw?url=".to_string(),
        ]
    );
}
