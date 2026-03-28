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
use output::{
    CheckApp, CheckResponse, CheckStatus, DoctorCheck, DoctorResponse, DoctorStatus, ListApp,
    ListResponse, RemoveApp, RemoveResponse, RemoveStatus, UpdateApp, UpdateResponse, UpdateStatus,
    ValidateApp, ValidateResponse, ValidateStatus, colors_enabled, print_check_human,
    print_doctor_human, print_json, print_list_human, print_progress, print_success,
    print_validate_human, print_warning,
};
use parser::ConfigPaths;
use state::StateManager;
use std::time::Instant;
use std::sync::mpsc;

const MAX_CONCURRENT_JOBS: usize = 2;

fn build_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent("fp-appimage-updater/1.0")
        .build()
        .into()
}

enum UpdateWorkResult {
    Updated {
        app: config::AppConfig,
        from_version: Option<String>,
        info: resolvers::UpdateInfo,
        new_path: std::path::PathBuf,
        old_path: Option<std::path::PathBuf>,
        started_at: Instant,
    },
    UpToDate {
        name: String,
        from_version: Option<String>,
        path: Option<String>,
    },
    Error {
        stage: UpdateErrorStage,
        name: String,
        from_version: Option<String>,
        to_version: Option<String>,
        path: Option<String>,
        error: String,
    },
}

enum UpdateErrorStage {
    Check,
    Download,
}

enum CheckWorkResult {
    Ok(CheckApp),
    Err(CheckApp),
}

fn process_check_job(
    app: config::AppConfig,
    state: Option<state::AppState>,
    client: &ureq::Agent,
) -> CheckWorkResult {
    let local_version = state.as_ref().and_then(|s| s.local_version.clone());

    match resolvers::check_for_updates(&app, state.as_ref(), client) {
        Ok(result) => {
            let mut capabilities = result.capabilities;
            if matches!(
                app.zsync,
                Some(config::ZsyncConfig::Enabled(true)) | Some(config::ZsyncConfig::Url(_))
            ) {
                capabilities.push("zsync".to_string());
            }
            resolvers::dedupe_capabilities(&mut capabilities);

            match result.update {
                Some(info) => CheckWorkResult::Ok(CheckApp {
                    name: app.name,
                    status: CheckStatus::UpdateAvailable,
                    local_version,
                    remote_version: Some(info.version),
                    download_url: Some(info.download_url),
                    capabilities,
                    error: None,
                }),
                None => CheckWorkResult::Ok(CheckApp {
                    name: app.name,
                    status: CheckStatus::UpToDate,
                    local_version,
                    remote_version: None,
                    download_url: None,
                    capabilities,
                    error: None,
                }),
            }
        }
        Err(e) => CheckWorkResult::Err(CheckApp {
            name: app.name,
            status: CheckStatus::Error,
            local_version,
            remote_version: None,
            download_url: None,
            capabilities: Vec::new(),
            error: Some(format!("{:#}", e)),
        }),
    }
}

