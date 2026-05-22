mod engine;

use crate::output::{UpdateApp, UpdateStatus};

pub use engine::{
    ForcedUpdateInfo, UpdateDownloadJob, UpdateEvent, UpdateWorkResult, adapt_download_limit,
    download_provider_key, estimate_download_bytes, median_speed_bps, should_retry_download_error,
    update_work_elapsed,
};

pub fn filter_update_apps(apps: &[UpdateApp], show_all: bool) -> Vec<UpdateApp> {
    if show_all {
        apps.to_vec()
    } else {
        apps.iter()
            .filter(|app| !matches!(app.status, UpdateStatus::UpToDate))
            .cloned()
            .collect()
    }
}

pub fn effective_show_all(config_show_all: bool, cli_show_all: bool) -> bool {
    config_show_all || cli_show_all
}

pub fn run(
    ctx: &mut crate::commands::helpers::ExecutionContext,
    app_name: Option<&str>,
    show_all: bool,
    forced_update: Option<engine::ForcedUpdateInfo>,
) -> anyhow::Result<()> {
    engine::run(ctx, app_name, show_all, forced_update)
}
