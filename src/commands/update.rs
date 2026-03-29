use crate::commands::helpers::*;
use crate::config;
use crate::downloader;
use crate::integrator;
use crate::output::{
    UpdateApp, UpdateResponse, UpdateStatus, format_rate_limit_retry_text, print_json,
    print_progress, print_success, print_warning,
};
use crate::parser::AppConfigLoadError;
use crate::resolvers;
use crate::state::{AppState, StateManager};
use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub enum UpdateWorkResult {
    ReadyToDownload {
        app: config::AppConfig,
        state: AppState,
        from_version: Option<String>,
        current_path: Option<String>,
        info: resolvers::UpdateInfo,
        elapsed: Duration,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
    },
    Updated {
        app: config::AppConfig,
        from_version: Option<String>,
        info: resolvers::UpdateInfo,
        new_path: std::path::PathBuf,
        old_path: Option<std::path::PathBuf>,
        elapsed: Duration,
        downloaded_bytes: u64,
        download_elapsed: Option<Duration>,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
        progress_completion_rendered: bool,
    },
    UpToDate {
        name: String,
        from_version: Option<String>,
        path: Option<String>,
        elapsed: Duration,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
    },
    RateLimited {
        name: String,
        from_version: Option<String>,
        path: Option<String>,
        elapsed: Duration,
        rate_limited_until: Option<u64>,
    },
    Error {
        stage: UpdateErrorStage,
        name: String,
        from_version: Option<String>,
        to_version: Option<String>,
        path: Option<String>,
        elapsed: Duration,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
        rate_limited_until: Option<u64>,
        error: String,
    },
}

pub enum UpdateEvent {
    Check(UpdateWorkResult),
    Download {
        provider: String,
        result: UpdateWorkResult,
    },
}

pub struct UpdateDownloadJob {
    pub app: config::AppConfig,
    pub state: AppState,
    pub from_version: Option<String>,
    pub current_path: Option<String>,
    pub info: resolvers::UpdateInfo,
    pub capabilities: Vec<String>,
    pub segmented_downloads: Option<bool>,
    pub provider: String,
}

pub struct ProviderDownloadScheduler {
    active_global: usize,
    active_by_provider: HashMap<String, usize>,
}

impl ProviderDownloadScheduler {
    pub fn new() -> Self {
        Self {
            active_global: 0,
            active_by_provider: HashMap::new(),
        }
    }

    fn provider_limit(provider: &str) -> usize {
        if provider == "github" { 3 } else { 2 }
    }

    pub fn try_acquire(&mut self, provider: &str, global_limit: usize) -> bool {
        if self.active_global >= global_limit {
            return false;
        }

        let provider_limit = Self::provider_limit(provider);
        let active_for_provider = *self.active_by_provider.get(provider).unwrap_or(&0);
        if active_for_provider >= provider_limit {
            return false;
        }

        self.active_global += 1;
        *self
            .active_by_provider
            .entry(provider.to_string())
            .or_insert(0) += 1;
        true
    }

    pub fn release(&mut self, provider: &str) {
        if self.active_global > 0 {
            self.active_global -= 1;
        }
        if let Some(active_for_provider) = self.active_by_provider.get_mut(provider) {
            if *active_for_provider > 1 {
                *active_for_provider -= 1;
            } else {
                self.active_by_provider.remove(provider);
            }
        }
    }
}

