/// Self-update flow for the installed binary.
///
/// It resolves the GitLab release, downloads the matching asset, verifies the
/// checksum, and then swaps the current executable in place.
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use ureq::Agent;

use crate::output::{
    print_progress, print_self_update_available, print_self_update_current,
    print_self_update_download, print_self_update_start, print_self_update_success, print_warning,
};

const REPO: &str = "fpsys/fp-appimage-updater";
const REPO_ENCODED: &str = "fpsys%2Ffp-appimage-updater";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn asset_suffix() -> Result<&'static str> {
    // The release assets are published with architecture-specific suffixes.
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("ARM"),
        arch => bail!("Unsupported architecture for self-update: {}", arch),
    }
}

fn resolve_latest_tag(client: &Agent, pre_release: bool) -> Result<String> {
    if pre_release {
        let api_url = format!(
            "https://gitlab.com/api/v4/projects/{}/releases?order_by=released_at&sort=desc&per_page=100",
            REPO_ENCODED
        );
        let releases: serde_json::Value = client
            .get(&api_url)
            .call()
            .context("Failed to reach GitLab API and parse releases list")?
            .into_body()
            .read_json()
            .context("Failed to parse GitLab releases list")?;

        let tag = releases
            .as_array()
            .context("Expected a JSON array from /releases")?
            .iter()
            .filter_map(|release| release["tag_name"].as_str())
            .find(|tag| tag.contains("-RC"))
            .context("No pre-releases found on GitLab")?
            .to_string();

        Ok(tag)
    } else {
        let api_url = format!(
            "https://gitlab.com/api/v4/projects/{}/releases/permalink/latest",
            REPO_ENCODED
        );
        let resp: serde_json::Value = client
            .get(&api_url)
            .call()
            .context("Failed to reach GitLab API and parse response")?
            .into_body()
            .read_json()
            .context("Failed to parse GitLab API response")?;

        resp["tag_name"]
            .as_str()
            .context("No tag_name in latest release")
            .map(|s| s.to_string())
    }
}

pub fn check_for_update(client: &Agent, pre_release: bool, colors: bool) -> Result<Option<String>> {
    let latest_tag = resolve_latest_tag(client, pre_release)?;
    let latest_semver = latest_tag
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("");

    if latest_semver != CURRENT_VERSION {
        print_self_update_available(CURRENT_VERSION, &latest_tag, colors);
        return Ok(Some(latest_tag));
    }

    Ok(None)
}

fn is_writable_by_user(path: &PathBuf) -> bool {
    use std::os::unix::fs::MetadataExt;
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let uid = unsafe { libc::getuid() };
    if uid == 0 {
        return true;
    } // root can write anywhere usually

    if metadata.uid() == uid {
        return metadata.permissions().mode() & 0o200 != 0;
    }

    false
}

fn verify_checksum(binary_path: &PathBuf, checksums_content: &str, asset_name: &str) -> Result<()> {
    let mut file = fs::File::open(binary_path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);

    for line in checksums_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 && parts[1] == asset_name {
            if parts[0] == hash_hex {
                return Ok(());
            } else {
                bail!(
                    "Checksum mismatch for {}: expected {}, found {}",
                    asset_name,
                    parts[0],
                    hash_hex
                );
            }
        }
    }

    bail!("No checksum found for {} in checksums.txt", asset_name);
}

pub fn self_update(client: &Agent, pre_release: bool, colors: bool) -> Result<()> {
    let current_binary = env::current_exe().context("Failed to resolve current executable path")?;

    // Safety check: ensure we are not trying to update a system-owned binary
    if !is_writable_by_user(&current_binary) {
        print_warning(
            "The current binary is not writable by the user. It might be installed in a system directory or managed by a package manager. Self-update aborted.",
            colors,
        );
        return Ok(());
    }

    let kind = if pre_release { "pre-release" } else { "stable" };
    print_self_update_start(kind, CURRENT_VERSION, colors);

    let latest_tag = resolve_latest_tag(client, pre_release)?;
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

    let suffix = asset_suffix()?;
    let binary_name = format!("fp-appimage-updater.{}", suffix);
    let download_url = format!(
        "https://gitlab.com/{}/-/releases/{}/downloads/bin/{}",
        REPO, latest_tag, binary_name
    );
    let checksums_url = format!(
        "https://gitlab.com/{}/-/releases/{}/downloads/bin/checksums.txt",
        REPO, latest_tag
    );

    print_self_update_download(&download_url, colors);

    // Download binary
    let response = client
        .get(&download_url)
        .call()
        .context("Failed to download new binary from GitLab release asset")?;

    let tmp_path = current_binary.with_extension("tmp");
    {
        let mut tmp_file = fs::File::create(&tmp_path)
            .context("Failed to create temporary file for new binary")?;
        let mut reader = response.into_body().into_reader();
        let mut buffer = [0u8; 8192];
        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            tmp_file.write_all(&buffer[..bytes_read])?;
        }
    }

    // Download and verify checksum
    print_progress("Verifying checksum...", colors);
    let checksum_resp = client
        .get(&checksums_url)
        .call()
        .context("Failed to download checksums.txt")?;
    let mut checksums_content = String::new();
    checksum_resp
        .into_body()
        .into_reader()
        .read_to_string(&mut checksums_content)?;

    if let Err(e) = verify_checksum(&tmp_path, &checksums_content, &binary_name) {
        let _ = fs::remove_file(&tmp_path);
        bail!("Integrity check failed: {}", e);
    }

    let mut perms = fs::metadata(&tmp_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&tmp_path, perms)?;

    fs::rename(&tmp_path, &current_binary).context("Failed to replace current binary")?;

    print_self_update_success(&latest_tag, colors);
    Ok(())
}
