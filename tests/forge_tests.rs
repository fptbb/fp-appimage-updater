use fp_appimage_updater::commands::helpers::cache_app_metadata;
use fp_appimage_updater::config::GlobalConfig;
use fp_appimage_updater::resolvers::forge::{
    AssetMatcher, ForgeHost, find_release_asset_in_html, find_release_asset_in_html_with_base,
    find_release_asset_in_html_with_base_and_matcher, forge_platform_from_swagger_title,
    github_release_web_url_with_config, release_api_url, release_api_url_with_config,
    release_assets, sanitize_github_proxy_url, validate_github_proxy_metadata,
};
use fp_appimage_updater::state::AppState;
use ureq::Agent;

#[test]
fn invalid_asset_pattern_is_reported_before_network() {
    let client = Agent::new_with_defaults();
    let err = fp_appimage_updater::resolvers::forge::resolve(
        &client,
        "https://github.com/fptbb/fp-appimage-updater",
        "[",
        None,
        None,
        false,
        &[String::from("https://gh-proxy.com/")],
        &GlobalConfig::default(),
    )
    .expect_err("expected invalid asset pattern to fail");

    let message = format!("{:#}", err);
    assert!(message.contains("Invalid asset_match pattern"));
    assert!(message.contains("https://github.com/fptbb/fp-appimage-updater"));
}

#[test]
fn invalid_asset_regex_is_reported_before_network() {
    let client = Agent::new_with_defaults();
    let err = fp_appimage_updater::resolvers::forge::resolve(
        &client,
        "https://github.com/fptbb/fp-appimage-updater",
        "*",
        Some("["),
        None,
        false,
        &[String::from("https://gh-proxy.com/")],
        &GlobalConfig::default(),
    )
    .expect_err("expected invalid asset regex to fail");

    let message = format!("{:#}", err);
    assert!(message.contains("Invalid asset_match_regex pattern"));
    assert!(message.contains("https://github.com/fptbb/fp-appimage-updater"));
}

#[test]
fn gitlab_release_url_uses_permalink_latest_api() {
    let url = release_api_url(
        ForgeHost::GitLab,
        "https://gitlab.com/fpsys/fp-appimage-updater",
    )
    .expect("expected gitlab api url");

    assert_eq!(
        url,
        "https://gitlab.com/api/v4/projects/fpsys%2Ffp-appimage-updater/releases/permalink/latest"
    );
}

#[test]
fn gitea_release_url_uses_api_v1_latest() {
    let url = release_api_url(
        ForgeHost::Gitea,
        "https://git.linux.toys/fptbb/fp-appimage-updater",
    )
    .expect("expected gitea api url");

    assert_eq!(
        url,
        "https://git.linux.toys/api/v1/repos/fptbb/fp-appimage-updater/releases/latest"
    );
}

#[test]
fn forgejo_release_url_uses_api_v1_latest() {
    let url = release_api_url(
        ForgeHost::Forgejo,
        "https://git.linux.toys/fptbb/fp-appimage-updater",
    )
    .expect("expected forgejo api url");

    assert_eq!(
        url,
        "https://git.linux.toys/api/v1/repos/fptbb/fp-appimage-updater/releases/latest"
    );
}

#[test]
fn forge_swagger_title_maps_to_platforms() {
    assert_eq!(
        forge_platform_from_swagger_title("Gitea API"),
        Some(ForgeHost::Gitea)
    );
    assert_eq!(
        forge_platform_from_swagger_title("Forgejo API"),
        Some(ForgeHost::Forgejo)
    );
    assert_eq!(forge_platform_from_swagger_title("Unknown"), None);
}

