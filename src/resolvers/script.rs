use anyhow::{anyhow, Context, Result};
use std::process::Command;
use crate::state::AppState;
use crate::config::AppConfig;
use super::UpdateInfo;

pub fn resolve(app: &AppConfig, script_path: &str, state: Option<&AppState>) -> Result<Option<UpdateInfo>> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script_path)
        .current_dir(&app.config_dir)
        .output()
        .context("Failed to execute resolver script")?;

    if !output.status.success() {
        return Err(anyhow!("Script execution failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut lines = stdout.lines();

    let download_url = lines.next().context("Script did not output download URL")?.to_string();
    let version = lines.next().unwrap_or("unknown-version").to_string();

    if let Some(s) = state {
        if s.local_version.as_deref() == Some(&version) {
            return Ok(None);
        }
    }

    Ok(Some(UpdateInfo {
        download_url,
        version,
        new_etag: None,
        new_last_modified: None,
    }))
}
