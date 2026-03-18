use anyhow::Result;
use clap::{CommandFactory, Parser};
use reqwest::Client;
use std::time::Duration;

mod cli;
mod config;
mod disintegrator;
mod downloader;
mod integrator;
mod parser;
mod output;
mod resolvers;
mod self_updater;
mod state;

use cli::{Cli, Commands};
use output::{
    colors_enabled, print_check_human, print_json, print_list_human, print_progress,
    print_success, print_warning, CheckApp, CheckResponse, CheckStatus, ListApp, ListResponse,
    RemoveApp, RemoveResponse, RemoveStatus, UpdateApp, UpdateResponse, UpdateStatus,
};
use parser::ConfigPaths;
use state::StateManager;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let json_output = cli.json;
    
    let paths = if let Some(config_dir) = cli.config.clone() {
        ConfigPaths::with_config_dir(config_dir)?
    } else {
        ConfigPaths::new()?
    };
    let global_config = parser::load_global_config(&paths)?;
    let app_configs = parser::load_app_configs(&paths)?;
    let mut state_manager = StateManager::load(paths.cache_path());
    let color_output = colors_enabled(json_output);

    // Use a connect timeout, but leave the stream timeout unbounded
    // so large AppImages (e.g., 250MB+) don't timeout mid-download.
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .user_agent("fp-appimage-updater/1.0")
        .build()?;

    match &cli.command {
        Commands::List => {
            let apps = app_configs
                .iter()
                .map(|app| {
                    let state = state_manager.get_app(&app.name);
                    ListApp {
                        name: app.name.clone(),
                        strategy: strategy_label(&app.strategy).to_string(),
                        local_version: state.and_then(|s| s.local_version.clone()),
                        integration: app.integration.unwrap_or(global_config.manage_desktop_files),
                        symlink: app.create_symlink.unwrap_or(global_config.create_symlinks),
                    }
                })
                .collect::<Vec<_>>();

            if json_output {
                print_json(&ListResponse { command: "list", apps })?;
            } else {
                print_list_human(&apps, color_output);
            }
        }
        Commands::Check { app_name } => {
            let mut found = false;
            let mut results = Vec::new();
            for app in &app_configs {
                if let Some(target) = &app_name && app.name != *target {
                    continue;
                }
                found = true;
                
                let state = state_manager.get_app(&app.name);
                match resolvers::check_for_updates(app, state).await {
                    Ok(Some(info)) => {
                        results.push(CheckApp {
                            name: app.name.clone(),
                            status: CheckStatus::UpdateAvailable,
                            local_version: state.and_then(|s| s.local_version.clone()),
                            remote_version: Some(info.version),
                            download_url: Some(info.download_url),
                            error: None,
                        });
                    }
                    Ok(None) => {
                        results.push(CheckApp {
                            name: app.name.clone(),
                            status: CheckStatus::UpToDate,
                            local_version: state.and_then(|s| s.local_version.clone()),
                            remote_version: None,
                            download_url: None,
                            error: None,
                        });
                    }
                    Err(e) => {
                        results.push(CheckApp {
                            name: app.name.clone(),
                            status: CheckStatus::Error,
                            local_version: state.and_then(|s| s.local_version.clone()),
                            remote_version: None,
                            download_url: None,
                            error: Some(format!("{:#}", e)),
                        });
                    }
                }
            }

            let error = if let Some(target) = app_name && !found {
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
            
            for app in &app_configs {
                if let Some(target) = app_name && app.name != *target {
                    continue;
                }
                found = true;

                let state = state_manager.get_app(&app.name);
                match resolvers::check_for_updates(app, state).await {
                    Ok(Some(info)) => {
                        let from_version = state.and_then(|s| s.local_version.clone());
                        let to_version = info.version.clone();

                        if !json_output {
                            print_progress(
                                &format!(
                                    "Updating {} from {} to {}",
                                    app.name,
                                    from_version.as_deref().unwrap_or("unknown"),
                                    to_version
                                ),
                                color_output,
                            );
                        }
                        
                        match downloader::download_app(
                            &client,
                            app,
                            &info,
                            &storage_dir,
                            &global_config.naming_format,
                            state,
                            json_output,
                            color_output,
                        )
                        .await
                        {
                            Ok(new_path) => {
                                let old_path_str = state.and_then(|s| s.file_path.clone());
                                let old_path = old_path_str.as_ref().map(std::path::Path::new);
                                
                                if let Err(e) = integrator::integrate(app, &global_config, &new_path, old_path).await {
                                    if json_output {
                                        results.push(UpdateApp {
                                            name: app.name.clone(),
                                            status: UpdateStatus::Error,
                                            from_version,
                                            to_version: Some(to_version),
                                            path: Some(new_path.to_string_lossy().to_string()),
                                            error: Some(format!("Integration failed: {:#}", e)),
                                        });
                                    } else {
                                        print_warning(
                                            &format!("Integration failed for {}: {:#}", app.name, e),
                                            color_output,
                                        );
                                    }
                                } else {
                                    // Update State
                                    let state_mut = state_manager.get_app_mut(&app.name);
                                    state_mut.local_version = Some(to_version.clone());
                                    if let Some(etag) = info.new_etag {
                                        state_mut.etag = Some(etag);
                                    }
                                    if let Some(lm) = info.new_last_modified {
                                        state_mut.last_modified = Some(lm);
                                    }
                                    state_mut.file_path = Some(new_path.to_string_lossy().to_string());
                                    
                                    if json_output {
                                        results.push(UpdateApp {
                                            name: app.name.clone(),
                                            status: UpdateStatus::Updated,
                                            from_version,
                                            to_version: Some(to_version),
                                            path: Some(new_path.to_string_lossy().to_string()),
                                            error: None,
                                        });
                                    } else {
                                        print_success(
                                            &format!("{} updated to {}", app.name, to_version),
                                            color_output,
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                if json_output {
                                    results.push(UpdateApp {
                                        name: app.name.clone(),
                                        status: UpdateStatus::Error,
                                        from_version,
                                        to_version: Some(to_version),
                                        path: None,
                                        error: Some(format!("Download failed: {:#}", e)),
                                    });
                                } else {
                                    print_warning(
                                        &format!("Download failed for {}: {:#}", app.name, e),
                                        color_output,
                                    );
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        if json_output {
                            results.push(UpdateApp {
                                name: app.name.clone(),
                                status: UpdateStatus::UpToDate,
                                from_version: state.and_then(|s| s.local_version.clone()),
                                to_version: None,
                                path: state.and_then(|s| s.file_path.clone()),
                                error: None,
                            });
                        } else {
                            print_progress(
                                &format!(
                                    "{} is already up to date ({})",
                                    app.name,
                                    state
                                        .and_then(|s| s.local_version.clone())
                                        .unwrap_or_else(|| "unknown".to_string())
                                ),
                                color_output,
                            );
                        }
                    }
                    Err(e) => {
                        if json_output {
                            results.push(UpdateApp {
                                name: app.name.clone(),
                                status: UpdateStatus::Error,
                                from_version: state.and_then(|s| s.local_version.clone()),
                                to_version: None,
                                path: state.and_then(|s| s.file_path.clone()),
                                error: Some(format!("{:#}", e)),
                            });
                        } else {
                            print_warning(
                                &format!("Error checking updates for {}: {:#}", app.name, e),
                                color_output,
                            );
                        }
                    }
                }
            }

            if json_output {
                let error = if let Some(target) = app_name && !found {
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
                        error: Some("Please provide an application name to remove, or use --all.".to_string()),
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
                            // After successful removal, clear from state cache
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
            self_updater::self_update(&client, *pre_release, color_output).await?;
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
