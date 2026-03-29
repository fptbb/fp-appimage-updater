use crate::state::AppState;
use anyhow::{Context, Result, anyhow};
use ureq::Agent;

use super::{CheckResult, UpdateInfo, dedupe_capabilities, rate_limit_info_from_headers};

pub fn resolve(
    client: &Agent,
    repository: &str,
    asset_match: &str,
    state: Option<&AppState>,
    github_proxy: bool,
    github_proxy_prefixes: &[String],
) -> Result<CheckResult> {
    let url = github_release_url(repository)?;
    let pattern = glob::Pattern::new(asset_match).with_context(|| {
        format!(
            "Invalid asset_match pattern '{}' for {}",
            asset_match, repository
        )
    })?;

    let response = client
        .get(&url)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .with_context(|| format!("Failed to reach GitHub release API for {}", repository))?;

    let status = response.status().as_u16();
    if status == 403 || status == 429 {
        let Some(rate_limit) = rate_limit_info_from_headers(response.headers()) else {
            return Err(anyhow!(
                "Rate limited by {} but no rate-limit headers were returned",
                repository
            ));
        };
        if github_proxy {
            return resolve_via_github_proxy(
                client,
                repository,
                &pattern,
                state,
                github_proxy_prefixes,
            )
            .map_err(|e| {
                anyhow::Error::from(rate_limit.clone()).context(format!(
                    "{} {}",
                    rate_limit.short_message(),
                    e
                ))
            });
        }

        let html_url = github_release_web_url(repository)?;
        let html = client
            .get(&html_url)
            .call()
            .with_context(|| {
                format!(
                    "{} GitHub release page fallback failed for {}",
                    rate_limit.short_message(),
                    repository
                )
            })?
            .into_body()
            .read_to_string()
            .with_context(|| format!("Failed to read GitHub release page for {}", repository))?;

        return resolve_from_github_release_page(client, repository, &pattern, &html, state)
            .map_err(|e| {
                anyhow::Error::from(rate_limit.clone()).context(format!(
                    "{} {}",
                    rate_limit.short_message(),
                    e
                ))
            });
    }

    if !response.status().is_success() {
        return Err(anyhow!(
            "GitHub release API returned {} for {}",
            response.status(),
            repository
        ));
    }

    let resp: serde_json::Value = response
        .into_body()
        .read_json()
        .with_context(|| format!("Failed to parse GitHub release metadata for {}", repository))?;
    let version = resp["tag_name"].as_str().with_context(|| {
        format!(
            "Release metadata for {} did not contain tag_name",
            repository
        )
    })?;
    let assets = resp["assets"].as_array().with_context(|| {
        format!(
            "Release metadata for {} did not contain an assets array",
            repository
        )
    })?;

    resolve_from_release_assets(
        client,
        repository,
        &pattern,
        version,
        assets,
        state,
        false,
        github_proxy_prefixes
            .first()
            .map(|prefix| prefix.as_str())
            .unwrap_or(""),
    )
}

fn github_release_url(repository: &str) -> Result<String> {
    if repository.starts_with("https://github.com/") {
        Ok(
            repository.replace("https://github.com/", "https://api.github.com/repos/")
                + "/releases/latest",
        )
    } else {
        Err(anyhow!(
            "Only github.com is currently supported for forge strategy, got {}",
            repository
        ))
    }
}

fn github_release_web_url(repository: &str) -> Result<String> {
    if repository.starts_with("https://github.com/") {
        Ok(format!("{}/releases/latest", repository))
    } else {
        Err(anyhow!(
            "Only github.com is currently supported for forge strategy, got {}",
            repository
        ))
    }
}

fn github_repo_path(repository: &str) -> Result<&str> {
    repository
        .strip_prefix("https://github.com/")
        .ok_or_else(|| {
            anyhow!(
                "Only github.com is currently supported for forge strategy, got {}",
                repository
            )
        })
}

fn github_proxy_release_url(repository: &str, github_proxy_prefix: &str) -> Result<String> {
    Ok(format!(
        "{}{}",
        github_proxy_prefix,
        github_release_url(repository)?
    ))
}

fn sanitize_github_proxy_url(url: &str, github_proxy_prefix: &str) -> String {
    url.strip_prefix(github_proxy_prefix)
        .unwrap_or(url)
        .to_string()
}

