use super::heuristics::should_retry_download_error;
use super::queue::UpdateErrorStage;
use super::types::{ForcedUpdateInfo, UpdateDownloadJob, UpdateWorkResult};
use crate::commands::helpers::rate_limit_from_error;
use crate::config;
use crate::downloader;
use crate::resolvers;
use crate::state::AppState;
use std::time::Instant;

pub(crate) fn process_update_check_job(
    client: &ureq::Agent,
    app: config::AppConfig,
    state: Option<AppState>,
    forced_update: Option<ForcedUpdateInfo>,
    github_proxy: bool,
    github_proxy_prefixes: Vec<String>,
    global_config: &config::GlobalConfig,
) -> UpdateWorkResult {
    let started_at = Instant::now();
    let app_name = app.name.clone();
    let from_version = state.as_ref().and_then(|s| s.local_version.clone());
    let current_path = state.as_ref().and_then(|s| s.file_path.clone());

    if let Some(forced_update) = forced_update {
        let state_for_download = state.clone().unwrap_or_default();
        return UpdateWorkResult::ReadyToDownload {
            app,
            state: state_for_download,
            from_version,
            current_path,
            info: resolvers::UpdateInfo {
                download_url: forced_update.download_url.clone(),
                version: forced_update.version.clone(),
                new_etag: None,
                new_last_modified: None,
            },
            elapsed: started_at.elapsed(),
            capabilities: Vec::new(),
            segmented_downloads: state.as_ref().and_then(|s| s.segmented_downloads),
            forge_repository: None,
            forge_platform: None,
        };
    }

    match resolvers::check_for_updates(
        &app,
        state.as_ref(),
        client,
        github_proxy,
        &github_proxy_prefixes,
        global_config,
    ) {
        Ok(result) => {
            let resolvers::CheckResult {
                update,
                capabilities,
                segmented_downloads: segmented_support,
                forge_repository,
                forge_platform,
            } = result;
            let Some(info) = update else {
                return UpdateWorkResult::UpToDate {
                    name: app_name,
                    from_version,
                    path: current_path,
                    elapsed: started_at.elapsed(),
                    capabilities,
                    segmented_downloads: segmented_support,
                    forge_repository,
                    forge_platform,
                };
            };
            UpdateWorkResult::ReadyToDownload {
                app,
                state: state.unwrap_or_default(),
                from_version,
                current_path,
                info,
                elapsed: started_at.elapsed(),
                capabilities,
                segmented_downloads: segmented_support,
                forge_repository,
                forge_platform,
            }
        }
        Err(e) => {
            let elapsed = started_at.elapsed();
            let rate_limited_until =
                rate_limit_from_error(&e).and_then(|info| info.until_epoch_seconds());
            if rate_limited_until.is_some() {
                UpdateWorkResult::RateLimited {
                    name: app_name,
                    from_version,
                    path: current_path,
                    elapsed,
                    rate_limited_until,
                }
            } else {
                UpdateWorkResult::Error {
                    stage: UpdateErrorStage::Check,
                    name: app_name,
                    from_version,
                    to_version: None,
                    path: current_path,
                    elapsed,
                    capabilities: Vec::new(),
                    segmented_downloads: None,
                    rate_limited_until: None,
                    error: format!("{:#}", e),
                    forge_repository: None,
                    forge_platform: None,
                    retry_job: None,
                }
            }
        }
    }
}

pub(crate) fn process_update_download_job(
    client: &ureq::Agent,
    job: UpdateDownloadJob,
    storage_dir: std::path::PathBuf,
    naming_format: String,
    segmented_downloads: bool,
    json_output: bool,
    color_output: bool,
) -> UpdateWorkResult {
    let started_at = Instant::now();
    let UpdateDownloadJob {
        app,
        state,
        from_version,
        current_path,
        info,
        capabilities,
        segmented_downloads: segmented_support,
        estimated_download_bytes,
        provider,
        forge_repository,
        forge_platform,
        retry_without_segmented_downloads,
        forced_update,
    } = job;
    let app_name = app.name.clone();
    let to_version = forced_update
        .as_ref()
        .map(|forced| forced.version.clone())
        .unwrap_or_else(|| info.version.clone());
    let segmented_downloads = if retry_without_segmented_downloads {
        false
    } else {
        segmented_downloads
    };

    let download_info = forced_update
        .as_ref()
        .map(|forced| resolvers::UpdateInfo {
            download_url: forced.download_url.clone(),
            version: forced.version.clone(),
            new_etag: None,
            new_last_modified: None,
        })
        .unwrap_or_else(|| info.clone());

    match downloader::download_app(
        client,
        &app,
        &download_info,
        &storage_dir,
        &naming_format,
        Some(&state),
        segmented_downloads,
        json_output,
        color_output,
    ) {
        Ok(download_result) => UpdateWorkResult::Updated {
            app,
            from_version,
            info: download_info,
            new_path: download_result.path,
            old_path: current_path.map(std::path::PathBuf::from),
            elapsed: started_at.elapsed(),
            downloaded_bytes: download_result.downloaded_bytes,
            download_elapsed: download_result.download_elapsed,
            capabilities,
            segmented_downloads: download_result.segmented_downloads.or(segmented_support),
            progress_completion_rendered: download_result.progress_completion_rendered,
            forge_repository,
            forge_platform,
        },
        Err(e) => {
            let retry_job = if !retry_without_segmented_downloads && should_retry_download_error(&e)
            {
                Some(UpdateDownloadJob {
                    app,
                    state,
                    from_version: from_version.clone(),
                    current_path,
                    info,
                    capabilities: capabilities.clone(),
                    segmented_downloads: segmented_support,
                    estimated_download_bytes,
                    provider,
                    forge_repository: forge_repository.clone(),
                    forge_platform: forge_platform.clone(),
                    retry_without_segmented_downloads: true,
                    forced_update: forced_update.clone(),
                })
            } else {
                None
            };

            UpdateWorkResult::Error {
                stage: UpdateErrorStage::Download,
                name: app_name,
                from_version,
                to_version: Some(to_version),
                path: None,
                elapsed: started_at.elapsed(),
                capabilities,
                segmented_downloads: segmented_support,
                rate_limited_until: None,
                error: format!("Download failed: {:#}", e),
                forge_repository,
                forge_platform,
                retry_job,
            }
        }
    }
}
