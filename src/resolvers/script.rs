use crate::config::AppConfig;
use crate::state::AppState;
use anyhow::{anyhow, Context, Result};
use super::UpdateInfo;
use std::process::Command;

pub fn resolve(
    app: &AppConfig,
    script_path: &str,
    state: Option<&AppState>,
) -> Result<Option<UpdateInfo>> {
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
            if stdout.trim().is_empty() { "<empty>" } else { &stdout },
            if stderr.trim().is_empty() { "<empty>" } else { &stderr },
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("Resolver script for '{}' did not output valid UTF-8", app.name))?;
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

    if let Some(s) = state && s.local_version.as_deref() == Some(&version) {
        return Ok(None);
    }

    Ok(Some(UpdateInfo {
        download_url,
        version,
        new_etag: None,
        new_last_modified: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{StrategyConfig};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_dir() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("fp-appimage-updater-test-{}", unique));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    #[test]
    fn script_failure_reports_full_context() {
        let config_dir = temp_config_dir();
        let script_path = config_dir.join("resolver.sh");

        fs::write(
            &script_path,
            "#!/bin/sh\necho \"this script is broken\"\necho \"and exits non-zero\" >&2\nexit 42\n",
        )
        .expect("failed to write script");

        let mut perms = fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).unwrap();

        let app = AppConfig {
            config_dir: config_dir.clone(),
            name: "broken-script".to_string(),
            zsync: None,
            integration: None,
            create_symlink: None,
            storage_dir: None,
            strategy: StrategyConfig::Script {
                script_path: "./resolver.sh".to_string(),
            },
        };

        let err = resolve(&app, "./resolver.sh", None)
            .expect_err("expected script failure");

        let message = format!("{:#}", err);
        assert!(message.contains("Resolver script for 'broken-script' failed"));
        assert!(message.contains("exit: 42"));
        assert!(message.contains("stdout:"));
        assert!(message.contains("stderr:"));
    }
}
