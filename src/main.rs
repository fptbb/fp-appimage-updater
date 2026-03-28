use anyhow::Result;
use clap::{CommandFactory, Parser};
use serde::Serialize;
use std::time::Duration;

mod cli;
mod config;
mod disintegrator;
mod doctor;
mod downloader;
mod initializer;
mod integrator;
mod lock;
mod output;
mod parser;
mod resolvers;
mod self_updater;
mod state;
mod validator;

use cli::{Cli, Commands};
use std::collections::{HashMap, VecDeque};
use output::{
    CheckApp, CheckResponse, CheckStatus, DoctorCheck, DoctorResponse, DoctorStatus, ListApp,
    ListResponse, RemoveApp, RemoveResponse, RemoveStatus, UpdateApp, UpdateResponse, UpdateStatus,
    ValidateApp, ValidateResponse, ValidateStatus, colors_enabled, format_rate_limit_retry_text,
    print_check_human,
    print_doctor_human, print_json, print_list_human, print_progress, print_success,
    print_validate_human, print_warning,
};
use parser::ConfigPaths;
use state::StateManager;
use std::time::Instant;
use std::sync::mpsc;

const MAX_CONCURRENT_CHECK_JOBS: usize = 3;
const MAX_CONCURRENT_DOWNLOADS: usize = 6;
const FAST_JOB_SECONDS: f64 = 2.0;
const SLOW_JOB_SECONDS: f64 = 15.0;
const FAST_DOWNLOAD_BPS: f64 = 40.0 * 1024.0 * 1024.0;
const SLOW_DOWNLOAD_BPS: f64 = 10.0 * 1024.0 * 1024.0;

fn build_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent("fp-appimage-updater/1.0")
        .build()
        .into()
}

