use anyhow::Result;
use fp_appimage_updater::cli::{Cli, Commands};
use fp_appimage_updater::commands;
use fp_appimage_updater::commands::helpers::{ExecutionContext, build_http_agent};
use fp_appimage_updater::lock;
use fp_appimage_updater::output::colors_enabled;
use fp_appimage_updater::output::print_warning;
use fp_appimage_updater::parser::{self, ConfigPaths};
use fp_appimage_updater::state::StateManager;
use fp_appimage_updater::update::effective_show_all;

fn main() -> Result<()> {
    let cli = Cli::parse()?;
    let json_output = cli.json;

    if let Commands::GenerateSchema { schema_type } = &cli.command {
        return commands::generate_schema::run(schema_type);
    }

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
        let global_config = fp_appimage_updater::config::GlobalConfig::default();
        let app_configs = [];
        let app_config_errors = [];
        let mut state_manager = StateManager::load("");
        let client = build_http_agent();
        let ctx = ExecutionContext {
            paths: &paths,
            global_config: &global_config,
            app_configs: &app_configs,
            app_config_errors: &app_config_errors,
            state_manager: &mut state_manager,
            client: &client,
            json_output,
            color_output,
        };
        return commands::init::run(&ctx, *global, app.as_deref(), *strategy, *force);
    }

    let global_config = parser::load_global_config(&paths)?;
    let app_load = parser::load_app_configs(&paths)?;
    let app_configs = app_load.apps;
    let app_config_errors = app_load.errors;
    let mut state_manager = StateManager::load(paths.cache_path());

    let client = build_http_agent();

    let mut ctx = ExecutionContext {
        paths: &paths,
        global_config: &global_config,
        app_configs: &app_configs,
        app_config_errors: &app_config_errors,
        state_manager: &mut state_manager,
        client: &client,
        json_output,
        color_output,
    };

    match &cli.command {
        Commands::Init { .. } => unreachable!("init handled before config loading"),
        Commands::Doctor => {
            commands::doctor::run(&ctx)?;
        }
        Commands::Validate { app_name } => {
            commands::validate::run(&ctx, app_name.as_deref())?;
        }
        Commands::List => {
            commands::list::run(&ctx)?;
        }
        Commands::Check { app_name } => {
            commands::check::run(&mut ctx, app_name.as_deref())?;
            save_state_best_effort(ctx.state_manager, ctx.color_output);
        }
        Commands::Update {
            app_name,
            show_all,
            self_update,
            debug_download_url,
            debug_version,
        } => {
            let show_all = effective_show_all(ctx.global_config.show_all, *show_all);
            commands::update::run(
                &mut ctx,
                app_name.as_deref(),
                show_all,
                debug_download_url.as_deref(),
                debug_version.as_deref(),
            )?;
            save_state_best_effort(ctx.state_manager, ctx.color_output);

            // After application updates, handle self-update
            if *self_update || ctx.global_config.auto_self_update {
                commands::self_update::run_if_available(&ctx, false)?;
            } else if !ctx.json_output {
                // By default, just check for updates if not in JSON mode
                commands::self_update::check(&ctx, false)?;
            }
        }
        Commands::Remove {
            app_name,
            all,
            orphan,
        } => {
            commands::remove::run(&mut ctx, app_name.as_ref(), *all, *orphan)?;
            save_state_best_effort(ctx.state_manager, ctx.color_output);
        }
        Commands::SelfUpdate { pre_release } => {
            commands::self_update::run(&ctx, *pre_release)?;
        }
        Commands::Completion { shell } => {
            commands::completion::run(shell)?;
        }
        Commands::GenerateSchema { .. } => unreachable!("generate-schema handled early"),
    }

    Ok(())
}

fn save_state_best_effort(state_manager: &StateManager, color_output: bool) {
    if let Err(e) = state_manager.save() {
        print_warning(
            &format!("Failed to save state cache, continuing anyway: {:#}", e),
            color_output,
        );
    }
}
