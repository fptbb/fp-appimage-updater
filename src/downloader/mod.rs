use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ureq::Agent;

pub mod http;
pub mod progress;

pub use http::*;
pub use progress::*;

use crate::config::{AppConfig, ZsyncConfig};
use crate::resolvers::UpdateInfo;
use crate::state::AppState;

#[derive(Debug)]
pub struct DownloadResult {
    pub path: PathBuf,
    pub segmented_downloads: Option<bool>,
    pub progress_completion_rendered: bool,
    pub downloaded_bytes: u64,
    pub download_elapsed: Option<Duration>,
}

pub fn suspend_for_print<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    if let Ok(mut ui) = progress_ui().lock() {
        let _ = ui.clear_rendered();
        let result = f();
        let _ = ui.draw();
        result
    } else {
        f()
    }
}

pub fn finalize_progress_output() -> Result<()> {
    if let Ok(mut ui) = progress_ui().lock() {
        ui.clear_all()?;
    }
    Ok(())
}

pub fn download_app(
    client: &Agent,
    app: &AppConfig,
    update_info: &UpdateInfo,
    storage_dir: &Path,
    naming_format: &str,
    state: Option<&AppState>,
    segmented_downloads: bool,
    quiet: bool,
    colors: bool,
) -> Result<DownloadResult> {
    let actual_storage_dir = app
        .storage_dir
        .as_ref()
        .map(|s| crate::integrator::expand_tilde(s))
        .unwrap_or_else(|| storage_dir.to_path_buf());

    let file_name = naming_format
        .replace("{name}", &app.name)
        .replace("{version}", &update_info.version);

    let final_path = actual_storage_dir.join(&file_name);
    let tmp_path = actual_storage_dir.join(format!("{}.tmp", file_name));

    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // zsync is a delta path: try an existing AppImage first, then fall back to HTTP.
    let zsync_url = match &app.zsync {
        Some(ZsyncConfig::Enabled(true)) => Some(format!("{}.zsync", update_info.download_url)),
        Some(ZsyncConfig::Url(url)) => Some(url.clone()),
        _ => None,
    };

    let mut zsync_success = false;
    let mut segmented_downloads_support = state.and_then(|s| s.segmented_downloads);

    if let Some(zurl) = zsync_url
        && let Some(old_path_str) = state.and_then(|s| s.file_path.as_ref())
    {
        let old_path = Path::new(old_path_str);
        if old_path.exists() && try_zsync(&zurl, old_path, &tmp_path, quiet, colors) {
            zsync_success = true;
        }
    }

    if !zsync_success {
        let download_started = Instant::now();
        let mut progress_completion_rendered = false;
        let (segmented_success, segmented_support, segmented_progress_displayed) =
            if segmented_downloads {
                try_segmented_http_download(
                    client,
                    &app.name,
                    &update_info.version,
                    &update_info.download_url,
                    &tmp_path,
                    segmented_downloads_support,
                    quiet,
                    colors,
                )
            } else {
                (false, segmented_downloads_support, false)
            };
        segmented_downloads_support = segmented_support;
        progress_completion_rendered |= segmented_progress_displayed;

        if !segmented_success {
            let (_download_progress_displayed, download_progress_completion_rendered) =
                download_http(
                    client,
                    &app.name,
                    &update_info.version,
                    &update_info.download_url,
                    &tmp_path,
                    quiet,
                    colors,
                )?;
            progress_completion_rendered |= download_progress_completion_rendered;
        }

        std::fs::rename(&tmp_path, &final_path)
            .context("Failed to rename tmp file to final destination")?;

        let downloaded_bytes = fs::metadata(&final_path)
            .map(|meta| meta.len())
            .unwrap_or(0);

        return Ok(DownloadResult {
            path: final_path,
            segmented_downloads: segmented_downloads_support,
            progress_completion_rendered,
            downloaded_bytes,
            download_elapsed: Some(download_started.elapsed()),
        });
    }

    std::fs::rename(&tmp_path, &final_path)
        .context("Failed to rename tmp file to final destination")?;

    let downloaded_bytes = fs::metadata(&final_path)
        .map(|meta| meta.len())
        .unwrap_or(0);

    Ok(DownloadResult {
        path: final_path,
        segmented_downloads: segmented_downloads_support,
        progress_completion_rendered: false,
        downloaded_bytes,
        download_elapsed: None,
    })
}
