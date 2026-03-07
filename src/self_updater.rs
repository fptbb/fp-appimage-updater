use anyhow::{Context, Result, bail};
use reqwest::Client;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

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

/// Check GitHub releases and, if a newer version exists, replace the running binary.
pub async fn self_update(client: &Client) -> Result<()> {
    println!("Checking for updates to fp-appimage-updater (current: v{})...", CURRENT_VERSION);

    // 1. Fetch latest release tag from GitHub API
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

    let latest_tag = resp["tag_name"]
        .as_str()
        .context("No tag_name in latest release")?;

    // Strip leading 'v' for comparison
    let latest_version = latest_tag.trim_start_matches('v');

    if latest_version == CURRENT_VERSION {
        println!("fp-appimage-updater is already up to date (v{}).", CURRENT_VERSION);
        return Ok(());
    }

    println!("New version available: {} → {}", CURRENT_VERSION, latest_tag);

    // 2. Resolve download URL
    let suffix = asset_suffix()?;
    let binary_name = format!("fp-appimage-updater.{}", suffix);
    let download_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        REPO, latest_tag, binary_name
    );

    println!("Downloading from {}...", download_url);

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

    println!("Successfully updated to {}!", latest_tag);
    Ok(())
}
