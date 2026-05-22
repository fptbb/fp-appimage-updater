use crate::commands::helpers::ExecutionContext;
use crate::disintegrator;
use crate::output::{
    RemoveApp, RemoveResponse, RemoveStatus, print_json, print_progress, print_warning,
};
use crate::state::AppState;
use anyhow::Result;

pub fn run(ctx: &mut ExecutionContext, app_name: Option<&String>, all: bool) -> Result<()> {
    let mut found = false;
    let mut apps_to_remove = Vec::new();
    let mut results = Vec::new();

    if all {
        for app in ctx.app_configs {
            apps_to_remove.push(app.name.clone());
        }
    } else if let Some(target) = app_name {
        apps_to_remove.push(target.clone());
    } else {
        if ctx.json_output {
            print_json(&RemoveResponse {
                command: "remove",
                apps: Vec::new(),
                error: Some(
                    "Please provide an application name to remove, or use --all.".to_string(),
                ),
            })?;
        } else {
            print_warning(
                "Error: Please provide an application name to remove, or use --all.",
                ctx.color_output,
            );
        }
        return Ok(());
    }

    for target_name in apps_to_remove {
        let mut target_found_in_configs = false;
        for app in ctx.app_configs {
            if app.name == target_name {
                found = true;
                target_found_in_configs = true;
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
                    if let Some(state) = ctx.state_manager.state.apps.get_mut(&app.name) {
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
                break;
            }
        }
        if !target_found_in_configs && ctx.json_output {
            results.push(RemoveApp {
                name: target_name,
                status: RemoveStatus::NotFound,
                error: Some("App not found in configuration.".to_string()),
            });
        }
    }

    if ctx.json_output {
        let error = if !found && !all {
            app_name.map(|target| format!("App '{}' not found in configuration.", target))
        } else {
            None
        };
        print_json(&RemoveResponse {
            command: "remove",
            apps: results,
            error,
        })?;
    } else if !found && !all {
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
