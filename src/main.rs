use anyhow::Result;
use clap::{CommandFactory, Parser};
use comfy_table::Table;
use reqwest::Client;
use std::time::Duration;

mod cli;
mod config;
mod disintegrator;
mod downloader;
mod integrator;
mod parser;
mod resolvers;
mod state;

use cli::{Cli, Commands};
use parser::ConfigPaths;
use state::StateManager;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    let paths = ConfigPaths::new()?;
    let global_config = parser::load_global_config(&paths)?;
    let app_configs = parser::load_app_configs(&paths)?;
    let mut state_manager = StateManager::load(paths.cache_path());

    // Use a connect timeout, but leave the stream timeout unbounded
    // so large AppImages (e.g., 250MB+) don't timeout mid-download.
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .user_agent("fp-appimage-updater/1.0")
        .build()?;

    match &cli.command {
        Commands::List => {
            let mut table = Table::new();
            table.set_header(vec!["App Name", "Strategy", "Local Version", "Integration"]);

            for app in &app_configs {
                let strategy = match app.strategy {
                    config::StrategyConfig::Forge { .. } => "Forge",
                    config::StrategyConfig::Direct { .. } => "Direct",
                    config::StrategyConfig::Script { .. } => "Script",
                };

                let state = state_manager.get_app(&app.name);
                let version = state.and_then(|s| s.local_version.clone()).unwrap_or_else(|| "Not installed".to_string());
                let integration_status = app.integration.unwrap_or(global_config.manage_desktop_files);

                table.add_row(vec![
                    app.name.as_str(),
                    strategy,
                    version.as_str(),
                    if integration_status { "Yes" } else { "No" },
                ]);
            }

            println!("{table}");
        }
        Commands::Check { app_name } => {
            let mut found = false;
            for app in &app_configs {
                if let Some(target) = &app_name {
                    if app.name != *target {
                        continue;
                    }
                }
                found = true;
                
                let state = state_manager.get_app(&app.name);
                match resolvers::check_for_updates(app, state).await {
                    Ok(Some(info)) => {
                        println!("Update available for {}: new version {}", app.name, info.version);
                    }
                    Ok(None) => {
                        println!("{} is up to date.", app.name);
                    }
                    Err(e) => {
                        eprintln!("Error checking updates for {}: {}", app.name, e);
                    }
                }
            }
            if let Some(target) = app_name {
                if !found {
                    eprintln!("App '{}' not found in configuration.", target);
                }
            }
        }
        Commands::Update { app_name } => {
            let storage_dir = integrator::expand_tilde(&global_config.storage_dir);
            
            for app in &app_configs {
                if let Some(target) = app_name {
                    if app.name != *target {
                        continue;
                    }
                }

                let state = state_manager.get_app(&app.name);
                match resolvers::check_for_updates(app, state).await {
                    Ok(Some(info)) => {
                        println!("Updating {} to version {}...", app.name, info.version);
                        
                        match downloader::download_app(
                            &client,
                            app,
                            &info,
                            &storage_dir,
                            &global_config.naming_format,
                            state,
                        ).await {
                            Ok(new_path) => {
                                let old_path_str = state.and_then(|s| s.file_path.clone());
                                let old_path = old_path_str.as_ref().map(std::path::Path::new);
                                
                                if let Err(e) = integrator::integrate(app, &global_config, &new_path, old_path).await {
                                    eprintln!("Integration failed for {}: {}", app.name, e);
                                } else {
                                    // Update State
                                    let state_mut = state_manager.get_app_mut(&app.name);
                                    state_mut.local_version = Some(info.version);
                                    if let Some(etag) = info.new_etag {
                                        state_mut.etag = Some(etag);
                                    }
                                    if let Some(lm) = info.new_last_modified {
                                        state_mut.last_modified = Some(lm);
                                    }
                                    state_mut.file_path = Some(new_path.to_string_lossy().to_string());
                                    
                                    println!("{} successfully updated.", app.name);
                                }
                            }
                            Err(e) => eprintln!("Download failed for {}: {}", app.name, e),
                        }
                    }
                    Ok(None) => println!("{} is up to date.", app.name),
                    Err(e) => eprintln!("Error checking updates for {}: {}", app.name, e),
                }
            }

            state_manager.save()?;
        }
        Commands::Remove { app_name, all } => {
            let mut found = false;
            let mut apps_to_remove = Vec::new();

            if *all {
                for app in &app_configs {
                    apps_to_remove.push(app.name.clone());
                }
            } else if let Some(target) = app_name {
                apps_to_remove.push(target.clone());
            } else {
                eprintln!("Error: Please provide an application name to remove, or use --all.");
                return Ok(());
            }

            for target_name in apps_to_remove {
                for app in &app_configs {
                    if app.name == target_name {
                        found = true;
                        let state = state_manager.get_app(&app.name);
                        
                        if let Err(e) = disintegrator::remove_app(app, &global_config, state) {
                            eprintln!("Error removing {}: {}", app.name, e);
                        } else {
                            // After successful removal, clear from state cache
                            state_manager.state.apps.remove(&app.name);
                        }
                        break;
                    }
                }
            }
            
            if !found && !all {
                eprintln!("App '{:?}' not found in configuration.", app_name);
            } else {
                state_manager.save()?;
            }
        }
        Commands::Completion { shell } => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
        }
    }

    Ok(())
}
