use anyhow::Result;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use ureq::Agent;

pub mod direct;
pub mod forge;
pub mod script;

use crate::config::GlobalConfig;
use crate::config::{AppConfig, StrategyConfig};
use crate::state::{AppState, ForgePlatform};

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub download_url: String,
    pub version: String,
    pub new_etag: Option<String>,
    pub new_last_modified: Option<String>,
}

#[derive(Debug, Default)]
pub struct CheckResult {
    pub update: Option<UpdateInfo>,
    pub capabilities: Vec<String>,
    pub segmented_downloads: Option<bool>,
    pub forge_repository: Option<String>,
    pub forge_platform: Option<ForgePlatform>,
}

#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    pub reset_at: Option<u64>,
    pub retry_after_seconds: Option<u64>,
}

impl RateLimitInfo {
    pub fn until_epoch_seconds(&self) -> Option<u64> {
        self.retry_after_seconds
            .map(|retry| now_seconds().saturating_add(retry))
            .or(self.reset_at)
    }

    pub fn short_message(&self) -> String {
        match self.until_epoch_seconds() {
            Some(until) => {
                let wait = until.saturating_sub(now_seconds());
                if wait < 60 {
                    format!("Rate limited. Retry in {}s.", wait)
                } else if wait < 3600 {
                    format!("Rate limited. Retry in {}m.", wait / 60)
                } else {
                    format!(
                        "Rate limited. Retry in {}h {}m.",
                        wait / 3600,
                        (wait % 3600) / 60
                    )
                }
            }
            None => "Rate limited. Retry later.".to_string(),
        }
    }
}

impl fmt::Display for RateLimitInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.short_message())
    }
}

impl std::error::Error for RateLimitInfo {}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn rate_limit_info_from_headers(headers: &ureq::http::HeaderMap) -> Option<RateLimitInfo> {
    let reset_after = header_u64_any(headers, &["ratelimit-reset", "RateLimit-Reset"]);
    let reset_at = reset_after
        .map(|s| now_seconds().saturating_add(s))
        .or_else(|| header_u64_any(headers, &["x-ratelimit-reset", "X-RateLimit-Reset"]));

    let retry_after_seconds = header_u64_any(headers, &["retry-after", "Retry-After"]);

    if reset_at.is_none() && retry_after_seconds.is_none() {
        None
    } else {
        Some(RateLimitInfo {
            reset_at,
            retry_after_seconds,
        })
    }
}

pub fn check_for_updates(
    app: &AppConfig,
    state: Option<&AppState>,
    client: &Agent,
    github_proxy: bool,
    github_proxy_prefixes: &[String],
    global_config: &GlobalConfig,
) -> Result<CheckResult> {
    match &app.strategy {
        StrategyConfig::Forge {
            repository,
            asset_match,
            asset_match_regex,
        } => forge::resolve(
            client,
            repository,
            asset_match,
            asset_match_regex.as_deref(),
            state,
            github_proxy,
            github_proxy_prefixes,
            global_config,
        ),
        StrategyConfig::Direct { url, check_method } => {
            direct::resolve(client, url, check_method, state)
        }
        StrategyConfig::Script { script_path } => script::resolve(client, app, script_path, state),
    }
}

pub fn capability_from_header_value(name: &str, value: Option<&str>) -> Option<String> {
    value?
        .trim()
        .eq_ignore_ascii_case("bytes")
        .then(|| name.to_string())
}

fn header_u64_any(headers: &ureq::http::HeaderMap, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| headers.get(*name))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
}

pub fn dedupe_capabilities(capabilities: &mut Vec<String>) {
    capabilities.sort();
    capabilities.dedup();
}