#[test]
fn forge_url_templates_render_account_repository_and_repo_path() {
    let config = GlobalConfig {
        github_release_api_url: Some(
            "https://api.example.com/repos/{account}/{repository}/releases/latest".to_string(),
        ),
        github_release_web_url: Some("https://example.com/{account}/{repository}".to_string()),
        gitlab_release_api_url: Some(
            "https://gitlab.example.com/api/v4/projects/{project_path}/releases/permalink/latest"
                .to_string(),
        ),
        gitlab_release_web_url: Some("https://gitlab.example.com/{repo_path}".to_string()),
        ..GlobalConfig::default()
    };

    assert_eq!(
        release_api_url_with_config(ForgeHost::GitHub, "https://github.com/owner/repo", &config,)
            .expect("expected github api url"),
        "https://api.example.com/repos/owner/repo/releases/latest"
    );
    assert_eq!(
        github_release_web_url_with_config("https://github.com/owner/repo", &config)
            .expect("expected github web url"),
        "https://example.com/owner/repo"
    );
    assert_eq!(
        release_api_url_with_config(
            ForgeHost::GitLab,
            "https://gitlab.com/group/subgroup/repo",
            &config,
        )
        .expect("expected gitlab api url"),
        "https://gitlab.example.com/api/v4/projects/group%2Fsubgroup%2Frepo/releases/permalink/latest"
    );
}

#[test]
fn gitlab_release_assets_use_direct_asset_url() {
    let resp = serde_json::json!({
        "tag_name": "v1.2.2",
        "assets": {
            "links": [
                {
                    "name": "fp-appimage-updater.x64",
                    "url": "https://gitlab.com/fpsys/fp-appimage-updater/-/jobs/artifacts/main/raw/build/fp-appimage-updater.x64?job=build-and-compress",
                    "direct_asset_url": "https://gitlab.com/fpsys/fp-appimage-updater/-/releases/v1.2.2/downloads/bin/fp-appimage-updater.x64"
                },
                {
                    "name": "fp-appimage-updater.ARM",
                    "url": "https://gitlab.com/fpsys/fp-appimage-updater/-/jobs/artifacts/main/raw/build/fp-appimage-updater.ARM?job=build-and-compress",
                    "direct_asset_url": "https://gitlab.com/fpsys/fp-appimage-updater/-/releases/v1.2.2/downloads/bin/fp-appimage-updater.ARM"
                }
            ]
        }
    });

    let assets = release_assets(
        ForgeHost::GitLab,
        &resp,
        "https://gitlab.com/fpsys/fp-appimage-updater",
    )
    .expect("expected gitlab assets");

    assert_eq!(assets.len(), 2);
    assert_eq!(assets[0].name, "fp-appimage-updater.x64");
    assert_eq!(
        assets[0].download_url,
        "https://gitlab.com/fpsys/fp-appimage-updater/-/releases/v1.2.2/downloads/bin/fp-appimage-updater.x64"
    );
}

#[test]
fn html_fallback_finds_matching_asset() {
    let html = r#"
        <a href="/owner/repo/releases/download/v1.2.3/app-x86_64.AppImage">download</a>
        <a href="/owner/repo/releases/download/v1.2.3/app-arm.AppImage">download</a>
    "#;
    let pattern = glob::Pattern::new("*x86_64.AppImage").unwrap();

    let (version, download_url) =
        find_release_asset_in_html(html, "/owner/repo", &pattern).expect("expected asset");

    assert_eq!(version, "v1.2.3");
    assert_eq!(
        download_url,
        "https://github.com/owner/repo/releases/download/v1.2.3/app-x86_64.AppImage"
    );
}

#[test]
fn html_fallback_uses_custom_release_base() {
    let html = r#"
        <a href="/owner/repo/releases/download/v1.2.3/app-x86_64.AppImage">download</a>
    "#;
    let pattern = glob::Pattern::new("*x86_64.AppImage").unwrap();

    let (version, download_url) =
        find_release_asset_in_html_with_base(html, "https://example.com/owner/repo", &pattern)
            .expect("expected asset");

    assert_eq!(version, "v1.2.3");
    assert_eq!(
        download_url,
        "https://example.com/owner/repo/releases/download/v1.2.3/app-x86_64.AppImage"
    );
}

