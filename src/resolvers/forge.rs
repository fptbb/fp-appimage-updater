use crate::state::AppState;
use anyhow::{Context, Result, anyhow};
use ureq::Agent;

use super::{CheckResult, UpdateInfo, dedupe_capabilities};

pub fn resolve(
    client: &Agent,
    repository: &str,
    asset_match: &str,
    state: Option<&AppState>,
) -> Result<CheckResult> {
    let url = github_release_url(repository)?;
    let pattern = glob::Pattern::new(asset_match).with_context(|| {
        format!(
            "Invalid asset_match pattern '{}' for {}",
            asset_match, repository
        )
    })?;

    let resp: serde_json::Value = client
        .get(&url)
        .call()
        .with_context(|| format!("Failed to reach GitHub release API for {}", repository))?
        .into_body()
        .read_json()
        .with_context(|| format!("Failed to parse GitHub release metadata for {}", repository))?;

    let version = resp["tag_name"]
        .as_str()
        .with_context(|| {
            format!(
                "Release metadata for {} did not contain tag_name",
                repository
            )
        })?
        .to_string();

    let assets = resp["assets"].as_array().with_context(|| {
        format!(
            "Release metadata for {} did not contain an assets array",
            repository
        )
    })?;

    for asset in assets {
        if let Some(name) = asset["name"].as_str()
            && pattern.matches(name)
            && let Some(download_url) = asset["browser_download_url"].as_str()
        {
            let mut capabilities = Vec::new();

            if let Ok(head_resp) = client.head(download_url).call()
                && let Some(range_header) = head_resp
                    .headers()
                    .get("Accept-Ranges")
                    .and_then(|value| value.to_str().ok())
                && range_header.trim().eq_ignore_ascii_case("bytes")
            {
                capabilities.push("segmented_downloads".to_string());
            }

            dedupe_capabilities(&mut capabilities);

            let update = if state
                .and_then(|s| s.local_version.as_deref())
                == Some(version.as_str())
            {
                None
            } else {
                Some(UpdateInfo {
                    download_url: download_url.to_string(),
                    version: version.clone(),
                    new_etag: None,
                    new_last_modified: None,
                })
            };

            return Ok(CheckResult { update, capabilities });
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
        asset_match,
        if available_assets.is_empty() {
            "<none>".to_string()
        } else {
            available_assets
        }
    ))
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
        )
        .expect_err("expected invalid asset pattern to fail");

        let message = format!("{:#}", err);
        assert!(message.contains("Invalid asset_match pattern"));
        assert!(message.contains("https://github.com/fptbb/fp-appimage-updater"));
    }

    #[test]
    fn unsupported_repository_is_reported() {
        let client = Agent::new_with_defaults();
        let err = resolve(&client, "https://example.com/owner/repo", "*", None)
            .expect_err("expected unsupported repository to fail");

        let message = format!("{:#}", err);
        assert!(message.contains("Only github.com is currently supported"));
        assert!(message.contains("https://example.com/owner/repo"));
    }
}
