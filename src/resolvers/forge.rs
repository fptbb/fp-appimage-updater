use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use crate::state::AppState;
use super::UpdateInfo;

pub async fn resolve(
    client: &Client,
    repository: &str,
    asset_match: &str,
    state: Option<&AppState>,
) -> Result<Option<UpdateInfo>> {
    // Basic GitHub API extraction 
    // Example: https://github.com/owner/repo -> api.github.com/repos/owner/repo/releases/latest
    let url = if repository.starts_with("https://github.com/") {
        repository.replace("https://github.com/", "https://api.github.com/repos/") + "/releases/latest"
    } else {
        return Err(anyhow!("Only github.com is currently supported for forge strategy, because it's the most widely used, if you want to see support for other forge platforms please open an issue with the forge platform you'd like to see supported and an application that uses it as an example"));
    };

    let resp: serde_json::Value = client.get(&url).send().await?.error_for_status()?.json().await?;
    
    let version = resp["tag_name"]
        .as_str()
        .context("No tag_name in release")?
        .to_string();

    if let Some(s) = state && s.local_version.as_deref() == Some(&version) {
        return Ok(None); // Already up to date
    }

    // Find asset
    let assets = resp["assets"].as_array().context("No assets array in release")?;
    let pattern = glob::Pattern::new(asset_match)?;

    for asset in assets {
        if let Some(name) = asset["name"].as_str() && pattern.matches(name) && let Some(download_url) = asset["browser_download_url"].as_str() {
            return Ok(Some(UpdateInfo {
                download_url: download_url.to_string(),
                version,
                new_etag: None,
                new_last_modified: None,
            }));
        }
    }

    Err(anyhow!("No matching asset found for pattern {}", asset_match))
}