fn validate_github_proxy_metadata(
    repository: &str,
    resp: &serde_json::Value,
    github_proxy_prefix: &str,
) -> Result<()> {
    let repo_path = github_repo_path(repository)?;
    let expected_api_prefix = format!("https://api.github.com/repos/{}/releases/", repo_path);
    let expected_web_prefix = format!("https://github.com/{}/", repo_path);

    if let Some(api_url) = resp["url"].as_str() {
        let api_url = sanitize_github_proxy_url(api_url, github_proxy_prefix);
        if !api_url.starts_with(&expected_api_prefix) {
            return Err(anyhow!(
                "GitHub proxy returned metadata for a different repository: {}",
                api_url
            ));
        }
    }

    if let Some(html_url) = resp["html_url"].as_str() {
        let html_url = sanitize_github_proxy_url(html_url, github_proxy_prefix);
        if !html_url.starts_with(&expected_web_prefix) {
            return Err(anyhow!(
                "GitHub proxy returned metadata for a different repository: {}",
                html_url
            ));
        }
    }

    Ok(())
}

fn validate_github_download_url(repo_path: &str, download_url: &str) -> Result<()> {
    let expected = format!("https://github.com/{}/releases/download/", repo_path);
    if download_url.starts_with(&expected) {
        Ok(())
    } else {
        Err(anyhow!(
            "GitHub proxy returned a download URL for a different repository: {}",
            download_url
        ))
    }
}

fn resolve_from_release_assets(
    client: &Agent,
    repository: &str,
    pattern: &glob::Pattern,
    version: &str,
    assets: &[serde_json::Value],
    state: Option<&AppState>,
    github_proxy: bool,
    github_proxy_prefix: &str,
) -> Result<CheckResult> {
    let repo_path = github_repo_path(repository)?;
    for asset in assets {
        if let Some(name) = asset["name"].as_str()
            && pattern.matches(name)
            && let Some(download_url) = asset["browser_download_url"].as_str()
        {
            let download_url = if github_proxy {
                let sanitized = sanitize_github_proxy_url(download_url, github_proxy_prefix);
                validate_github_download_url(repo_path, &sanitized)?;
                sanitized
            } else {
                download_url.to_string()
            };
            return build_check_result(
                client,
                repository,
                version.to_string(),
                download_url.to_string(),
                state,
            );
        }
    }

    let available_assets = assets
        .iter()
        .filter_map(|asset| asset["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ");

    Err(anyhow!(
        "No matching asset found for repository {} with pattern '{}'. Available assets: {}",
        repository,
        pattern.as_str(),
        if available_assets.is_empty() {
            "<none>".to_string()
        } else {
            available_assets
        }
    ))
}

fn resolve_via_github_proxy(
    client: &Agent,
    repository: &str,
    pattern: &glob::Pattern,
    state: Option<&AppState>,
    github_proxy_prefixes: &[String],
) -> Result<CheckResult> {
    let mut last_error = None;

    for github_proxy_prefix in github_proxy_prefixes {
        match resolve_via_single_github_proxy(
            client,
            repository,
            pattern,
            state,
            github_proxy_prefix,
        ) {
            Ok(result) => return Ok(result),
            Err(err) => {
                last_error = Some(err.context(format!(
                    "GitHub proxy {} failed for {}",
                    github_proxy_prefix, repository
                )));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        anyhow!(
            "GitHub proxy is enabled for {} but no proxy prefixes were configured",
            repository
        )
    }))
}

fn resolve_via_single_github_proxy(
    client: &Agent,
    repository: &str,
    pattern: &glob::Pattern,
    state: Option<&AppState>,
    github_proxy_prefix: &str,
) -> Result<CheckResult> {
    let proxy_url = github_proxy_release_url(repository, github_proxy_prefix)?;
    let response = client
        .get(&proxy_url)
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .with_context(|| format!("Failed to reach GitHub proxy for {}", repository))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "GitHub proxy returned {} for {}",
            response.status(),
            repository
        ));
    }

    let resp: serde_json::Value = response.into_body().read_json().with_context(|| {
        format!(
            "Failed to parse proxied release metadata for {}",
            repository
        )
    })?;
    validate_github_proxy_metadata(repository, &resp, github_proxy_prefix)?;
    let version = resp["tag_name"].as_str().with_context(|| {
        format!(
            "Release metadata for {} did not contain tag_name",
            repository
        )
    })?;
    let assets = resp["assets"].as_array().with_context(|| {
        format!(
            "Release metadata for {} did not contain an assets array",
            repository
        )
    })?;

    resolve_from_release_assets(
        client,
        repository,
        pattern,
        version,
        assets,
        state,
        true,
        github_proxy_prefix,
    )
}

