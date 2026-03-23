use anyhow::{Context, Result};
use ureq::Agent;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{AppConfig, ZsyncConfig};
use crate::resolvers::UpdateInfo;
use crate::output::{print_progress, print_warning};
use crate::state::AppState;

pub fn download_app(
    client: &Agent,
    app: &AppConfig,
    update_info: &UpdateInfo,
    storage_dir: &Path,
    naming_format: &str,
    state: Option<&AppState>,
    quiet: bool,
    colors: bool,
) -> Result<PathBuf> {
    let actual_storage_dir = app.storage_dir
        .as_ref()
        .map(|s| crate::integrator::expand_tilde(s))
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

    if let Some(zurl) = zsync_url && let Some(old_path_str) = state.and_then(|s| s.file_path.as_ref()) {
        let old_path = Path::new(old_path_str);
        if old_path.exists() && try_zsync(&zurl, old_path, &tmp_path, quiet, colors) {
            zsync_success = true;
        }
    }

    if !zsync_success {
        download_http(client, &update_info.download_url, &tmp_path)?;
    }

    // Rename tmp to final
    std::fs::rename(&tmp_path, &final_path).context("Failed to rename tmp file to final destination")?;

    Ok(final_path)
}

fn try_zsync(
    zsync_url: &str,
    old_file: &Path,
    target_file: &Path,
    quiet: bool,
    colors: bool,
) -> bool {
    if !quiet {
        print_progress(&format!("Attempting zsync update using: {}", zsync_url), colors);
    }
    // Run `zsync -i <old_file> -o <target_file> <zsync_url>`
    let status = Command::new("zsync")
        .arg("-i")
        .arg(old_file)
        .arg("-o")
        .arg(target_file)
        .arg(zsync_url)
        .status();

    match status {
        Ok(s) if s.success() => {
            if !quiet {
                print_progress("Successfully updated via zsync!", colors);
            }
            true
        }
        _ => {
            if !quiet {
                print_warning(
                    "zsync failed or not found, falling back to full HTTP download.",
                    colors,
                );
            }
            false
        }
    }
}

fn download_http(client: &Agent, url: &str, target_path: &Path) -> Result<()> {
    let response = client.get(url).call()?;
    let mut file = File::create(target_path)?;

    std::io::copy(&mut response.into_body().into_reader(), &mut file)?;

    Ok(())
}
