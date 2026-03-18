use anyhow::{Context, Result, bail};
use reqwest::Client;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::output::{
    print_self_update_available, print_self_update_current, print_self_update_download,
    print_self_update_start, print_self_update_success,
};

const REPO: &str = "fptbb/fp-appimage-updater";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Detect the release asset suffix for the running architecture.
fn asset_suffix() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("ARM"),
        arch => bail!("Unsupported architecture for self-update: {}", arch),
    }
}

/// 1. Fetch the latest release tag from GitHub.
///
/// When `pre_release` is true, scans the releases list for the first pre-release entry.
/// Otherwise, uses the /releases/latest shortcut which only returns stable releases.
async fn resolve_latest_tag(client: &Client, pre_release: bool) -> Result<String> {
    if pre_release {
        let api_url = format!("https://api.github.com/repos/{}/releases?per_page=1", REPO);
        let releases: serde_json::Value = client
            .get(&api_url)
            .send()
            .await
            .context("Failed to reach GitHub API")?
            .error_for_status()
            .context("GitHub API returned an error")?
            .json()
            .await
            .context("Failed to parse GitHub releases list")?;

        let tag = releases
            .as_array()
            .context("Expected a JSON array from /releases")?
            .first()
            .and_then(|r| r["tag_name"].as_str())
            .context("No releases found on GitHub")?
            .to_string();

        Ok(tag)
    } else {
        let api_url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
        let resp: serde_json::Value = client
            .get(&api_url)
            .send()
            .await
            .context("Failed to reach GitHub API")?
            .error_for_status()
            .context("GitHub API returned an error")?
            .json()
            .await
            .context("Failed to parse GitHub API response")?;

        resp["tag_name"]
            .as_str()
            .context("No tag_name in latest release")
            .map(|s| s.to_string())
    }
}

/// Check GitHub releases and, if a newer version exists, replace the running binary.
pub async fn self_update(client: &Client, pre_release: bool, colors: bool) -> Result<()> {
    let kind = if pre_release { "pre-release" } else { "stable" };
    print_self_update_start(kind, CURRENT_VERSION, colors);

    let latest_tag = resolve_latest_tag(client, pre_release).await?;

    // Normalise to bare semver for comparison: strip leading 'v' and any '-RC<N>' suffix.
    // CURRENT_VERSION is always plain semver (from Cargo.toml), so both sides must match that form.
    let latest_semver = latest_tag
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("");

    if latest_semver == CURRENT_VERSION {
        print_self_update_current(CURRENT_VERSION, colors);
        return Ok(());
    }

    print_self_update_available(CURRENT_VERSION, &latest_tag, colors);

    // 2. Resolve download URL
    let suffix = asset_suffix()?;
    let binary_name = format!("fp-appimage-updater.{}", suffix);
    let download_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, latest_tag, binary_name
    );

    print_self_update_download(&download_url, colors);

    // 3. Download new binary to a temp file
    let mut response = client
        .get(&download_url)
        .send()
        .await
        .context("Failed to download new binary")?
        .error_for_status()
        .context("Download URL returned an error")?;

    let current_binary = env::current_exe().context("Failed to resolve current executable path")?;
    let tmp_path = current_binary.with_extension("tmp");

    {
        let mut tmp_file = fs::File::create(&tmp_path)
            .context("Failed to create temporary file for new binary")?;

        use std::io::Write;
        while let Some(chunk) = response.chunk().await.context("Error while downloading")? {
            tmp_file.write_all(&chunk).context("Failed to write chunk to temp file")?;
        }
    }

    // 4. Make executable, then atomically replace the current binary
    let mut perms = fs::metadata(&tmp_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&tmp_path, perms)?;

    fs::rename(&tmp_path, &current_binary)
        .context("Failed to replace current binary — you may need elevated permissions (are you maybe on an immutable filesystem?)")?;

    print_self_update_success(&latest_tag, colors);
    Ok(())
}
