use crate::config;
use crate::resolvers;
use crate::state::AppState;
use std::time::Duration;
use super::queue::UpdateErrorStage;

pub enum UpdateWorkResult {
    ReadyToDownload {
        app: config::AppConfig,
        state: AppState,
        from_version: Option<String>,
        current_path: Option<String>,
        info: resolvers::UpdateInfo,
        elapsed: Duration,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
        forge_repository: Option<String>,
        forge_platform: Option<crate::state::ForgePlatform>,
    },
    Updated {
        app: config::AppConfig,
        from_version: Option<String>,
        info: resolvers::UpdateInfo,
        new_path: std::path::PathBuf,
        old_path: Option<std::path::PathBuf>,
        elapsed: Duration,
        downloaded_bytes: u64,
        download_elapsed: Option<Duration>,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
        progress_completion_rendered: bool,
        forge_repository: Option<String>,
        forge_platform: Option<crate::state::ForgePlatform>,
    },
    UpToDate {
        name: String,
        from_version: Option<String>,
        path: Option<String>,
        elapsed: Duration,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
        forge_repository: Option<String>,
        forge_platform: Option<crate::state::ForgePlatform>,
    },
    RateLimited {
        name: String,
        from_version: Option<String>,
        path: Option<String>,
        elapsed: Duration,
        rate_limited_until: Option<u64>,
    },
    Error {
        stage: UpdateErrorStage,
        name: String,
        from_version: Option<String>,
        to_version: Option<String>,
        path: Option<String>,
        elapsed: Duration,
        capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
        rate_limited_until: Option<u64>,
        error: String,
        forge_repository: Option<String>,
        forge_platform: Option<crate::state::ForgePlatform>,
        retry_job: Option<UpdateDownloadJob>,
    },
}

pub enum UpdateEvent {
    Check(UpdateWorkResult),
    Download {
        provider: String,
        result: UpdateWorkResult,
    },
}

pub struct UpdateDownloadJob {
    pub app: config::AppConfig,
    pub state: AppState,
    pub from_version: Option<String>,
    pub current_path: Option<String>,
    pub info: resolvers::UpdateInfo,
    pub capabilities: Vec<String>,
    pub segmented_downloads: Option<bool>,
    pub estimated_download_bytes: Option<u64>,
    pub provider: String,
    pub forge_repository: Option<String>,
    pub forge_platform: Option<crate::state::ForgePlatform>,
    pub retry_without_segmented_downloads: bool,
}
