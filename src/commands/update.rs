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
    debug_download_url: Option<&str>,
    debug_version: Option<&str>,
    json_output: bool,
    color_output: bool,
) -> Result<()> {
    let forced_update = match (debug_download_url, debug_version) {
        (Some(download_url), Some(version)) => {
            if app_name.is_none() {
                anyhow::bail!("debug downgrade requires specifying an app name");
            }
            Some(crate::update::ForcedUpdateInfo {
                download_url: download_url.to_string(),
                version: version.to_string(),
            })
        }
        (None, None) => None,
        _ => {
            anyhow::bail!("debug downgrade requires both --debug-download-url and --debug-version")
        }
    };

    crate::update::run(
        app_configs,
        app_config_errors,
        global_config,
        state_manager,
        client,
        app_name,
        forced_update,
        json_output,
        color_output,
    )
}