enum UpdateWorkResult {
    ReadyToDownload {
        app: config::AppConfig,
        state: state::AppState,
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

enum UpdateEvent {
    Check(UpdateWorkResult),
    Download {
        provider: String,
        result: UpdateWorkResult,
    },
}

struct UpdateDownloadJob {
    app: config::AppConfig,
    state: state::AppState,
    from_version: Option<String>,
    current_path: Option<String>,
    info: resolvers::UpdateInfo,
    capabilities: Vec<String>,
    segmented_downloads: Option<bool>,
    provider: String,
}

struct ProviderDownloadScheduler {
    active_global: usize,
    active_by_provider: HashMap<String, usize>,
}

impl ProviderDownloadScheduler {
    fn new() -> Self {
        Self {
            active_global: 0,
            active_by_provider: HashMap::new(),
        }
    }

    fn provider_limit(provider: &str) -> usize {
        if provider == "github" {
            3
        } else {
            2
        }
    }

    fn try_acquire(&mut self, provider: &str, global_limit: usize) -> bool {
        if self.active_global >= global_limit {
            return false;
        }

        let provider_limit = Self::provider_limit(provider);
        let active_for_provider = *self.active_by_provider.get(provider).unwrap_or(&0);
        if active_for_provider >= provider_limit {
            return false;
        }

        self.active_global += 1;
        *self.active_by_provider.entry(provider.to_string()).or_insert(0) += 1;
        true
    }

    fn release(&mut self, provider: &str) {
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

enum UpdateErrorStage {
    Check,
    Download,
}

enum CheckWorkResult {
    Ok {
        app: CheckApp,
        elapsed: Duration,
        cache_capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
    },
    RateLimited {
        app: CheckApp,
        elapsed: Duration,
        rate_limited_until: Option<u64>,
    },
    Err {
        app: CheckApp,
        elapsed: Duration,
    },
}

fn cache_app_metadata(
    state: &mut state::AppState,
    capabilities: Vec<String>,
    segmented_downloads: Option<bool>,
) {
    let mut capabilities = capabilities;
    resolvers::dedupe_capabilities(&mut capabilities);
    state.capabilities = capabilities;
    if let Some(segmented_downloads) = segmented_downloads {
        state.segmented_downloads = Some(segmented_downloads);
    }
}

fn adapt_worker_limit(
    current: usize,
    elapsed: Duration,
    pending: usize,
    hard_max: usize,
) -> usize {
    let mut next = current;
    let elapsed_secs = elapsed.as_secs_f64();

    if elapsed_secs <= FAST_JOB_SECONDS && pending > current && next < hard_max {
        next += 1;
    } else if elapsed_secs >= SLOW_JOB_SECONDS && next > 1 {
        next -= 1;
    }

    next.clamp(1, hard_max)
}

fn adapt_worker_limit_for_speed(
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

fn update_work_elapsed(result: &UpdateWorkResult) -> Duration {
    match result {
        UpdateWorkResult::ReadyToDownload { elapsed, .. } => *elapsed,
        UpdateWorkResult::Updated { elapsed, .. }
        | UpdateWorkResult::UpToDate { elapsed, .. }
        | UpdateWorkResult::Error { elapsed, .. }
        | UpdateWorkResult::RateLimited { elapsed, .. } => *elapsed,
    }
}

fn download_provider_key(url: &str) -> String {
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

fn now_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rate_limit_enabled(app: &config::AppConfig, global: &config::GlobalConfig) -> bool {
    app.respect_rate_limits.unwrap_or(global.respect_rate_limits)
}

fn github_proxy_enabled(app: &config::AppConfig, global: &config::GlobalConfig) -> bool {
    app.github_proxy.unwrap_or(global.github_proxy)
}

fn github_proxy_prefix(app: &config::AppConfig, global: &config::GlobalConfig) -> String {
    app.github_proxy_prefix
        .clone()
        .unwrap_or_else(|| global.github_proxy_prefix.clone())
}

fn app_uses_github_forge(app: &config::AppConfig) -> bool {
    matches!(
        &app.strategy,
        config::StrategyConfig::Forge { repository, .. } if repository.starts_with("https://github.com/")
    )
}

fn clear_expired_rate_limit(state: &mut state::AppState, now: u64) {
    if matches!(state.rate_limited_until, Some(until) if until <= now) {
        state.rate_limited_until = None;
    }
}

fn snapshot_app_state(
    state_manager: &mut StateManager,
    app_name: &str,
    now: u64,
) -> state::AppState {
    let state = state_manager.get_app_mut(app_name);
    clear_expired_rate_limit(state, now);
    state.clone()
}

fn rate_limit_note() -> &'static str {
    "Rate limit hit. Set respect_rate_limits: false globally or per app to keep trying."
}

fn rate_limit_from_error(error: &anyhow::Error) -> Option<resolvers::RateLimitInfo> {
    error.downcast_ref::<resolvers::RateLimitInfo>().cloned()
}

fn process_check_job(
    app: config::AppConfig,
    state: Option<state::AppState>,
    client: &ureq::Agent,
    github_proxy: bool,
    github_proxy_prefix: String,
) -> CheckWorkResult {
    let started_at = Instant::now();
    let local_version = state.as_ref().and_then(|s| s.local_version.clone());

    match resolvers::check_for_updates(
        &app,
        state.as_ref(),
        client,
        github_proxy,
        &github_proxy_prefix,
    ) {
        Ok(result) => {
            let mut capabilities = result.capabilities;
            let cache_capabilities = capabilities.clone();
            if matches!(
                app.zsync,
                Some(config::ZsyncConfig::Enabled(true)) | Some(config::ZsyncConfig::Url(_))
            ) {
                capabilities.push("zsync".to_string());
            }
            resolvers::dedupe_capabilities(&mut capabilities);
            let elapsed = started_at.elapsed();

            match result.update {
                Some(info) => CheckWorkResult::Ok {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::UpdateAvailable,
                        local_version,
                        remote_version: Some(info.version),
                        download_url: Some(info.download_url),
                        rate_limited_until: None,
                        capabilities,
                        error: None,
                    },
                    elapsed,
                    cache_capabilities,
                    segmented_downloads: result.segmented_downloads,
                },
                None => CheckWorkResult::Ok {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::UpToDate,
                        local_version,
                        remote_version: None,
                        download_url: None,
                        rate_limited_until: None,
                        capabilities,
                        error: None,
                    },
                    elapsed,
                    cache_capabilities,
                    segmented_downloads: result.segmented_downloads,
                },
            }
        }
        Err(e) => {
            let elapsed = started_at.elapsed();
            let rate_limited_until =
                rate_limit_from_error(&e).and_then(|info| info.until_epoch_seconds());

            if rate_limited_until.is_some() {
                CheckWorkResult::RateLimited {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::RateLimited,
                        local_version,
                        remote_version: None,
                        download_url: None,
                        rate_limited_until,
                        capabilities: Vec::new(),
                        error: None,
                    },
                    elapsed,
                    rate_limited_until,
                }
            } else {
                CheckWorkResult::Err {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::Error,
                        local_version,
                        remote_version: None,
                        download_url: None,
                        rate_limited_until: None,
                        capabilities: Vec::new(),
                        error: Some(format!("{:#}", e)),
                    },
                    elapsed,
                }
            }
        }
    }
}

fn process_update_check_job(
    client: &ureq::Agent,
    app: config::AppConfig,
    state: Option<state::AppState>,
    github_proxy: bool,
    github_proxy_prefix: String,
) -> UpdateWorkResult {
    let started_at = Instant::now();
    let app_name = app.name.clone();
    let from_version = state.as_ref().and_then(|s| s.local_version.clone());
    let current_path = state.as_ref().and_then(|s| s.file_path.clone());

    match resolvers::check_for_updates(
        &app,
        state.as_ref(),
        &client,
        github_proxy,
        &github_proxy_prefix,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let json_output = cli.json;

    let paths = if let Some(config_dir) = cli.config.clone() {
        ConfigPaths::with_config_dir(config_dir)?
    } else {
        ConfigPaths::new()?
    };
    let _process_lock = lock::FileLock::acquire(paths.lock_path())?;
    let color_output = colors_enabled(json_output);

    if let Commands::Init {
        global,
        app,
        strategy,
        force,
    } = &cli.command
    {
        let result = initializer::run(&paths, *global, app.as_deref(), *strategy, *force)?;

        if json_output {
            #[derive(Serialize)]
            struct InitResponse {
                command: &'static str,
                created: Vec<String>,
                skipped: Vec<String>,
            }
            print_json(&InitResponse {
                command: "init",
                created: result
                    .created
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
                skipped: result
                    .skipped
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect(),
            })?;
        } else {
            for path in &result.created {
                print_success(&format!("Created {}", path.display()), color_output);
                print_progress(&format!("Edit: {}", path.display()), color_output);
                print_progress("Then run: fp-appimage-updater validate", color_output);
            }
            for path in &result.skipped {
                print_warning(
                    &format!(
                        "Skipped existing file {} (use --force to overwrite)",
                        path.display()
                    ),
                    color_output,
                );
            }
            if result.created.is_empty() && result.skipped.is_empty() {
                print_progress("Nothing to initialize.", color_output);
            }
        }
        return Ok(());
    }

    let global_config = parser::load_global_config(&paths)?;
    let app_load = parser::load_app_configs(&paths)?;
    let app_configs = app_load.apps;
    let app_config_errors = app_load.errors;
    let mut state_manager = StateManager::load(paths.cache_path());

    let client = build_http_agent();

    match &cli.command {
        Commands::Init { .. } => unreachable!("init handled before config loading"),
        Commands::Doctor => {
            let checks = doctor::run(&paths, app_configs.len(), app_config_errors.len())
                .into_iter()
                .map(|check| DoctorCheck {
                    name: check.name,
                    status: match check.status {
                        doctor::DoctorStatus::Ok => DoctorStatus::Ok,
                        doctor::DoctorStatus::Warn => DoctorStatus::Warn,
                    },
                    detail: check.detail,
                })
                .collect::<Vec<_>>();

            if json_output {
                print_json(&DoctorResponse {
                    command: "doctor",
                    checks,
                })?;
            } else {
                print_doctor_human(&checks, color_output);
                if !app_config_errors.is_empty() {
                    print_progress(
                        "Tip: run `fp-appimage-updater validate` for detailed parse errors.",
                        color_output,
                    );
                }
            }
        }
        Commands::Validate { app_name } => {
            let (apps, error) = validator::validate_app_configs(&paths, app_name.as_deref())?;
            let results = apps
                .into_iter()
                .map(|app| ValidateApp {
                    name: app.app_name,
                    file: app.file,
                    status: match app.status {
                        validator::ValidationStatus::Valid => ValidateStatus::Valid,
                        validator::ValidationStatus::Invalid => ValidateStatus::Invalid,
                    },
                    error: app.error,
                })
                .collect::<Vec<_>>();

            if json_output {
                print_json(&ValidateResponse {
                    command: "validate",
                    apps: results,
                    error,
                })?;
            } else {
                print_validate_human(&results, error.as_deref(), color_output);
            }
        }
        Commands::List => {
            let apps = app_configs
                .iter()
                .map(|app| {
                    let state = state_manager.get_app(&app.name);
                    ListApp {
                        name: app.name.clone(),
                        strategy: strategy_label(&app.strategy).to_string(),
                        local_version: state.and_then(|s| s.local_version.clone()),
                        integration: app
                            .integration
                            .unwrap_or(global_config.manage_desktop_files),
                        symlink: app.create_symlink.unwrap_or(global_config.create_symlinks),
                    }
                })
                .collect::<Vec<_>>();

            if json_output {
                print_json(&ListResponse {
                    command: "list",
                    apps,
                })?;
            } else {
                print_list_human(&apps, color_output);
            }
        }
        Commands::Check { app_name } => {
            let mut found = false;
            let mut results = Vec::new();
            let mut check_jobs = Vec::new();
            let mut rate_limit_note_needed = false;
            let now = now_epoch_seconds();

            for app in &app_configs {
                if let Some(target) = &app_name
                    && app.name != *target
                {
                    continue;
                }
                found = true;
                let state = snapshot_app_state(&mut state_manager, &app.name, now);
                let rate_limited_until = state.rate_limited_until;
                let github_proxy = github_proxy_enabled(app, &global_config);
                if rate_limit_enabled(app, &global_config)
                    && !(github_proxy && app_uses_github_forge(app))
                    && matches!(rate_limited_until, Some(until) if until > now)
                {
                    rate_limit_note_needed = true;
                    results.push(CheckApp {
                        name: app.name.clone(),
                        status: CheckStatus::RateLimited,
                        local_version: state.local_version.clone(),
                        remote_version: None,
                        download_url: None,
                        rate_limited_until,
                        capabilities: state.capabilities.clone(),
                        error: None,
                    });
                    continue;
                }
                check_jobs.push((app.clone(), Some(state)));
            }

            let hard_max = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
                .min(MAX_CONCURRENT_CHECK_JOBS)
                .max(1);
            let mut worker_limit = hard_max;

            let (tx, rx) = mpsc::channel();
            let mut pending = check_jobs.into_iter();
            let mut active = 0usize;

            let spawn_check_worker = |app: config::AppConfig,
                                      state: Option<state::AppState>,
                                      client: ureq::Agent,
                                      github_proxy: bool,
                                      github_proxy_prefix: String,
                                      tx: mpsc::Sender<CheckWorkResult>| {
                std::thread::spawn(move || {
                    let _ = tx.send(process_check_job(
                        app,
                        state,
                        &client,
                        github_proxy,
                        github_proxy_prefix,
                    ));
                });
            };

            while active < worker_limit {
                if let Some((app, state)) = pending.next() {
                    let github_proxy = github_proxy_enabled(&app, &global_config);
                    let github_proxy_prefix = github_proxy_prefix(&app, &global_config);
                    spawn_check_worker(
                        app,
                        state,
                        client.clone(),
                        github_proxy,
                        github_proxy_prefix,
                        tx.clone(),
                    );
                    active += 1;
                } else {
                    break;
                }
            }

            while active > 0 {
                let result = rx.recv().expect("check worker panicked");
                active -= 1;

                match result {
                    CheckWorkResult::Ok {
                        app: app_result,
                        elapsed,
                        cache_capabilities,
                        segmented_downloads,
                    } => {
                        let state = state_manager.get_app_mut(&app_result.name);
                        cache_app_metadata(state, cache_capabilities, segmented_downloads);
                        results.push(app_result);
                        worker_limit =
                            adapt_worker_limit(worker_limit, elapsed, pending.len(), hard_max);
                    }
                    CheckWorkResult::RateLimited {
                        app: app_result,
                        elapsed,
                        rate_limited_until,
                    } => {
                        let state = state_manager.get_app_mut(&app_result.name);
                        if let Some(until) = rate_limited_until {
                            state.rate_limited_until = Some(until);
                        }
                        results.push(app_result);
                        rate_limit_note_needed = true;
                        worker_limit =
                            adapt_worker_limit(worker_limit, elapsed, pending.len(), hard_max);
                    }
                    CheckWorkResult::Err {
                        app: app_result,
                        elapsed,
                    } => {
                        results.push(app_result);
                        worker_limit =
                            adapt_worker_limit(worker_limit, elapsed, pending.len(), hard_max);
                    }
                }

                if let Some((app, state)) = pending.next() {
                    let github_proxy = github_proxy_enabled(&app, &global_config);
                    let github_proxy_prefix = github_proxy_prefix(&app, &global_config);
                    spawn_check_worker(
                        app,
                        state,
                        client.clone(),
                        github_proxy,
                        github_proxy_prefix,
                        tx.clone(),
                    );
                    active += 1;
                }
            }

            for parse_error in &app_config_errors {
                if !matches_target(app_name.as_deref(), parse_error.app_name.as_deref()) {
                    continue;
                }
                found = true;
                results.push(CheckApp {
                    name: parse_error
                        .app_name
                        .clone()
                        .unwrap_or_else(|| parse_error.path.display().to_string()),
                    status: CheckStatus::Error,
                    local_version: None,
                    remote_version: None,
                    download_url: None,
                    rate_limited_until: None,
                    capabilities: Vec::new(),
                    error: Some(format!(
                        "Failed to parse app config at {}: {}",
                        parse_error.path.display(),
                        parse_error.message
                    )),
                });
            }

            let error = if let Some(target) = app_name
                && !found
            {
                Some(format!("App '{}' not found in configuration.", target))
            } else {
                None
            };

            if json_output {
                print_json(&CheckResponse {
                    command: "check",
                    apps: results,
                    error,
                })?;
            } else {
                print_check_human(
                    &results,
                    error.as_deref(),
                    if rate_limit_note_needed {
                        Some(rate_limit_note())
                    } else {
                        None
                    },
                    color_output,
                );
            }
            state_manager.save()?;
        }
        Commands::Update { app_name } => {
            let storage_dir = integrator::expand_tilde(&global_config.storage_dir);
            let mut results = Vec::new();
            let mut found = false;
            let mut rate_limit_note_needed = false;
            let mut deferred_status_messages = Vec::new();
            let mut deferred_warning_messages = Vec::new();
            let now = now_epoch_seconds();
            let mut pending_checks = Vec::new();

            for app in &app_configs {
                if let Some(target) = app_name
                    && app.name != *target
                {
                    continue;
                }
                found = true;
                let state = snapshot_app_state(&mut state_manager, &app.name, now);
                let rate_limited_until = state.rate_limited_until;
                let github_proxy = github_proxy_enabled(app, &global_config);
                if rate_limit_enabled(app, &global_config)
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
                let github_proxy_prefix = github_proxy_prefix(app, &global_config);
                pending_checks.push((app.clone(), Some(state), github_proxy, github_proxy_prefix));
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
                                      state: Option<state::AppState>,
                                      client: ureq::Agent,
                                      github_proxy: bool,
                                      github_proxy_prefix: String,
                                      tx: mpsc::Sender<UpdateEvent>| {
                std::thread::spawn(move || {
                    let _ = tx.send(UpdateEvent::Check(process_update_check_job(
                        &client,
                        app,
                        state,
                        github_proxy,
                        github_proxy_prefix,
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
                                           tx: &mpsc::Sender<UpdateEvent>| {
                if pending_downloads.is_empty() {
                    return;
                }

                let mut rotations = 0usize;
                let pending_len = pending_downloads.len();
                while *active_downloads < download_limit && !pending_downloads.is_empty() && rotations < pending_len {
                    let Some(job) = pending_downloads.pop_front() else {
                        break;
                    };

                    if download_scheduler.try_acquire(&job.provider, download_limit) {
                        let storage_dir = storage_dir.clone();
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
                if let Some((app, state, github_proxy, github_proxy_prefix)) = pending_checks.next()
                {
                    spawn_check_worker(
                        app,
                        state,
                        client.clone(),
                        github_proxy,
                        github_proxy_prefix,
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
                                        UpdateErrorStage::Check => deferred_warning_messages
                                            .push(format!("Error checking updates for {}: {}", name, error)),
                                        UpdateErrorStage::Download => deferred_warning_messages
                                            .push(format!("Download failed for {}: {}", name, error)),
                                    }
                                }
                            }
                            _ => {}
                        }

                        check_limit = adapt_worker_limit(
                            check_limit,
                            elapsed,
                            pending_checks.len(),
                            hard_max_check,
                        );
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
                                        downloaded_bytes as f64
                                            / download_elapsed.as_secs_f64().max(0.001)
                                    }
                                });

                                if let Err(e) =
                                    integrator::integrate(&app, &global_config, &new_path, old_path_ref)
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
                                        deferred_warning_messages.push(format!(
                                            "Integration failed for {}: {:#}",
                                            app_name, e
                                        ));
                                        deferred_warning_messages.push(format!(
                                            "Rolling back {} to its previous state...",
                                            app_name
                                        ));
                                    }

                                    integrator::rollback(&app, &global_config, &new_path, old_path_ref);
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
                                        UpdateErrorStage::Check => deferred_warning_messages
                                            .push(format!("Error checking updates for {}: {}", name, error)),
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
                    if let Some((app, state, github_proxy, github_proxy_prefix)) =
                        pending_checks.next()
                    {
                        spawn_check_worker(
                            app,
                            state,
                            client.clone(),
                            github_proxy,
                            github_proxy_prefix,
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
                );
            }

            if !json_output {
                downloader::finalize_progress_output()?;
            }

            for parse_error in &app_config_errors {
                if !matches_target(app_name.as_deref(), parse_error.app_name.as_deref()) {
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
            state_manager.save()?;
        }
        Commands::Remove { app_name, all } => {
            let mut found = false;
            let mut apps_to_remove = Vec::new();
            let mut results = Vec::new();

            if *all {
                for app in &app_configs {
                    apps_to_remove.push(app.name.clone());
                }
            } else if let Some(target) = app_name {
                apps_to_remove.push(target.clone());
            } else {
                if json_output {
                    print_json(&RemoveResponse {
                        command: "remove",
                        apps: Vec::new(),
                        error: Some(
                            "Please provide an application name to remove, or use --all."
                                .to_string(),
                        ),
                    })?;
                } else {
                    print_warning(
                        "Error: Please provide an application name to remove, or use --all.",
                        color_output,
                    );
                }
                return Ok(());
            }

            for target_name in apps_to_remove {
                for app in &app_configs {
                    if app.name == target_name {
                        found = true;
                        let state = state_manager.get_app(&app.name);

                        if let Err(e) = disintegrator::remove_app(
                            app,
                            &global_config,
                            state,
                            json_output,
                            color_output,
                        ) {
                            if json_output {
                                results.push(RemoveApp {
                                    name: app.name.clone(),
                                    status: RemoveStatus::Error,
                                    error: Some(format!("{:#}", e)),
                                });
                            } else {
                                print_warning(
                                    &format!("Error removing {}: {:#}", app.name, e),
                                    color_output,
                                );
                            }
                        } else {
                            state_manager.state.apps.remove(&app.name);
                            if json_output {
                                results.push(RemoveApp {
                                    name: app.name.clone(),
                                    status: RemoveStatus::Removed,
                                    error: None,
                                });
                            }
                        }
                        break;
                    }
                }
                if !app_configs.iter().any(|app| app.name == target_name) && json_output {
                    results.push(RemoveApp {
                        name: target_name,
                        status: RemoveStatus::NotFound,
                        error: Some("App not found in configuration.".to_string()),
                    });
                }
            }

            if json_output {
                let error = if !found && !*all {
                    app_name
                        .as_ref()
                        .map(|target| format!("App '{}' not found in configuration.", target))
                } else {
                    None
                };
                print_json(&RemoveResponse {
                    command: "remove",
                    apps: results,
                    error,
                })?;
                state_manager.save()?;
            } else if !found && !*all {
                print_warning(
                    &format!("App '{:?}' not found in configuration.", app_name),
                    color_output,
                );
            } else {
                let removed = results
                    .iter()
                    .filter(|app| matches!(app.status, RemoveStatus::Removed))
                    .count();
                let missing = results
                    .iter()
                    .filter(|app| matches!(app.status, RemoveStatus::NotFound))
                    .count();
                let failed = results
                    .iter()
                    .filter(|app| matches!(app.status, RemoveStatus::Error))
                    .count();
                print_progress(
                    &format!(
                        "summary: {} removed, {} missing, {} failed",
                        removed, missing, failed
                    ),
                    color_output,
                );
                state_manager.save()?;
            }
        }
        Commands::SelfUpdate { pre_release } => {
            self_updater::self_update(&client, *pre_release, color_output)?;
        }
        Commands::Completion { shell } => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
        }
    }

    Ok(())
}

fn strategy_label(strategy: &config::StrategyConfig) -> &'static str {
    match strategy {
        config::StrategyConfig::Forge { .. } => "Forge",
        config::StrategyConfig::Direct { .. } => "Direct",
        config::StrategyConfig::Script { .. } => "Script",
    }
}

fn matches_target(target: Option<&str>, app_name: Option<&str>) -> bool {
    match target {
        Some(target) => app_name == Some(target),
        None => true,
    }
}