#[test]
fn regex_asset_matcher_excludes_arm64_suffix() {
    let matcher = AssetMatcher::from_config(
        "*",
        Some(r"Obsidian-[0-9.]+\.AppImage"),
        "https://github.com/obsidianmd/obsidian-releases",
    )
    .expect("expected regex matcher");

    let html = r#"
        <a href="/obsidianmd/obsidian-releases/releases/download/v1.12.7/Obsidian-1.12.7-arm64.AppImage">download</a>
        <a href="/obsidianmd/obsidian-releases/releases/download/v1.12.7/Obsidian-1.12.7.AppImage">download</a>
    "#;

    let (version, download_url) = find_release_asset_in_html_with_base_and_matcher(
        html,
        "https://github.com/obsidianmd/obsidian-releases",
        &matcher,
    )
    .expect("expected regex matcher to find the generic asset");

    assert_eq!(version, "v1.12.7");
    assert_eq!(
        download_url,
        "https://github.com/obsidianmd/obsidian-releases/releases/download/v1.12.7/Obsidian-1.12.7.AppImage"
    );
}

#[test]
fn github_proxy_prefix_is_removed() {
    let proxied =
        "https://gh-proxy.com/https://github.com/owner/repo/releases/download/v1.2.3/app.AppImage";
    assert_eq!(
        sanitize_github_proxy_url(proxied, "https://gh-proxy.com/"),
        "https://github.com/owner/repo/releases/download/v1.2.3/app.AppImage"
    );
}

#[test]
fn github_proxy_release_url_supports_multiple_proxy_bases() {
    let gh_proxy = format!(
        "https://gh-proxy.com/{}",
        release_api_url(ForgeHost::GitHub, "https://github.com/owner/repo").unwrap()
    );
    assert_eq!(
        gh_proxy,
        "https://gh-proxy.com/https://api.github.com/repos/owner/repo/releases/latest"
    );

    let cors_proxy = format!(
        "https://corsproxy.io/?{}",
        release_api_url(ForgeHost::GitHub, "https://github.com/owner/repo").unwrap()
    );
    assert_eq!(
        cors_proxy,
        "https://corsproxy.io/?https://api.github.com/repos/owner/repo/releases/latest"
    );

    let allorigins = format!(
        "https://api.allorigins.win/raw?url={}",
        release_api_url(ForgeHost::GitHub, "https://github.com/owner/repo").unwrap()
    );
    assert_eq!(
        allorigins,
        "https://api.allorigins.win/raw?url=https://api.github.com/repos/owner/repo/releases/latest"
    );
}

#[test]
fn github_proxy_metadata_must_match_repository() {
    let resp = serde_json::json!({
        "url": "https://gh-proxy.com/https://api.github.com/repos/other/repo/releases/1",
        "html_url": "https://gh-proxy.com/https://github.com/other/repo/releases/tag/v1.2.3"
    });

    let err = validate_github_proxy_metadata(
        "https://github.com/owner/repo",
        &resp,
        "https://gh-proxy.com/",
    )
    .expect_err("expected repository mismatch to fail");

    let message = format!("{:#}", err);
    assert!(message.contains("different repository"));
}

#[test]
fn cache_app_metadata_stores_forge_details() {
    let mut state = AppState::default();

    cache_app_metadata(
        &mut state,
        vec!["segmented_downloads".to_string(), "etag".to_string()],
        Some(true),
        Some("https://git.linux.toys/fptbb/fp-appimage-updater".to_string()),
        Some(ForgeHost::Forgejo),
    );

    assert_eq!(
        state.forge_repository.as_deref(),
        Some("https://git.linux.toys/fptbb/fp-appimage-updater")
    );
    assert_eq!(state.forge_platform, Some(ForgeHost::Forgejo));
    assert_eq!(state.segmented_downloads, Some(true));
    assert_eq!(
        state.capabilities,
        vec!["etag".to_string(), "segmented_downloads".to_string()]
    );

    cache_app_metadata(&mut state, Vec::new(), None, None, None);
    assert_eq!(state.forge_repository, None);
    assert_eq!(state.forge_platform, None);
}
