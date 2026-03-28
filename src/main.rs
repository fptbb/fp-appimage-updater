use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
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
use output::{colors_enabled};
use parser::ConfigPaths;
use state::StateManager;
use commands::helpers::build_http_agent;

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

    // Init is handled early because it doesn't require loading config
    if let Commands::Init {
        global,
        app,
        strategy,
        force,
    } = &cli.command
    {
        return commands::init::run(
            &paths,
            *global,
            app.as_deref(),
            *strategy,
            *force,
            json_output,
            color_output,
        );
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
            commands::doctor::run(
                &paths,
                app_configs.len(),
                app_config_errors.len(),
                &app_config_errors,
                json_output,
                color_output,
            )?;
        }
        Commands::Validate { app_name } => {
            commands::validate::run(&paths, app_name.as_deref(), json_output, color_output)?;
        }
        Commands::List => {
            commands::list::run(
                &app_configs,
                &global_config,
                &state_manager,
                json_output,
                color_output,
            )?;
        }
        Commands::Check { app_name } => {
            commands::check::run(
                &app_configs,
                &app_config_errors,
                &global_config,
                &mut state_manager,
                &client,
                app_name.as_deref(),
                json_output,
                color_output,
            )?;
            state_manager.save()?;
        }
        Commands::Update { app_name } => {
            commands::update::run(
                &app_configs,
                &app_config_errors,
                &global_config,
                &mut state_manager,
                &client,
                app_name.as_deref(),
                json_output,
                color_output,
            )?;
            state_manager.save()?;
        }
        Commands::Remove { app_name, all } => {
            commands::remove::run(
                &app_configs,
                &global_config,
                &mut state_manager,
                app_name.as_ref(),
                *all,
                json_output,
                color_output,
            )?;
            state_manager.save()?;
        }
        Commands::SelfUpdate { pre_release } => {
            commands::self_update::run(&client, *pre_release, color_output)?;
        }
        Commands::Completion { shell } => {
            commands::completion::run(*shell)?;
        }
    }

    Ok(())
}
