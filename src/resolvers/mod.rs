use anyhow::Result;
use std::time::Duration;

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

pub fn check_for_updates(app: &AppConfig, state: Option<&AppState>) -> Result<Option<UpdateInfo>> {
    let client: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent("fp-appimage-updater/1.0 (+https://fau.fpt.icu/)")
        .build()
        .into();

    match &app.strategy {
        StrategyConfig::Forge { repository, asset_match } => {
            forge::resolve(&client, repository, asset_match, state)
        }
        StrategyConfig::Direct { url, check_method } => {
            direct::resolve(&client, url, check_method, state)
        }
        StrategyConfig::Script { script_path } => {
            script::resolve(app, script_path, state)
        }
    }
}
