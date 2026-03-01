use anyhow::{Context, Result};
use reqwest::Client;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{AppConfig, ZsyncConfig};
use crate::resolvers::UpdateInfo;
use crate::state::AppState;

pub async fn download_app(
    client: &Client,
    app: &AppConfig,
    update_info: &UpdateInfo,
    storage_dir: &Path,
    naming_format: &str,
    state: Option<&AppState>,
) -> Result<PathBuf> {
    let actual_storage_dir = app.storage_dir
        .as_ref()
        .map(|s| PathBuf::from(crate::integrator::expand_tilde(s)))
        .unwrap_or_else(|| storage_dir.to_path_buf());

    let file_name = naming_format
        .replace("{name}", &app.name)
        .replace("{version}", &update_info.version);

    let final_path = actual_storage_dir.join(&file_name);
    let tmp_path = actual_storage_dir.join(format!("{}.tmp", file_name));

    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let zsync_url = match &app.zsync {
        Some(ZsyncConfig::Enabled(true)) => Some(format!("{}.zsync", update_info.download_url)),
        Some(ZsyncConfig::Url(url)) => Some(url.clone()),
        _ => None,
    };

    let mut zsync_success = false;

    if let Some(zurl) = zsync_url {
        if let Some(old_path_str) = state.and_then(|s| s.file_path.as_ref()) {
            let old_path = Path::new(old_path_str);
            if old_path.exists() {
                if try_zsync(&zurl, old_path, &tmp_path) {
                    zsync_success = true;
                }
            }
        }
    }

    if !zsync_success {
        download_http(client, &update_info.download_url, &tmp_path).await?;
    }

    // Rename tmp to final
    std::fs::rename(&tmp_path, &final_path).context("Failed to rename tmp file to final destination")?;

    Ok(final_path)
}

fn try_zsync(zsync_url: &str, old_file: &Path, target_file: &Path) -> bool {
    // Run `zsync -i <old_file> -o <target_file> <zsync_url>`
    let status = Command::new("zsync")
        .arg("-i")
        .arg(old_file)
        .arg("-o")
        .arg(target_file)
        .arg(zsync_url)
        .status();

    match status {
        Ok(s) if s.success() => true,
        _ => {
            eprintln!("Warning: zsync failed or not found, falling back to full HTTP download.");
            false
        }
    }
}

async fn download_http(client: &Client, url: &str, target_path: &Path) -> Result<()> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    let mut file = File::create(target_path)?;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk)?;
    }

    Ok(())
}