fn process_update_job(
    client: &ureq::Agent,
    app: config::AppConfig,
    state: Option<state::AppState>,
    storage_dir: std::path::PathBuf,
    naming_format: String,
    segmented_downloads: bool,
    json_output: bool,
    color_output: bool,
) -> UpdateWorkResult {
    let started_at = Instant::now();
    let app_name = app.name.clone();
    let from_version = state.as_ref().and_then(|s| s.local_version.clone());
    let current_path = state.as_ref().and_then(|s| s.file_path.clone());

    match resolvers::check_for_updates(&app, state.as_ref(), &client) {
        Ok(result) => {
            let Some(info) = result.update else {
                return UpdateWorkResult::UpToDate {
                    name: app_name,
                    from_version,
                    path: current_path,
                };
            };
            let to_version = info.version.clone();

            if !json_output {
                print_progress(&format!("Downloading {}", app_name), color_output);
            }

            match downloader::download_app(
                &client,
                &app,
                &info,
                &storage_dir,
                &naming_format,
                state.as_ref(),
                segmented_downloads,
                json_output,
                color_output,
            ) {
                Ok(new_path) => UpdateWorkResult::Updated {
                    app,
                    from_version,
                    info,
                    new_path,
                    old_path: current_path.map(std::path::PathBuf::from),
                    started_at,
                },
                Err(e) => UpdateWorkResult::Error {
                    stage: UpdateErrorStage::Download,
                    name: app_name,
                    from_version,
                    to_version: Some(to_version),
                    path: None,
                    error: format!("Download failed: {:#}", e),
                },
            }
        }
        Err(e) => UpdateWorkResult::Error {
            stage: UpdateErrorStage::Check,
            name: app_name,
            from_version,
            to_version: None,
            path: current_path,
            error: format!("{:#}", e),
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

            for app in &app_configs {
                if let Some(target) = &app_name
                    && app.name != *target
                {
                    continue;
                }
                found = true;
                check_jobs.push((app.clone(), state_manager.get_app(&app.name).cloned()));
            }

            let worker_limit = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
                .min(MAX_CONCURRENT_JOBS)
                .max(1);

            let (tx, rx) = mpsc::channel();
            let mut pending = check_jobs.into_iter();
            let mut active = 0usize;

            let spawn_check_worker = |app: config::AppConfig,
                                      state: Option<state::AppState>,
                                      client: ureq::Agent,
                                      tx: mpsc::Sender<CheckWorkResult>| {
                std::thread::spawn(move || {
                    let _ = tx.send(process_check_job(app, state, &client));
                });
            };

            while active < worker_limit {
                if let Some((app, state)) = pending.next() {
                    spawn_check_worker(app, state, client.clone(), tx.clone());
                    active += 1;
                } else {
                    break;
                }
            }

            while active > 0 {
                let result = rx.recv().expect("check worker panicked");
                active -= 1;

                match result {
                    CheckWorkResult::Ok(app_result) => results.push(app_result),
                    CheckWorkResult::Err(app_result) => results.push(app_result),
                }

                if let Some((app, state)) = pending.next() {
                    spawn_check_worker(app, state, client.clone(), tx.clone());
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
                print_check_human(&results, error.as_deref(), color_output);
            }
        }
        Commands::Update { app_name } => {
            let storage_dir = integrator::expand_tilde(&global_config.storage_dir);
            let mut results = Vec::new();
            let mut found = false;
            let mut update_jobs = Vec::new();

            for app in &app_configs {
                if let Some(target) = app_name
                    && app.name != *target
                {
                    continue;
                }
                found = true;
                update_jobs.push((app.clone(), state_manager.get_app(&app.name).cloned()));
            }

            let worker_limit = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2)
                .min(MAX_CONCURRENT_JOBS)
                .max(1);

            let (tx, rx) = mpsc::channel();
            let mut pending = update_jobs.into_iter();
            let mut active = 0usize;

            let spawn_update_worker = |app: config::AppConfig,
                                       state: Option<state::AppState>,
                                       storage_dir: std::path::PathBuf,
                                       naming_format: String,
                                       segmented_downloads: bool,
                                       client: ureq::Agent,
                                       tx: mpsc::Sender<UpdateWorkResult>,
                                       json_output: bool,
                                       color_output: bool| {
                std::thread::spawn(move || {
                    let _ = tx.send(process_update_job(
                        &client,
                        app,
                        state,
                        storage_dir,
                        naming_format,
                        segmented_downloads,
                        json_output,
                        color_output,
                    ));
                });
            };

            while active < worker_limit {
                if let Some((app, state)) = pending.next() {
                    let storage_dir = storage_dir.clone();
                    let naming_format = global_config.naming_format.clone();
                    let segmented_downloads = app
                        .segmented_downloads
                        .unwrap_or(global_config.segmented_downloads);
                    spawn_update_worker(
                        app,
                        state,
                        storage_dir,
                        naming_format,
                        segmented_downloads,
                        client.clone(),
                        tx.clone(),
                        json_output,
                        color_output,
                    );
                    active += 1;
                } else {
                    break;
                }
            }

            while active > 0 {
                let result = rx.recv().expect("update worker panicked");
                active -= 1;

                match result {
                        UpdateWorkResult::Updated {
                            app,
                            from_version,
                            info,
                            new_path,
                            old_path,
                            started_at,
                        } => {
                            let app_name = app.name.clone();
                            let to_version = info.version.clone();
                            let old_path_ref = old_path.as_deref();
                            let was_update = from_version.is_some();

                            if let Err(e) =
                                integrator::integrate(&app, &global_config, &new_path, old_path_ref)
                            {
                                results.push(UpdateApp {
                                    name: app_name.clone(),
                                    status: UpdateStatus::Error,
                                    from_version: from_version.clone(),
                                    to_version: Some(to_version.clone()),
                                    path: Some(new_path.to_string_lossy().to_string()),
                                    duration_seconds: None,
                                    error: Some(format!("Integration failed: {:#}", e)),
                                });
                                if !json_output {
                                    print_warning(
                                        &format!("Integration failed for {}: {:#}", app_name, e),
                                        color_output,
                                    );
                                    print_warning(
                                        &format!(
                                            "Rolling back {} to its previous state...",
                                            app_name
                                        ),
                                        color_output,
                                    );
                                }

                                integrator::rollback(&app, &global_config, &new_path, old_path_ref);
                            } else {
                                let duration_seconds = started_at.elapsed().as_secs_f64();
                                let state_mut = state_manager.get_app_mut(&app_name);
                                state_mut.local_version = Some(to_version.clone());
                                if let Some(etag) = info.new_etag {
                                    state_mut.etag = Some(etag);
                                }
                                if let Some(lm) = info.new_last_modified {
                                    state_mut.last_modified = Some(lm);
                                }
                                state_mut.file_path = Some(new_path.to_string_lossy().to_string());

                                results.push(UpdateApp {
                                    name: app_name.clone(),
                                    status: UpdateStatus::Updated,
                                    from_version,
                                    to_version: Some(to_version.clone()),
                                    path: Some(new_path.to_string_lossy().to_string()),
                                    duration_seconds: Some(duration_seconds),
                                    error: None,
                                });
                                if !json_output {
                                    let action = if was_update {
                                        "updated to"
                                    } else {
                                        "installed"
                                    };
                                    print_success(
                                        &format!(
                                            "{} {} {} in {:.2}s",
                                            app_name, action, to_version, duration_seconds
                                        ),
                                        color_output,
                                    );
                                }
                            }
                        }
                        UpdateWorkResult::UpToDate {
                            name,
                            from_version,
                            path,
                        } => {
                            results.push(UpdateApp {
                                name: name.clone(),
                                status: UpdateStatus::UpToDate,
                                from_version: from_version.clone(),
                                to_version: None,
                                path,
                                duration_seconds: None,
                                error: None,
                            });
                            if !json_output {
                                print_progress(
                                    &format!(
                                        "{} is already up to date ({})",
                                        name,
                                        from_version.unwrap_or_else(|| "unknown".to_string())
                                    ),
                                    color_output,
                                );
                            }
                        }
                        UpdateWorkResult::Error {
                            stage,
                            name,
                            from_version,
                            to_version,
                            path,
                            error,
                        } => {
                            results.push(UpdateApp {
                                name: name.clone(),
                                status: UpdateStatus::Error,
                                from_version,
                                to_version,
                                path,
                                duration_seconds: None,
                                error: Some(error.clone()),
                            });
                            if !json_output {
                                match stage {
                                    UpdateErrorStage::Check => print_warning(
                                        &format!("Error checking updates for {}: {}", name, error),
                                        color_output,
                                    ),
                                    UpdateErrorStage::Download => print_warning(
                                        &format!("Download failed for {}: {}", name, error),
                                        color_output,
                                    ),
                                }
                            }
                        }
                    }

                if let Some((app, state)) = pending.next() {
                    let storage_dir = storage_dir.clone();
                    let naming_format = global_config.naming_format.clone();
                    let segmented_downloads = app
                        .segmented_downloads
                        .unwrap_or(global_config.segmented_downloads);
                    spawn_update_worker(
                        app,
                        state,
                        storage_dir,
                        naming_format,
                        segmented_downloads,
                        client.clone(),
                        tx.clone(),
                        json_output,
                        color_output,
                    );
                    active += 1;
                }
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
                    duration_seconds: None,
                    error: Some(parse_error_message.clone()),
                });
                if !json_output {
                    print_warning(&parse_error_message, color_output);
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
                let failed = results
                    .iter()
                    .filter(|app| matches!(app.status, UpdateStatus::Error))
                    .count();
                print_progress(
                    &format!(
                        "summary: {} updated, {} current, {} failed",
                        updated, current, failed
                    ),
                    color_output,
                );
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
