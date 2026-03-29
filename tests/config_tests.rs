use fp_appimage_updater::config::{AppConfig, GithubProxyPrefixes, GlobalConfig};

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
