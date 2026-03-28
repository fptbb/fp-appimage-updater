use anyhow::Result;
use ureq::Agent;

pub mod direct;
pub mod forge;
pub mod script;

use crate::config::{AppConfig, StrategyConfig};
use crate::state::AppState;

#[derive(Debug)]
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
}

pub fn check_for_updates(
    app: &AppConfig,
    state: Option<&AppState>,
    client: &Agent,
) -> Result<CheckResult> {
    match &app.strategy {
        StrategyConfig::Forge {
            repository,
            asset_match,
        } => forge::resolve(&client, repository, asset_match, state),
        StrategyConfig::Direct { url, check_method } => {
            direct::resolve(&client, url, check_method, state)
        }
        StrategyConfig::Script { script_path } => script::resolve(client, app, script_path, state),
    }
}

pub fn capability_from_header_value(name: &str, value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.eq_ignore_ascii_case("bytes") {
        Some(name.to_string())
    } else {
        None
    }
}

pub fn dedupe_capabilities(capabilities: &mut Vec<String>) {
    capabilities.sort();
    capabilities.dedup();
}
