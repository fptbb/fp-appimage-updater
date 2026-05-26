use crate::commands::helpers::ExecutionContext;
use crate::disintegrator;
use crate::output::{
    RemoveApp, RemoveResponse, RemoveStatus, print_json, print_progress, print_warning,
};
use crate::state::AppState;
use anyhow::Result;

pub fn run(
    ctx: &mut ExecutionContext,
    app_name: Option<&String>,
    all: bool,
    orphan: bool,
) -> Result<()> {
    let mut found = false;
    let mut apps_to_remove = Vec::new();
    let mut results = Vec::new();

    if orphan {
        // Collect all orphaned app names from state that are not in configs
        let orphans: Vec<String> = ctx
            .state_manager
            .state
            .apps
            .keys()
            .filter(|name| !ctx.app_configs.iter().any(|app| app.name == **name))
            .cloned()
            .collect();

        if let Some(target) = app_name {
            if orphans.contains(target) {
                apps_to_remove.push((target.clone(), true));
            } else {
                if ctx.json_output {
                    print_json(&RemoveResponse {
                        command: "remove",
                        apps: Vec::new(),
                        error: Some(format!(
                            "App '{}' is not an orphaned application in state cache.",
                            target
                        )),
                    })?;
                } else {
                    print_warning(
                        &format!(
                            "Error: App '{}' is not an orphaned application in state cache.",
                            target
                        ),
                        ctx.color_output,
                    );
                }
                return Ok(());
            }
        } else {
            // Remove all orphans
            for name in orphans {
                apps_to_remove.push((name, true));
            }
            if apps_to_remove.is_empty() {
                if ctx.json_output {
                    print_json(&RemoveResponse {
                        command: "remove",
                        apps: Vec::new(),
                        error: Some("No orphaned applications found to remove.".to_string()),
                    })?;
                } else {
                    print_progress(
                        "No orphaned applications found to remove.",
                        ctx.color_output,
                    );
                }
                return Ok(());
            }
        }
    } else if all {
        for app in ctx.app_configs {
            apps_to_remove.push((app.name.clone(), false));
        }
    } else if let Some(target) = app_name {
        apps_to_remove.push((target.clone(), false));
    } else {
        if ctx.json_output {
            print_json(&RemoveResponse {
                command: "remove",
                apps: Vec::new(),
                error: Some(
                    "Please provide an application name to remove, or use --all or --orphan."
                        .to_string(),
                ),
            })?;
        } else {
            print_warning(
                "Error: Please provide an application name to remove, or use --all or --orphan.",
                ctx.color_output,
            );
        }
        return Ok(());
    }

    for (target_name, is_orphan) in apps_to_remove {
        let mut target_found = false;
        let mut matching_config = None;

        for app in ctx.app_configs {
            if app.name == target_name {
                matching_config = Some(app);
                break;
            }
        }

        if let Some(app) = matching_config {
            found = true;
            target_found = true;
            let state = ctx.state_manager.get_app(&app.name);

            if let Err(e) = disintegrator::remove_app(
                app,
                ctx.global_config,
                state,
                ctx.json_output,
                ctx.color_output,
            ) {
                if ctx.json_output {
                    results.push(RemoveApp {
                        name: app.name.clone(),
                        status: RemoveStatus::Error,
                        error: Some(format!("{:#}", e)),
                    });
                } else {
                    print_warning(
                        &format!("Error removing {}: {:#}", app.name, e),
                        ctx.color_output,
                    );
                }
            } else {
                if is_orphan {
                    ctx.state_manager.state.apps.remove(&app.name);
                } else if let Some(state) = ctx.state_manager.state.apps.get_mut(&app.name) {
                    clear_installed_state(state);
                }
                if ctx.json_output {
                    results.push(RemoveApp {
                        name: app.name.clone(),
                        status: RemoveStatus::Removed,
                        error: None,
                    });
                }
            }
        } else {
            if let Some(state) = ctx.state_manager.state.apps.get(&target_name).cloned() {
                found = true;
                target_found = true;
                let dummy_app = crate::config::AppConfig {
                    config_dir: std::path::PathBuf::new(),
                    name: target_name.clone(),
                    ignore: None,
                    zsync: None,
                    integration: None,
                    create_symlink: None,
                    segmented_downloads: None,
                    respect_rate_limits: None,
                    github_proxy: None,
                    github_proxy_prefix: None,
                    storage_dir: None,
                    naming_format: None,
                    inner_asset_match: None,
                    strategy: crate::config::StrategyConfig::Direct {
                        url: String::new(),
                        check_method: crate::config::CheckMethod::Etag,
                    },
                };

                if let Err(e) = disintegrator::remove_app(
                    &dummy_app,
                    ctx.global_config,
                    Some(&state),
                    ctx.json_output,
                    ctx.color_output,
                ) {
                    if ctx.json_output {
                        results.push(RemoveApp {
                            name: target_name.clone(),
                            status: RemoveStatus::Error,
                            error: Some(format!("{:#}", e)),
                        });
                    } else {
                        print_warning(
                            &format!("Error removing orphaned app {}: {:#}", target_name, e),
                            ctx.color_output,
                        );
                    }
                } else {
                    ctx.state_manager.state.apps.remove(&target_name);
                    if ctx.json_output {
                        results.push(RemoveApp {
                            name: target_name.clone(),
                            status: RemoveStatus::Removed,
                            error: None,
                        });
                    }
                }
            }
        }

        if !target_found && ctx.json_output {
            results.push(RemoveApp {
                name: target_name,
                status: RemoveStatus::NotFound,
                error: Some("App not found in configuration or state cache.".to_string()),
            });
        }
    }

    if ctx.json_output {
        let error = if !found && !all && !orphan {
            app_name.map(|target| format!("App '{}' not found in configuration.", target))
        } else {
            None
        };
        print_json(&RemoveResponse {
            command: "remove",
            apps: results,
            error,
        })?;
    } else if !found && !all && !orphan {
        print_warning(
            &format!("App '{:?}' not found in configuration.", app_name),
            ctx.color_output,
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
            ctx.color_output,
        );
    }
    Ok(())
}

pub fn clear_installed_state(state: &mut AppState) {
    state.local_version = None;
    state.etag = None;
    state.last_modified = None;
    state.file_path = None;
    state.sanitized_name = None;
    state.rate_limited_until = None;
}
