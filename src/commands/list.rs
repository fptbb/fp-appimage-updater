use crate::commands::helpers::{ExecutionContext, app_is_ignored};
use crate::config;
use crate::output::{ListApp, ListResponse, print_json, print_list_human};
use anyhow::Result;

pub fn run(ctx: &ExecutionContext) -> Result<()> {
    let apps = ctx
        .app_configs
        .iter()
        .map(|app| {
            let state = ctx.state_manager.get_app(&app.name);
            ListApp {
                name: app.name.clone(),
                strategy: strategy_label(&app.strategy).to_string(),
                local_version: state.and_then(|s| s.local_version.clone()),
                ignored: app_is_ignored(app),
                integration: app
                    .integration
                    .unwrap_or(ctx.global_config.manage_desktop_files),
                symlink: app
                    .create_symlink
                    .unwrap_or(ctx.global_config.create_symlinks),
            }
        })
        .collect::<Vec<_>>();

    if ctx.json_output {
        print_json(&ListResponse {
            command: "list",
            apps,
        })?;
    } else {
        print_list_human(&apps, ctx.color_output);
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
