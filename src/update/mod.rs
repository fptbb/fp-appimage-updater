mod engine;

pub use engine::{
    adapt_download_limit, download_provider_key, estimate_download_bytes, median_speed_bps,
    should_retry_download_error, update_work_elapsed, UpdateDownloadJob, UpdateEvent,
    UpdateWorkResult,
};

pub fn run(
    app_configs: &[crate::config::AppConfig],
    app_config_errors: &[crate::parser::AppConfigLoadError],
    global_config: &crate::config::GlobalConfig,
    state_manager: &mut crate::state::StateManager,
    client: &ureq::Agent,
    app_name: Option<&str>,
    json_output: bool,
    color_output: bool,
) -> anyhow::Result<()> {
    engine::run(
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