fn resolve_from_github_release_page(
    client: &Agent,
    repository: &str,
    pattern: &glob::Pattern,
    html: &str,
    state: Option<&AppState>,
) -> Result<CheckResult> {
    let repo_path = github_repo_path(repository)?;
    let Some((version, download_url)) = find_release_asset_in_html(html, repo_path, pattern) else {
        return Err(anyhow!(
            "Rate limited for {} and no matching asset was found on the release page for pattern '{}'",
            repository,
            pattern.as_str()
        ));
    };

    build_check_result(client, repository, version, download_url, state)
}

fn build_check_result(
    client: &Agent,
    repository: &str,
    version: String,
    download_url: String,
    state: Option<&AppState>,
) -> Result<CheckResult> {
    let mut capabilities = Vec::new();
    let mut segmented_downloads = Some(false);

    if let Ok(head_resp) = client.head(&download_url).call()
        && let Some(range_header) = head_resp
            .headers()
            .get("Accept-Ranges")
            .and_then(|value| value.to_str().ok())
        && range_header.trim().eq_ignore_ascii_case("bytes")
    {
        capabilities.push("segmented_downloads".to_string());
        segmented_downloads = Some(true);
    }

    dedupe_capabilities(&mut capabilities);

    if version.is_empty() {
        return Err(anyhow!(
            "Release metadata for {} did not contain a version",
            repository
        ));
    }

    let update = if state.and_then(|s| s.local_version.as_deref()) == Some(version.as_str()) {
        None
    } else {
        Some(UpdateInfo {
            download_url,
            version,
            new_etag: None,
            new_last_modified: None,
        })
    };

    Ok(CheckResult {
        update,
        capabilities,
        segmented_downloads,
    })
}

fn find_release_asset_in_html(
    html: &str,
    repo_path: &str,
    pattern: &glob::Pattern,
) -> Option<(String, String)> {
    let needle = format!("{}/releases/download/", repo_path);
    let mut search_start = 0usize;

    while let Some(relative_idx) = html[search_start..].find(&needle) {
        let start = search_start + relative_idx;
        let after = &html[start + needle.len()..];
        let tag_end = after.find('/')?;
        let version = &after[..tag_end];
        let after_tag = &after[tag_end + 1..];
        let file_end = after_tag
            .find(|c: char| c == '"' || c == '\'' || c == '?' || c == '<' || c.is_whitespace())
            .unwrap_or(after_tag.len());
        let asset_name = &after_tag[..file_end];

        if pattern.matches(asset_name) {
            let download_url = format!(
                "https://github.com{}/{}/{}",
                repo_path,
                "releases/download",
                format!("{}/{}", version, asset_name)
            );
            return Some((version.to_string(), download_url));
        }

        search_start = start + needle.len();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ureq::Agent;

    #[test]
    fn invalid_asset_pattern_is_reported_before_network() {
        let client = Agent::new_with_defaults();
        let err = resolve(
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
        let err = resolve(
            &client,
            "https://example.com/owner/repo",
            "*",
            None,
            false,
            &[String::from("https://gh-proxy.com/")],
        )
        .expect_err("expected unsupported repository to fail");

        let message = format!("{:#}", err);
        assert!(message.contains("Only github.com is currently supported"));
        assert!(message.contains("https://example.com/owner/repo"));
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
        let proxied = "https://gh-proxy.com/https://github.com/owner/repo/releases/download/v1.2.3/app.AppImage";
        assert_eq!(
            sanitize_github_proxy_url(proxied, "https://gh-proxy.com/"),
            "https://github.com/owner/repo/releases/download/v1.2.3/app.AppImage"
        );
    }

    #[test]
    fn github_proxy_release_url_supports_multiple_proxy_bases() {
        let release_url =
            github_proxy_release_url("https://github.com/owner/repo", "https://corsproxy.io/?")
                .expect("expected proxy url");
        assert_eq!(
            release_url,
            "https://corsproxy.io/?https://api.github.com/repos/owner/repo/releases/latest"
        );

        let release_url = github_proxy_release_url(
            "https://github.com/owner/repo",
            "https://api.allorigins.win/raw?url=",
        )
        .expect("expected proxy url");
        assert_eq!(
            release_url,
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
}
