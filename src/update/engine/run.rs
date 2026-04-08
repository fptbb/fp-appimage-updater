use super::heuristics::{
    adapt_download_limit, download_provider_key, estimate_download_bytes, median_speed_bps,
    update_work_elapsed,
};
use super::queue::{DownloadQueues, ProviderDownloadScheduler, UpdateErrorStage};
use super::types::{UpdateDownloadJob, UpdateEvent, UpdateWorkResult};
use super::workers::{process_update_check_job, process_update_download_job};
use crate::commands::helpers::*;
use crate::config;
use crate::downloader;
use crate::integrator;
use crate::output::{
    format_rate_limit_retry_text, print_json, print_progress, print_success, print_warning,
    UpdateApp, UpdateResponse, UpdateStatus,
};
use crate::parser::AppConfigLoadError;
use crate::state::{AppState, StateManager};
use anyhow::Result;
use std::sync::mpsc;

pub fn run(
    app_configs: &[config::AppConfig],
    app_config_errors: &[AppConfigLoadError],
    global_config: &config::GlobalConfig,
    state_manager: &mut StateManager,
    client: &ureq::Agent,
    app_name: Option<&str>,
    json_output: bool,
    color_output: bool,
) -> Result<()> {
    let storage_dir = integrator::expand_tilde(&global_config.storage_dir);
    let mut results = Vec::new();
    let mut found = false;
    let mut rate_limit_note_needed = false;
    let mut deferred_status_messages = Vec::new();
    let mut deferred_warning_messages = Vec::new();
    let mut total_downloaded_bytes = 0u64;
    let mut download_speed_samples: Vec<f64> = Vec::new();
    let mut peak_download_bps: Option<f64> = None;
    let now = now_epoch_seconds();
    let mut pending_checks = Vec::new();

    for app in app_configs {
        if let Some(target) = app_name
            && app.name != *target
        {
            continue;
        }
        found = true;
        let state = snapshot_app_state(state_manager, &app.name, now);
        let rate_limited_until = state.rate_limited_until;
        let github_proxy = github_proxy_enabled(app, global_config);
        if rate_limit_enabled(app, global_config)
            && !(github_proxy && app_uses_github_forge(app))
            && matches!(rate_limited_until, Some(until) if until > now)
        {
            rate_limit_note_needed = true;
            results.push(UpdateApp {
                name: app.name.clone(),
                status: UpdateStatus::RateLimited,
                from_version: state.local_version.clone(),
                to_version: None,
                path: state.file_path.clone(),
                rate_limited_until,
                duration_seconds: None,
                error: None,
            });
            if !json_output {
                deferred_warning_messages.push(format!(
                    "Skipping {} ({})",
                    app.name,
                    format_rate_limit_retry_text(rate_limited_until)
                ));
            }
            continue;
        }
        let github_proxy_prefixes = github_proxy_prefixes(app, global_config);
        pending_checks.push((
            app.clone(),
            Some(state),
            github_proxy,
            github_proxy_prefixes,
        ));
    }

    let hard_max_check = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(MAX_CONCURRENT_CHECK_JOBS)
        .max(1);
    let hard_max_download = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(MAX_CONCURRENT_DOWNLOADS)
        .max(1);
    let mut check_limit = hard_max_check;
    let mut download_limit = hard_max_download;
    let mut download_scheduler = ProviderDownloadScheduler::new();
    let mut retry_downloads = DownloadQueues::new();

    let (tx, rx) = mpsc::channel::<UpdateEvent>();
    let mut pending_checks = pending_checks.into_iter().peekable();
    let mut pending_downloads = DownloadQueues::new();
    let mut active_checks = 0usize;
    let mut active_downloads = 0usize;

    let spawn_check_worker = |app: config::AppConfig,
                              state: Option<AppState>,
                              client: ureq::Agent,
                              github_proxy: bool,
                              github_proxy_prefixes: Vec<String>,
                              global_config: config::GlobalConfig,
                              tx: mpsc::Sender<UpdateEvent>| {
        std::thread::spawn(move || {
            let _ = tx.send(UpdateEvent::Check(process_update_check_job(
                &client,
                app,
                state,
                github_proxy,
                github_proxy_prefixes,
                &global_config,
            )));
        });
    };

    let spawn_download_worker = |job: UpdateDownloadJob,
                                 client: ureq::Agent,
                                 storage_dir: std::path::PathBuf,
                                 naming_format: String,
                                 segmented_downloads: bool,
                                 tx: mpsc::Sender<UpdateEvent>,
                                 json_output: bool,
                                 color_output: bool| {
        let provider = job.provider.clone();
        std::thread::spawn(move || {
            let result = process_update_download_job(
                &client,
                job,
                storage_dir,
                naming_format,
                segmented_downloads,
                json_output,
                color_output,
            );
            let _ = tx.send(UpdateEvent::Download { provider, result });
        });
    };

    let spawn_pending_downloads = |pending_downloads: &mut DownloadQueues,
                                   active_downloads: &mut usize,
                                   download_limit: usize,
                                   download_scheduler: &mut ProviderDownloadScheduler,
                                   tx: &mpsc::Sender<UpdateEvent>,
                                   global_config: &config::GlobalConfig,
                                   storage_dir: &std::path::Path,
                                   client: &ureq::Agent,
                                   json_output: bool,
                                   color_output: bool| {
        if pending_downloads.is_empty() {
            return;
        }

        let mut rotations = 0usize;
        let pending_len = pending_downloads.normal.len();
        while *active_downloads < download_limit
            && !pending_downloads.normal.is_empty()
            && rotations < pending_len
        {
            let Some(job) = pending_downloads.pop_next(false) else {
                break;
            };

            if download_scheduler.try_acquire(&job.provider, download_limit) {
                let storage_dir = storage_dir.to_path_buf();
                let naming_format = global_config.naming_format.clone();
                let segmented_downloads = job
                    .segmented_downloads
                    .or(job.app.segmented_downloads)
                    .unwrap_or(global_config.segmented_downloads);
                let segmented_downloads = if job.retry_without_segmented_downloads {
                    false
                } else {
                    segmented_downloads
                };
                let download_client = if job.retry_without_segmented_downloads {
                    build_http_agent()
                } else {
                    client.clone()
                };
                spawn_download_worker(
                    job,
                    download_client,
                    storage_dir,
                    naming_format,
                    segmented_downloads,
                    tx.clone(),
                    json_output,
                    color_output,
                );
                *active_downloads += 1;
                rotations = 0;
            } else {
                pending_downloads.normal.push_back(job);
                rotations += 1;
            }
        }

        if *active_downloads == 0 && pending_downloads.normal.is_empty() {
            let large_limit = 1usize;
            while *active_downloads < large_limit {
                let Some(job) = pending_downloads.pop_next(true) else {
                    break;
                };

                if download_scheduler.try_acquire(&job.provider, download_limit) {
                    let storage_dir = storage_dir.to_path_buf();
                    let naming_format = global_config.naming_format.clone();
                    let segmented_downloads = job
                        .segmented_downloads
                        .or(job.app.segmented_downloads)
                        .unwrap_or(global_config.segmented_downloads);
                    let segmented_downloads = if job.retry_without_segmented_downloads {
                        false
                    } else {
                        segmented_downloads
                    };
                    let download_client = if job.retry_without_segmented_downloads {
                        build_http_agent()
                    } else {
                        client.clone()
                    };
                    spawn_download_worker(
                        job,
                        download_client,
                        storage_dir,
                        naming_format,
                        segmented_downloads,
                        tx.clone(),
                        json_output,
                        color_output,
                    );
                    *active_downloads += 1;
                } else {
                    pending_downloads.large.push_front(job);
                    break;
                }
            }
        }
    };

    while active_checks < check_limit {
        if let Some((app, state, github_proxy, github_proxy_prefixes)) = pending_checks.next() {
            spawn_check_worker(
                app,
                state,
                client.clone(),
                github_proxy,
                github_proxy_prefixes,
                global_config.clone(),
                tx.clone(),
            );
            active_checks += 1;
        } else {
            break;
        }
    }

    spawn_pending_downloads(
        &mut pending_downloads,
        &mut active_downloads,
        download_limit,
        &mut download_scheduler,
        &tx,
        global_config,
        &storage_dir,
        client,
        json_output,
        color_output,
    );

    while active_checks > 0
        || active_downloads > 0
        || !pending_downloads.is_empty()
        || !retry_downloads.is_empty()
    {
        if active_checks == 0 && active_downloads == 0 && pending_downloads.is_empty() {
            if !retry_downloads.is_empty() {
                download_scheduler = ProviderDownloadScheduler::new();
                download_limit = 1;
                pending_downloads = retry_downloads;
                retry_downloads = DownloadQueues::new();
                spawn_pending_downloads(
                    &mut pending_downloads,
                    &mut active_downloads,
                    download_limit,
                    &mut download_scheduler,
                    &tx,
                    global_config,
                    &storage_dir,
                    client,
                    json_output,
                    color_output,
                );
            }
        }

        let event = rx.recv().expect("update worker panicked");
        match event {
            UpdateEvent::Check(result) => {
                active_checks = active_checks.saturating_sub(1);
                let elapsed = update_work_elapsed(&result);

                match result {
                    UpdateWorkResult::ReadyToDownload {
                        app,
                        state,
                        from_version,
                        current_path,
                        info,
                        elapsed: _,
                        capabilities,
                        segmented_downloads,
                        forge_repository,
                        forge_platform,
                    } => {
                        let provider = download_provider_key(&info.download_url);
                        let estimated_download_bytes =
                            estimate_download_bytes(&state, current_path.as_deref());
                        pending_downloads.push(UpdateDownloadJob {
                            app,
                            state,
                            from_version,
                            current_path,
                            info,
                            capabilities,
                            segmented_downloads,
                            estimated_download_bytes,
                            provider,
                            forge_repository,
                            forge_platform,
                            retry_without_segmented_downloads: false,
                        });
                    }
                    UpdateWorkResult::UpToDate {
                        name,
                        from_version,
                        path,
                        elapsed: _,
                        capabilities,
                        segmented_downloads,
                        forge_repository,
                        forge_platform,
                    } => {
                        let state = state_manager.get_app_mut(&name);
                        cache_app_metadata(
                            state,
                            capabilities,
                            segmented_downloads,
                            forge_repository,
                            forge_platform,
                        );
                        results.push(UpdateApp {
                            name: name.clone(),
                            status: UpdateStatus::UpToDate,
                            from_version: from_version.clone(),
                            to_version: None,
                            path,
                            rate_limited_until: None,
                            duration_seconds: None,
                            error: None,
                        });
                        if !json_output {
                            deferred_status_messages.push(format!(
                                "{} is already up to date ({})",
                                name,
                                from_version.unwrap_or_else(|| "unknown".to_string())
                            ));
                        }
                    }
                    UpdateWorkResult::RateLimited {
                        name,
                        from_version,
                        path,
                        elapsed,
                        rate_limited_until,
                    } => {
                        let state = state_manager.get_app_mut(&name);
                        state.rate_limited_until = rate_limited_until;
                        results.push(UpdateApp {
                            name: name.clone(),
                            status: UpdateStatus::RateLimited,
                            from_version,
                            to_version: None,
                            path,
                            rate_limited_until,
                            duration_seconds: None,
                            error: None,
                        });
                        rate_limit_note_needed = true;
                        if !json_output {
                            deferred_warning_messages.push(format!(
                                "Skipping {} ({})",
                                name,
                                format_rate_limit_retry_text(rate_limited_until)
                            ));
                        }
                        check_limit = adapt_worker_limit(
                            check_limit,
                            elapsed,
                            pending_checks.len(),
                            hard_max_check,
                        );
                    }
                    UpdateWorkResult::Error {
                        stage,
                        name,
                        from_version,
                        to_version,
                        path,
                        elapsed: _,
                        capabilities,
                        segmented_downloads,
                        rate_limited_until,
                        error,
                        forge_repository,
                        forge_platform,
                        retry_job,
                    } => {
                        if let Some(retry_job) = retry_job {
                            retry_downloads.push(retry_job);
                            continue;
                        }
                        let state = state_manager.get_app_mut(&name);
                        cache_app_metadata(
                            state,
                            capabilities,
                            segmented_downloads,
                            forge_repository,
                            forge_platform,
                        );
                        state.rate_limited_until = rate_limited_until;
                        results.push(UpdateApp {
                            name: name.clone(),
                            status: UpdateStatus::Error,
                            from_version,
                            to_version,
                            path,
                            rate_limited_until,
                            duration_seconds: None,
                            error: Some(error.clone()),
                        });
                        if !json_output {
                            match stage {
                                UpdateErrorStage::Check => deferred_warning_messages.push(format!(
                                    "Error checking updates for {}: {}",
                                    name, error
                                )),
                                UpdateErrorStage::Download => deferred_warning_messages
                                    .push(format!("Download failed for {}: {}", name, error)),
                            }
                        }
                    }
                    _ => {}
                }

                check_limit =
                    adapt_worker_limit(check_limit, elapsed, pending_checks.len(), hard_max_check);
            }
            UpdateEvent::Download { provider, result } => {
                active_downloads = active_downloads.saturating_sub(1);
                download_scheduler.release(&provider);
                let _elapsed = update_work_elapsed(&result);

                match result {
                    UpdateWorkResult::Updated {
                        app,
                        from_version,
                        info,
                        new_path,
                        old_path,
                        elapsed,
                        downloaded_bytes,
                        download_elapsed,
                        capabilities,
                        segmented_downloads,
                        progress_completion_rendered,
                        forge_repository,
                        forge_platform,
                    } => {
                        let app_name = app.name.clone();
                        let to_version = info.version.clone();
                        let old_path_ref = old_path.as_deref();
                        let was_update = from_version.is_some();
                        let download_bps = download_elapsed.map(|download_elapsed| {
                            if download_elapsed.is_zero() {
                                0.0
                            } else {
                                downloaded_bytes as f64 / download_elapsed.as_secs_f64().max(0.001)
                            }
                        });

                        if let Err(e) =
                            integrator::integrate(&app, global_config, &new_path, old_path_ref)
                        {
                            let state = state_manager.get_app_mut(&app_name);
                            cache_app_metadata(
                                state,
                                capabilities,
                                segmented_downloads,
                                forge_repository,
                                forge_platform,
                            );
                            results.push(UpdateApp {
                                name: app_name.clone(),
                                status: UpdateStatus::Error,
                                from_version: from_version.clone(),
                                to_version: Some(to_version.clone()),
                                path: Some(new_path.to_string_lossy().to_string()),
                                rate_limited_until: None,
                                duration_seconds: None,
                                error: Some(format!("Integration failed: {:#}", e)),
                            });
                            if !json_output {
                                deferred_warning_messages
                                    .push(format!("Integration failed for {}: {:#}", app_name, e));
                                deferred_warning_messages.push(format!(
                                    "Rolling back {} to its previous state...",
                                    app_name
                                ));
                            }

                            integrator::rollback(&app, global_config, &new_path, old_path_ref);
                        } else {
                            let duration_seconds = elapsed.as_secs_f64();
                            let state_mut = state_manager.get_app_mut(&app_name);
                            state_mut.local_version = Some(to_version.clone());
                            state_mut.rate_limited_until = None;
                            if let Some(etag) = info.new_etag {
                                state_mut.etag = Some(etag);
                            }
                            if let Some(lm) = info.new_last_modified {
                                state_mut.last_modified = Some(lm);
                            }
                            state_mut.file_path = Some(new_path.to_string_lossy().to_string());
                            state_mut.download_bytes = Some(downloaded_bytes);
                            cache_app_metadata(
                                state_mut,
                                capabilities,
                                segmented_downloads,
                                forge_repository,
                                forge_platform,
                            );

                            results.push(UpdateApp {
                                name: app_name.clone(),
                                status: UpdateStatus::Updated,
                                from_version,
                                to_version: Some(to_version.clone()),
                                path: Some(new_path.to_string_lossy().to_string()),
                                rate_limited_until: None,
                                duration_seconds: Some(duration_seconds),
                                error: None,
                            });
                            if !json_output && !progress_completion_rendered {
                                let action = if was_update {
                                    "updated to"
                                } else {
                                    "downloaded"
                                };
                                deferred_status_messages.push(format!(
                                    "{} {} {} in {:.2}s",
                                    app_name, action, to_version, duration_seconds
                                ));
                            }
                        }

                        total_downloaded_bytes =
                            total_downloaded_bytes.saturating_add(downloaded_bytes);
                        if let Some(download_elapsed) = download_elapsed
                            && !download_elapsed.is_zero()
                        {
                            let speed = downloaded_bytes as f64 / download_elapsed.as_secs_f64().max(0.001);
                            download_speed_samples.push(speed);
                            peak_download_bps = Some(peak_download_bps.map_or(speed, |peak| peak.max(speed)));
                        }

                        download_limit = if let Some(download_bps) = download_bps {
                            adapt_download_limit(
                                download_limit,
                                downloaded_bytes,
                                Some(download_bps),
                                peak_download_bps,
                                pending_downloads.len(),
                                hard_max_download,
                            )
                        } else {
                            adapt_download_limit(
                                download_limit,
                                downloaded_bytes,
                                None,
                                peak_download_bps,
                                pending_downloads.len(),
                                hard_max_download,
                            )
                        };
                    }
                    UpdateWorkResult::Error {
                        stage,
                        name,
                        from_version,
                        to_version,
                        path,
                        elapsed: _,
                        capabilities,
                        segmented_downloads,
                        rate_limited_until,
                        error,
                        forge_repository,
                        forge_platform,
                        retry_job,
                    } => {
                        if let Some(retry_job) = retry_job {
                            retry_downloads.push(retry_job);
                            continue;
                        }
                        let state = state_manager.get_app_mut(&name);
                        cache_app_metadata(
                            state,
                            capabilities,
                            segmented_downloads,
                            forge_repository,
                            forge_platform,
                        );
                        state.rate_limited_until = rate_limited_until;
                        results.push(UpdateApp {
                            name: name.clone(),
                            status: UpdateStatus::Error,
                            from_version,
                            to_version,
                            path,
                            rate_limited_until,
                            duration_seconds: None,
                            error: Some(error.clone()),
                        });
                        if !json_output {
                            match stage {
                                UpdateErrorStage::Check => deferred_warning_messages.push(format!(
                                    "Error checking updates for {}: {}",
                                    name, error
                                )),
                                UpdateErrorStage::Download => deferred_warning_messages
                                    .push(format!("Download failed for {}: {}", name, error)),
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        while active_checks < check_limit {
            if let Some((app, state, github_proxy, github_proxy_prefixes)) = pending_checks.next() {
                spawn_check_worker(
                    app,
                    state,
                    client.clone(),
                    github_proxy,
                    github_proxy_prefixes,
                    global_config.clone(),
                    tx.clone(),
                );
                active_checks += 1;
            } else {
                break;
            }
        }

        spawn_pending_downloads(
            &mut pending_downloads,
            &mut active_downloads,
            download_limit,
            &mut download_scheduler,
            &tx,
            global_config,
            &storage_dir,
            client,
            json_output,
            color_output,
        );
    }

    if !json_output {
        downloader::finalize_progress_output()?;
    }

    for parse_error in app_config_errors {
        if !matches_target(app_name, parse_error.app_name.as_deref()) {
            continue;
        }
        found = true;
        let parse_error_message = format!(
            "Failed to parse app config at {}: {}",
            parse_error.path.display(),
            parse_error.message
        );
        results.push(UpdateApp {
            name: parse_error
                .app_name
                .clone()
                .unwrap_or_else(|| parse_error.path.display().to_string()),
            status: UpdateStatus::Error,
            from_version: None,
            to_version: None,
            path: None,
            rate_limited_until: None,
            duration_seconds: None,
            error: Some(parse_error_message.clone()),
        });
        if !json_output {
            deferred_warning_messages.push(parse_error_message);
        }
    }

    if !json_output {
        for message in deferred_status_messages {
            print_success(&message, color_output);
        }
        for message in deferred_warning_messages {
            print_warning(&message, color_output);
        }
    }

    if json_output {
        let error = if let Some(target) = app_name
            && !found
        {
            Some(format!("App '{}' not found in configuration.", target))
        } else {
            None
        };
        print_json(&UpdateResponse {
            command: "update",
            apps: results,
            total_downloaded_bytes: Some(total_downloaded_bytes),
            median_download_speed_bps: median_speed_bps(&download_speed_samples),
            error,
        })?;
    } else {
        let updated = results
            .iter()
            .filter(|app| matches!(app.status, UpdateStatus::Updated))
            .count();
        let current = results
            .iter()
            .filter(|app| matches!(app.status, UpdateStatus::UpToDate))
            .count();
        let rate_limited = results
            .iter()
            .filter(|app| matches!(app.status, UpdateStatus::RateLimited))
            .count();
        let failed = results
            .iter()
            .filter(|app| matches!(app.status, UpdateStatus::Error))
            .count();
        let median_download_speed = median_speed_bps(&download_speed_samples);
        print_progress(
            &format!(
                "summary: {} updated, {} current, {} rate limited, {} failed, {} total downloaded, median speed {}",
                updated,
                current,
                rate_limited,
                failed,
                downloader::human_bytes_precise(total_downloaded_bytes as f64),
                median_download_speed
                    .map(downloader::human_rate)
                    .unwrap_or_else(|| "n/a".to_string())
            ),
            color_output,
        );
        if rate_limit_note_needed {
            print_warning(rate_limit_note(), color_output);
        }
    }
    Ok(())
}