pub enum UpdateErrorStage {
    Check,
    Download,
}

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

    let (tx, rx) = mpsc::channel::<UpdateEvent>();
    let mut pending_checks = pending_checks.into_iter().peekable();
    let mut pending_downloads: VecDeque<UpdateDownloadJob> = VecDeque::new();
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

    let spawn_pending_downloads = |pending_downloads: &mut VecDeque<UpdateDownloadJob>,
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
        let pending_len = pending_downloads.len();
        while *active_downloads < download_limit
            && !pending_downloads.is_empty()
            && rotations < pending_len
        {
            let Some(job) = pending_downloads.pop_front() else {
                break;
            };

            if download_scheduler.try_acquire(&job.provider, download_limit) {
                let storage_dir = storage_dir.to_path_buf();
                let naming_format = global_config.naming_format.clone();
                let segmented_downloads = job
                    .segmented_downloads
                    .or(job.app.segmented_downloads)
                    .unwrap_or(global_config.segmented_downloads);
                spawn_download_worker(
                    job,
                    client.clone(),
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
                pending_downloads.push_back(job);
                rotations += 1;
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

    while active_checks > 0 || active_downloads > 0 || !pending_downloads.is_empty() {
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
                    } => {
                        let provider = download_provider_key(&info.download_url);
                        pending_downloads.push_back(UpdateDownloadJob {
                            app,
                            state,
                            from_version,
                            current_path,
                            info,
                            capabilities,
                            segmented_downloads,
                            provider,
                        });
                    }
                    UpdateWorkResult::UpToDate {
                        name,
                        from_version,
                        path,
                        elapsed: _,
                        capabilities,
                        segmented_downloads,
                    } => {
                        let state = state_manager.get_app_mut(&name);
                        cache_app_metadata(state, capabilities, segmented_downloads);
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
                    } => {
                        let state = state_manager.get_app_mut(&name);
                        cache_app_metadata(state, capabilities, segmented_downloads);
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
                            cache_app_metadata(state, capabilities, segmented_downloads);
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
                            cache_app_metadata(state_mut, capabilities, segmented_downloads);

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

                        download_limit = if let Some(download_bps) = download_bps {
                            adapt_worker_limit_for_speed(
                                download_limit,
                                Some(download_bps),
                                pending_downloads.len(),
                                hard_max_download,
                            )
                        } else {
                            adapt_worker_limit(
                                download_limit,
                                elapsed,
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
                    } => {
                        let state = state_manager.get_app_mut(&name);
                        cache_app_metadata(state, capabilities, segmented_downloads);
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
        print_progress(
            &format!(
                "summary: {} updated, {} current, {} rate limited, {} failed",
                updated, current, rate_limited, failed
            ),
            color_output,
        );
        if rate_limit_note_needed {
            print_warning(rate_limit_note(), color_output);
        }
    }
    Ok(())
}

fn process_update_check_job(
    client: &ureq::Agent,
    app: config::AppConfig,
    state: Option<AppState>,
    github_proxy: bool,
    github_proxy_prefixes: Vec<String>,
    global_config: &config::GlobalConfig,
) -> UpdateWorkResult {
    let started_at = Instant::now();
    let app_name = app.name.clone();
    let from_version = state.as_ref().and_then(|s| s.local_version.clone());
    let current_path = state.as_ref().and_then(|s| s.file_path.clone());

    match resolvers::check_for_updates(
        &app,
        state.as_ref(),
        client,
        github_proxy,
        &github_proxy_prefixes,
        global_config,
    ) {
        Ok(result) => {
            let capabilities = result.capabilities;
            let segmented_support = result.segmented_downloads;
            let Some(info) = result.update else {
                return UpdateWorkResult::UpToDate {
                    name: app_name,
                    from_version,
                    path: current_path,
                    elapsed: started_at.elapsed(),
                    capabilities,
                    segmented_downloads: segmented_support,
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
                }
            }
        }
    }
}

fn process_update_download_job(
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
        provider: _,
    } = job;
    let app_name = app.name.clone();
    let to_version = info.version.clone();

    match downloader::download_app(
        client,
        &app,
        &info,
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
            info,
            new_path: download_result.path,
            old_path: current_path.map(std::path::PathBuf::from),
            elapsed: started_at.elapsed(),
            downloaded_bytes: download_result.downloaded_bytes,
            download_elapsed: download_result.download_elapsed,
            capabilities,
            segmented_downloads: download_result.segmented_downloads.or(segmented_support),
            progress_completion_rendered: download_result.progress_completion_rendered,
        },
        Err(e) => UpdateWorkResult::Error {
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
        },
    }
}

pub fn adapt_worker_limit_for_speed(
    current: usize,
    download_bps: Option<f64>,
    pending: usize,
    hard_max: usize,
) -> usize {
    let mut next = current;

    if let Some(bps) = download_bps {
        if bps >= FAST_DOWNLOAD_BPS && pending > current && next < hard_max {
            next += 1;
        } else if bps <= SLOW_DOWNLOAD_BPS && next > 1 {
            next -= 1;
        }
    }

    next.clamp(1, hard_max)
}

pub fn update_work_elapsed(result: &UpdateWorkResult) -> Duration {
    match result {
        UpdateWorkResult::ReadyToDownload { elapsed, .. } => *elapsed,
        UpdateWorkResult::Updated { elapsed, .. }
        | UpdateWorkResult::UpToDate { elapsed, .. }
        | UpdateWorkResult::Error { elapsed, .. }
        | UpdateWorkResult::RateLimited { elapsed, .. } => *elapsed,
    }
}

pub fn download_provider_key(url: &str) -> String {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();

    if host.ends_with("github.com") || host.ends_with("githubusercontent.com") {
        "github".to_string()
    } else {
        host
    }
}

const FAST_DOWNLOAD_BPS: f64 = 40.0 * 1024.0 * 1024.0;
const SLOW_DOWNLOAD_BPS: f64 = 10.0 * 1024.0 * 1024.0;
