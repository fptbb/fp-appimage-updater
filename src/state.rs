/// On-disk cache for the last known download state.
///
/// This is intentionally lightweight and opportunistic: the app rebuilds it
/// from successful runs instead of treating it as source-of-truth config.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForgePlatform {
    GitHub,
    GitLab,
    Gitea,
    Forgejo,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub local_version: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limited_until: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segmented_downloads: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge_repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge_platform: Option<ForgePlatform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_bytes: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub apps: HashMap<String, AppState>,
}

pub struct StateManager {
    cache_path: PathBuf,
    pub state: State,
}

impl StateManager {
    pub fn load(cache_path: impl AsRef<Path>) -> Self {
        let cache_path = cache_path.as_ref().to_path_buf();
        let state = fs::read_to_string(&cache_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default();

        Self { cache_path, state }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.state)?;
        let tmp_path = self.cache_path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&tmp_path, &self.cache_path)?;
        Ok(())
    }

    pub fn get_app(&self, name: &str) -> Option<&AppState> {
        self.state.apps.get(name)
    }

    pub fn get_app_mut(&mut self, name: &str) -> &mut AppState {
        self.state.apps.entry(name.to_string()).or_default()
    }
}
