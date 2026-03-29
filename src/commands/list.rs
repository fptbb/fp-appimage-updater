use crate::config;
use crate::output::{ListApp, ListResponse, print_json, print_list_human};
use crate::state::StateManager;
use anyhow::Result;

pub fn run(
    app_configs: &[config::AppConfig],
    global_config: &config::GlobalConfig,
    state_manager: &StateManager,
    json_output: bool,
    color_output: bool,
) -> Result<()> {
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
    Ok(())
}

fn strategy_label(strategy: &config::StrategyConfig) -> &'static str {
    match strategy {
        config::StrategyConfig::Forge { .. } => "Forge",
        config::StrategyConfig::Direct { .. } => "Direct",
        config::StrategyConfig::Script { .. } => "Script",
    }
}
