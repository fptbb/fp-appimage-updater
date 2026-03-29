use fp_appimage_updater::resolvers::forge::{
    ForgeHost, find_release_asset_in_html, release_api_url, release_assets,
    sanitize_github_proxy_url, validate_github_proxy_metadata,
};
use ureq::Agent;

#[test]
fn invalid_asset_pattern_is_reported_before_network() {
    let client = Agent::new_with_defaults();
    let err = fp_appimage_updater::resolvers::forge::resolve(
        &client,
        "https://github.com/fptbb/fp-appimage-updater",
        "[",
        None,
        false,
        &[String::from("https://gh-proxy.com/")],
    )
    .expect_err("expected invalid asset pattern to fail");

    let message = format!("{:#}", err);
    assert!(message.contains("Invalid asset_match pattern"));
    assert!(message.contains("https://github.com/fptbb/fp-appimage-updater"));
}

#[test]
fn unsupported_repository_is_reported() {
    let client = Agent::new_with_defaults();
    let err = fp_appimage_updater::resolvers::forge::resolve(
        &client,
        "https://example.com/owner/repo",
        "*",
        None,
        false,
        &[String::from("https://gh-proxy.com/")],
    )
    .expect_err("expected unsupported repository to fail");

    let message = format!("{:#}", err);
    assert!(message.contains("Only github.com and gitlab.com are currently supported"));
    assert!(message.contains("https://example.com/owner/repo"));
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
