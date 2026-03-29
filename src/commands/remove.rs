use crate::config;
use crate::disintegrator;
use crate::output::{
    RemoveApp, RemoveResponse, RemoveStatus, print_json, print_progress, print_warning,
};
use crate::state::StateManager;
use anyhow::Result;

pub fn run(
    app_configs: &[config::AppConfig],
    global_config: &config::GlobalConfig,
    state_manager: &mut StateManager,
    app_name: Option<&String>,
    all: bool,
    json_output: bool,
    color_output: bool,
) -> Result<()> {
    let mut found = false;
    let mut apps_to_remove = Vec::new();
    let mut results = Vec::new();

    if all {
        for app in app_configs {
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
                    "Please provide an application name to remove, or use --all.".to_string(),
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
        let mut target_found_in_configs = false;
        for app in app_configs {
            if app.name == target_name {
                found = true;
                target_found_in_configs = true;
                let state = state_manager.get_app(&app.name);

                if let Err(e) =
                    disintegrator::remove_app(app, global_config, state, json_output, color_output)
                {
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
        if !target_found_in_configs && json_output {
            results.push(RemoveApp {
                name: target_name,
                status: RemoveStatus::NotFound,
                error: Some("App not found in configuration.".to_string()),
            });
        }
    }

    if json_output {
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
    }
    Ok(())
}
