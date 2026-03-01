use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

pub mod direct;
pub mod forge;
pub mod script;

use crate::config::{AppConfig, StrategyConfig};
use crate::state::AppState;

pub struct UpdateInfo {
    pub download_url: String,
    pub version: String,
    pub new_etag: Option<String>,
    pub new_last_modified: Option<String>,
}

pub async fn check_for_updates(app: &AppConfig, state: Option<&AppState>) -> Result<Option<UpdateInfo>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("fp-appimage-updater/1.0")
        .build()?;

    match &app.strategy {
        StrategyConfig::Forge { repository, asset_match } => {
            forge::resolve(&client, repository, asset_match, state).await
        }
        StrategyConfig::Direct { url, check_method } => {
            direct::resolve(&client, url, check_method, state).await
        }
        StrategyConfig::Script { script_path } => {
            script::resolve(app, script_path, state)
        }
    }
}
