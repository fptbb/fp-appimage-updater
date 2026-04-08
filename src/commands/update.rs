use crate::config;
use crate::parser::AppConfigLoadError;
use crate::state::StateManager;
use anyhow::Result;

pub fn run(
    app_configs: &[config::AppConfig],
    app_config_errors: &[AppConfigLoadError],
    global_config: &config::GlobalConfig,
    state_manager: &mut StateManager,
    client: &ureq::Agent,
    app_name: Option<&str>,
    json_output: bool,
    color_output: bool,
) -> Result<()> {
    crate::update::run(
        app_configs,
        app_config_errors,
        global_config,
        state_manager,
        client,
        app_name,
        json_output,
        color_output,
    )
}
