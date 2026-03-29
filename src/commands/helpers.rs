use crate::config;
use crate::resolvers;
use crate::state::{AppState, StateManager};
use std::time::Duration;

pub const MAX_CONCURRENT_CHECK_JOBS: usize = 3;
pub const MAX_CONCURRENT_DOWNLOADS: usize = 6;
pub const FAST_JOB_SECONDS: f64 = 2.0;
pub const SLOW_JOB_SECONDS: f64 = 15.0;
const ALL_GITHUB_PROXY_PREFIXES: [&str; 3] = [
    "https://gh-proxy.com/",
    "https://corsproxy.io/?",
    "https://api.allorigins.win/raw?url=",
];

pub fn now_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn rate_limit_enabled(app: &config::AppConfig, global: &config::GlobalConfig) -> bool {
    app.respect_rate_limits
        .unwrap_or(global.respect_rate_limits)
}

pub fn github_proxy_enabled(app: &config::AppConfig, global: &config::GlobalConfig) -> bool {
    app.github_proxy.unwrap_or(global.github_proxy)
}

pub fn github_proxy_prefixes(
    app: &config::AppConfig,
    global: &config::GlobalConfig,
) -> Vec<String> {
    let prefixes = app
        .github_proxy_prefix
        .clone()
        .unwrap_or_else(|| global.github_proxy_prefix.clone())
        .into_vec();

    if prefixes
        .iter()
        .any(|prefix| prefix.trim().eq_ignore_ascii_case("all"))
    {
        return ALL_GITHUB_PROXY_PREFIXES
            .iter()
            .map(|prefix| prefix.to_string())
            .collect();
    }

    prefixes
        .into_iter()
        .map(|prefix| prefix.trim().to_string())
        .filter(|prefix| !prefix.is_empty())
        .collect()
}

pub fn app_uses_github_forge(app: &config::AppConfig) -> bool {
    matches!(
        &app.strategy,
        config::StrategyConfig::Forge { repository, .. } if repository.starts_with("https://github.com/")
    )
}

pub fn clear_expired_rate_limit(state: &mut AppState, now: u64) {
    if matches!(state.rate_limited_until, Some(until) if until <= now) {
        state.rate_limited_until = None;
    }
}

pub fn snapshot_app_state(state_manager: &mut StateManager, app_name: &str, now: u64) -> AppState {
    let state = state_manager.get_app_mut(app_name);
    clear_expired_rate_limit(state, now);
    state.clone()
}

pub fn rate_limit_note() -> &'static str {
    "Rate limit hit. Set respect_rate_limits: false globally or per app to keep trying."
}

pub fn rate_limit_from_error(error: &anyhow::Error) -> Option<resolvers::RateLimitInfo> {
    error.downcast_ref::<resolvers::RateLimitInfo>().cloned()
}

pub fn adapt_worker_limit(
    current: usize,
    elapsed: Duration,
    pending: usize,
    hard_max: usize,
) -> usize {
    let mut next = current;
    let elapsed_secs = elapsed.as_secs_f64();

    if elapsed_secs <= FAST_JOB_SECONDS && pending > current && next < hard_max {
        next += 1;
    } else if elapsed_secs >= SLOW_JOB_SECONDS && next > 1 {
        next -= 1;
    }

    next.clamp(1, hard_max)
}

pub fn cache_app_metadata(
    state: &mut AppState,
    capabilities: Vec<String>,
    segmented_downloads: Option<bool>,
) {
    let mut capabilities = capabilities;
    resolvers::dedupe_capabilities(&mut capabilities);
    state.capabilities = capabilities;
    if let Some(segmented_downloads) = segmented_downloads {
        state.segmented_downloads = Some(segmented_downloads);
    }
}

pub fn build_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent("fp-appimage-updater/1.0")
        .build()
        .into()
}

pub fn matches_target(target: Option<&str>, app_name: Option<&str>) -> bool {
    match target {
        Some(target) => app_name == Some(target),
        None => true,
    }
}
