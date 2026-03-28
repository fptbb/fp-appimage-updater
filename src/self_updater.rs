use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use ureq::Agent;

use crate::output::{
    print_self_update_available, print_self_update_current, print_self_update_download,
    print_self_update_start, print_self_update_success,
};

const REPO: &str = "fpsys/fp-appimage-updater";
const REPO_ENCODED: &str = "fpsys%2Ffp-appimage-updater";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn asset_suffix() -> Result<&'static str> {
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

pub fn self_update(client: &Agent, pre_release: bool, colors: bool) -> Result<()> {
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
    let fallback_download_url = format!(
        "https://gitlab.com/{}/-/jobs/artifacts/main/raw/build/{}?job=build-and-compress",
        REPO, binary_name
    );

    print_self_update_download(&download_url, colors);

    let response = client.get(&download_url).call().or_else(|_| {
        client
            .get(&fallback_download_url)
            .call()
            .context("Failed to download new binary from GitLab release asset or fallback artifact")
    })?;

    let current_binary = env::current_exe().context("Failed to resolve current executable path")?;
    let tmp_path = current_binary.with_extension("tmp");

    {
        let mut tmp_file = fs::File::create(&tmp_path)
            .context("Failed to create temporary file for new binary")?;

        std::io::copy(&mut response.into_body().into_reader(), &mut tmp_file)
            .context("Failed to write buffer to temp file")?;
    }

    let mut perms = fs::metadata(&tmp_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&tmp_path, perms)?;

    fs::rename(&tmp_path, &current_binary)
        .context("Failed to replace current binary — you may need elevated permissions (are you maybe on an immutable filesystem?)")?;

    print_self_update_success(&latest_tag, colors);
    Ok(())
}
