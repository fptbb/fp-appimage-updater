use super::{CheckResult, UpdateInfo, dedupe_capabilities};
use crate::config::AppConfig;
use crate::state::AppState;
use anyhow::{Context, Result, anyhow};
use std::process::Command;
use ureq::Agent;

pub fn resolve(
    client: &Agent,
    app: &AppConfig,
    script_path: &str,
    state: Option<&AppState>,
) -> Result<CheckResult> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script_path)
        .current_dir(&app.config_dir)
        .output()
        .with_context(|| {
            format!(
                "Failed to execute resolver script for {} in {}",
                app.name,
                app.config_dir.display()
            )
        })?;

    if !output.status.success() {
        let exit_desc = output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        return Err(anyhow!(
            "Resolver script for '{}' failed (exit: {}).\nscript: {}\ncwd: {}\nstdout:\n{}\nstderr:\n{}",
            app.name,
            exit_desc,
            script_path,
            app.config_dir.display(),
            if stdout.trim().is_empty() {
                "<empty>"
            } else {
                &stdout
            },
            if stderr.trim().is_empty() {
                "<empty>"
            } else {
                &stderr
            },
        ));
    }

    let stdout = String::from_utf8(output.stdout).with_context(|| {
        format!(
            "Resolver script for '{}' did not output valid UTF-8",
            app.name
        )
    })?;
    let mut lines = stdout.lines();

    let download_url = lines
        .next()
        .context("Script did not output a download URL on the first line")?
        .trim()
        .to_string();
    let version = lines
        .next()
        .context("Script did not output a version on the second line")?
        .trim()
        .to_string();

    if download_url.is_empty() {
        return Err(anyhow!(
            "Resolver script for '{}' returned an empty download URL.\nscript: {}\ncwd: {}",
            app.name,
            script_path,
            app.config_dir.display()
        ));
    }

    if version.is_empty() {
        return Err(anyhow!(
            "Resolver script for '{}' returned an empty version string.\nscript: {}\ncwd: {}",
            app.name,
            script_path,
            app.config_dir.display()
        ));
    }

    let mut capabilities = Vec::new();
    let mut segmented_downloads = None;
    if let Ok(head_resp) = client.head(&download_url).call() {
        segmented_downloads = Some(false);
        if let Some(range_header) = head_resp
            .headers()
            .get("Accept-Ranges")
            .and_then(|value| value.to_str().ok())
            && range_header.trim().eq_ignore_ascii_case("bytes")
        {
            capabilities.push("segmented_downloads".to_string());
            segmented_downloads = Some(true);
        }
    }
    dedupe_capabilities(&mut capabilities);

    let update = if state.and_then(|s| s.local_version.as_deref()) == Some(version.as_str()) {
        None
    } else {
        Some(UpdateInfo {
            download_url,
            version,
            new_etag: None,
            new_last_modified: None,
        })
    };

    Ok(CheckResult {
        update,
        capabilities,
        segmented_downloads,
        forge_repository: None,
        forge_platform: None,
    })
}
