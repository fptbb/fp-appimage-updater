/// Self-update flow for the installed binary.
///
/// It resolves the GitLab release, downloads the matching asset, verifies the
/// checksum, and then swaps the current executable in place.
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
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
        "x86_64" => {
            if cfg!(target_env = "musl") {
                Ok("x64-musl")
            } else {
                Ok("x64")
            }
        }
        "aarch64" => {
            if cfg!(target_env = "musl") {
                Ok("ARM-musl")
            } else {
                Ok("ARM")
            }
        }
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
    if is_update_available(&latest_tag) {
        print_self_update_available(CURRENT_VERSION, &latest_tag, colors);
        return Ok(Some(latest_tag));
    }

    Ok(None)
}

fn is_writable_by_user(path: &std::path::Path) -> bool {
    let c_path = match CString::new(path.as_os_str().as_bytes()) {
        Ok(p) => p,
        Err(_) => return false,
    };

    // SAFETY: access() reads a valid null-terminated C string.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
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

fn is_update_available(latest_tag: &str) -> bool {
    let latest_semver = latest_tag
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("");

    latest_semver != CURRENT_VERSION
}

enum SelfUpdatePlan {
    AlreadyCurrent,
    UpdateAvailable,
    UpdateAvailableButBinaryNotWritable,
}

#[derive(Clone, Copy)]
enum SelfUpdateMode {
    Interactive,
    QuietIfCurrent,
}

fn plan_self_update(current_binary: &PathBuf, latest_tag: &str) -> SelfUpdatePlan {
    if !is_update_available(latest_tag) {
        return SelfUpdatePlan::AlreadyCurrent;
    }

    if !is_writable_by_user(current_binary.as_path()) {
        return SelfUpdatePlan::UpdateAvailableButBinaryNotWritable;
    }

    SelfUpdatePlan::UpdateAvailable
}

fn should_print_start_message(mode: SelfUpdateMode) -> bool {
    matches!(mode, SelfUpdateMode::Interactive)
}

fn should_print_current_message(mode: SelfUpdateMode) -> bool {
    matches!(mode, SelfUpdateMode::Interactive)
}

fn self_update_with_mode(
    client: &Agent,
    pre_release: bool,
    colors: bool,
    mode: SelfUpdateMode,
) -> Result<()> {
    let current_binary = env::current_exe().context("Failed to resolve current executable path")?;

    if should_print_start_message(mode) {
        let kind = if pre_release { "pre-release" } else { "stable" };
        print_self_update_start(kind, CURRENT_VERSION, colors);
    }

    let latest_tag = resolve_latest_tag(client, pre_release)?;
    match plan_self_update(&current_binary, &latest_tag) {
        SelfUpdatePlan::AlreadyCurrent => {
            if should_print_current_message(mode) {
                print_self_update_current(CURRENT_VERSION, colors);
            }
            return Ok(());
        }
        SelfUpdatePlan::UpdateAvailableButBinaryNotWritable => {
            print_self_update_available(CURRENT_VERSION, &latest_tag, colors);
            print_warning(
                "The current binary is not writable. Please update it via your package manager or the source where it was installed.",
                colors,
            );
            return Ok(());
        }
        SelfUpdatePlan::UpdateAvailable => {
            print_self_update_available(CURRENT_VERSION, &latest_tag, colors);
        }
    }

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

pub fn self_update(client: &Agent, pre_release: bool, colors: bool) -> Result<()> {
    self_update_with_mode(client, pre_release, colors, SelfUpdateMode::Interactive)
}

pub fn self_update_if_available(client: &Agent, pre_release: bool, colors: bool) -> Result<()> {
    self_update_with_mode(client, pre_release, colors, SelfUpdateMode::QuietIfCurrent)
}

#[cfg(test)]
mod tests {
    use super::{
        SelfUpdateMode, SelfUpdatePlan, is_update_available, plan_self_update,
        should_print_current_message, should_print_start_message,
    };
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn stable_release_matching_current_version_is_not_an_update() {
        assert!(!is_update_available(&format!(
            "v{}",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[test]
    fn prerelease_with_same_semver_is_not_an_update() {
        assert!(!is_update_available(&format!(
            "v{}-RC1",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[test]
    fn newer_release_is_an_update() {
        assert!(is_update_available("v999.0.0"));
    }

    #[test]
    fn unwritable_binary_only_warns_when_update_exists() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let binary_path = temp_dir.path().join("fp-appimage-updater");
        fs::write(&binary_path, b"binary").expect("write test binary");

        let mut permissions = fs::metadata(&binary_path).expect("metadata").permissions();
        permissions.set_mode(0o444);
        fs::set_permissions(&binary_path, permissions).expect("set readonly perms");

        assert!(matches!(
            plan_self_update(&binary_path, &format!("v{}", env!("CARGO_PKG_VERSION"))),
            SelfUpdatePlan::AlreadyCurrent
        ));

        assert!(matches!(
            plan_self_update(&binary_path, "v999.0.0"),
            SelfUpdatePlan::UpdateAvailableButBinaryNotWritable
        ));
    }

    #[test]
    fn quiet_mode_suppresses_routine_self_update_messages() {
        assert!(!should_print_start_message(SelfUpdateMode::QuietIfCurrent));
        assert!(!should_print_current_message(
            SelfUpdateMode::QuietIfCurrent
        ));
        assert!(should_print_start_message(SelfUpdateMode::Interactive));
        assert!(should_print_current_message(SelfUpdateMode::Interactive));
    }
}
