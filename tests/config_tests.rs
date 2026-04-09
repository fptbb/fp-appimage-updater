use fp_appimage_updater::config::{AppConfig, GithubProxyPrefixes, GlobalConfig, StrategyConfig};

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

#[test]
fn forge_asset_match_defaults_when_regex_is_used() {
    let app: AppConfig = serde_yaml::from_str(
        r#"
name: obsidian
strategy:
  strategy: forge
  repository: "https://github.com/obsidianmd/obsidian-releases"
  asset_match_regex: "^Obsidian-[0-9.]+\\.AppImage$"
"#,
    )
    .expect("expected regex-only forge recipe to parse");

    match app.strategy {
        StrategyConfig::Forge {
            asset_match,
            asset_match_regex,
            ..
        } => {
            assert_eq!(asset_match, "*");
            assert_eq!(
                asset_match_regex.as_deref(),
                Some("^Obsidian-[0-9.]+\\.AppImage$")
            );
        }
        _ => panic!("expected forge strategy"),
    }
}

#[test]
fn global_forge_url_templates_are_optional() {
    let config: GlobalConfig = serde_yaml::from_str(
        r#"
storage_dir: ~/.local/bin/AppImages
symlink_dir: ~/.local/bin
naming_format: "{name}.AppImage"
manage_desktop_files: true
create_symlinks: false
segmented_downloads: true
respect_rate_limits: true
github_proxy: false
github_release_api_url: "https://api.example.com/repos/{account}/{repository}/releases/latest"
github_release_web_url: "https://example.com/{account}/{repository}"
gitlab_release_api_url: "https://gitlab.example.com/api/v4/projects/{project_path}/releases/permalink/latest"
gitlab_release_web_url: "https://gitlab.example.com/{repo_path}"
"#,
    )
    .expect("expected forge templates to parse");

    assert_eq!(
        config.github_release_api_url.as_deref(),
        Some("https://api.example.com/repos/{account}/{repository}/releases/latest")
    );
    assert_eq!(
        config.github_release_web_url.as_deref(),
        Some("https://example.com/{account}/{repository}")
    );
    assert_eq!(
        config.gitlab_release_api_url.as_deref(),
        Some("https://gitlab.example.com/api/v4/projects/{project_path}/releases/permalink/latest")
    );
    assert_eq!(
        config.gitlab_release_web_url.as_deref(),
        Some("https://gitlab.example.com/{repo_path}")
    );
}
