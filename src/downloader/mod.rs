use anyhow::{Context, Result};
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ureq::Agent;

pub mod http;
pub mod progress;

pub use http::*;
pub use progress::*;

use crate::config::{AppConfig, ZsyncConfig};
use crate::resolvers::UpdateInfo;
use crate::state::AppState;

#[derive(Debug)]
pub struct DownloadResult {
    pub path: PathBuf,
    pub segmented_downloads: Option<bool>,
    pub progress_completion_rendered: bool,
    pub downloaded_bytes: u64,
    pub download_elapsed: Option<Duration>,
}

pub fn suspend_for_print<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    if let Ok(mut ui) = progress_ui().lock() {
        let _ = ui.clear_rendered();
        let result = f();
        let _ = ui.draw();
        result
    } else {
        f()
    }
}

pub fn finalize_progress_output() -> Result<()> {
    if let Ok(mut ui) = progress_ui().lock() {
        ui.clear_all()?;
    }
    Ok(())
}

pub fn download_app(
    client: &Agent,
    app: &AppConfig,
    update_info: &UpdateInfo,
    storage_dir: &Path,
    naming_format: &str,
    state: Option<&AppState>,
    segmented_downloads: bool,
    quiet: bool,
    colors: bool,
) -> Result<DownloadResult> {
    let actual_storage_dir = app
        .storage_dir
        .as_ref()
        .map(|s| crate::integrator::expand_tilde(s))
        .unwrap_or_else(|| storage_dir.to_path_buf());

    let file_name = naming_format
        .replace("{name}", &app.name)
        .replace("{version}", &update_info.version);

    let final_path = actual_storage_dir.join(&file_name);
    let tmp_path = actual_storage_dir.join(format!("{}.tmp", file_name));

    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // zsync is a delta path: try an existing AppImage first, then fall back to HTTP.
    let zsync_url = match &app.zsync {
        Some(ZsyncConfig::Enabled(true)) => Some(format!("{}.zsync", update_info.download_url)),
        Some(ZsyncConfig::Url(url)) => Some(url.clone()),
        _ => None,
    };

    let mut zsync_success = false;
    let mut segmented_downloads_support = state.and_then(|s| s.segmented_downloads);

    if let Some(zurl) = zsync_url
        && let Some(old_path_str) = state.and_then(|s| s.file_path.as_ref())
    {
        let old_path = Path::new(old_path_str);
        if old_path.exists() && try_zsync(&zurl, old_path, &tmp_path, quiet, colors) {
            zsync_success = true;
        }
    }

    if !zsync_success {
        let download_started = Instant::now();
        let mut progress_completion_rendered = false;
        let (segmented_success, segmented_support, segmented_progress_displayed) =
            if segmented_downloads {
                try_segmented_http_download(
                    client,
                    &app.name,
                    &update_info.version,
                    &update_info.download_url,
                    &tmp_path,
                    segmented_downloads_support,
                    quiet,
                    colors,
                )
            } else {
                (false, segmented_downloads_support, false)
            };
        segmented_downloads_support = segmented_support;
        progress_completion_rendered |= segmented_progress_displayed;

        if !segmented_success {
            let (_download_progress_displayed, download_progress_completion_rendered) =
                download_http(
                    client,
                    &app.name,
                    &update_info.version,
                    &update_info.download_url,
                    &tmp_path,
                    quiet,
                    colors,
                )?;
            progress_completion_rendered |= download_progress_completion_rendered;
        }

        if let Err(err) = ensure_downloaded_appimage_matches_host(&tmp_path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(err);
        }

        std::fs::rename(&tmp_path, &final_path)
            .context("Failed to rename tmp file to final destination")?;

        let downloaded_bytes = fs::metadata(&final_path)
            .map(|meta| meta.len())
            .unwrap_or(0);

        return Ok(DownloadResult {
            path: final_path,
            segmented_downloads: segmented_downloads_support,
            progress_completion_rendered,
            downloaded_bytes,
            download_elapsed: Some(download_started.elapsed()),
        });
    }

    if let Err(err) = ensure_downloaded_appimage_matches_host(&tmp_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    std::fs::rename(&tmp_path, &final_path)
        .context("Failed to rename tmp file to final destination")?;

    let downloaded_bytes = fs::metadata(&final_path)
        .map(|meta| meta.len())
        .unwrap_or(0);

    Ok(DownloadResult {
        path: final_path,
        segmented_downloads: segmented_downloads_support,
        progress_completion_rendered: false,
        downloaded_bytes,
        download_elapsed: None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElfMachineArch {
    X86_64,
    AArch64,
    X86,
    Arm,
    Riscv64,
    PowerPc64,
}

impl ElfMachineArch {
    fn label(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::AArch64 => "aarch64",
            Self::X86 => "x86",
            Self::Arm => "arm",
            Self::Riscv64 => "riscv64",
            Self::PowerPc64 => "powerpc64",
        }
    }
}

fn ensure_downloaded_appimage_matches_host(path: &Path) -> Result<()> {
    let Some(detected_arch) = detect_elf_machine_arch(path)? else {
        return Ok(());
    };
    let host_arch = host_elf_machine_arch()?;

    if detected_arch != host_arch {
        anyhow::bail!(
            "Downloaded AppImage targets {}, but this machine is {}. The update was skipped and the existing AppImage was left unchanged.",
            detected_arch.label(),
            host_arch.label()
        );
    }

    Ok(())
}

fn host_elf_machine_arch() -> Result<ElfMachineArch> {
    match std::env::consts::ARCH {
        "x86_64" => Ok(ElfMachineArch::X86_64),
        "aarch64" => Ok(ElfMachineArch::AArch64),
        "x86" => Ok(ElfMachineArch::X86),
        "arm" => Ok(ElfMachineArch::Arm),
        "riscv64" => Ok(ElfMachineArch::Riscv64),
        "powerpc64" => Ok(ElfMachineArch::PowerPc64),
        arch => anyhow::bail!(
            "Unsupported host architecture for AppImage validation: {}",
            arch
        ),
    }
}

fn detect_elf_machine_arch(path: &Path) -> Result<Option<ElfMachineArch>> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open downloaded AppImage {}", path.display()))?;
    let mut header = [0u8; 20];
    if let Err(err) = file.read_exact(&mut header) {
        if err.kind() == ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(err)
            .with_context(|| format!("Failed to read ELF header from {}", path.display()));
    }
    detect_elf_machine_arch_from_bytes(&header)
        .map(Some)
        .or_else(|err| {
            if format!("{:#}", err).contains("not an ELF executable") {
                Ok(None)
            } else {
                Err(err)
            }
        })
}

fn detect_elf_machine_arch_from_bytes(header: &[u8]) -> Result<ElfMachineArch> {
    if header.len() < 20 || &header[..4] != b"\x7FELF" {
        anyhow::bail!("Downloaded file is not an ELF executable");
    }

    let machine = match header[5] {
        1 => u16::from_le_bytes([header[18], header[19]]),
        2 => u16::from_be_bytes([header[18], header[19]]),
        other => anyhow::bail!("Unsupported ELF data encoding: {}", other),
    };

    let arch = match machine {
        3 => ElfMachineArch::X86,
        40 => ElfMachineArch::Arm,
        62 => ElfMachineArch::X86_64,
        183 => ElfMachineArch::AArch64,
        21 => ElfMachineArch::PowerPc64,
        243 => ElfMachineArch::Riscv64,
        other => anyhow::bail!("Unsupported ELF machine type: {}", other),
    };

    Ok(arch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf_header_for_machine(machine: u16) -> [u8; 20] {
        let mut header = [0u8; 20];
        header[..4].copy_from_slice(b"\x7FELF");
        header[4] = 2;
        header[5] = 1;
        header[18..20].copy_from_slice(&machine.to_le_bytes());
        header
    }

    #[test]
    fn detects_x86_64_elf_machine() {
        let arch =
            detect_elf_machine_arch_from_bytes(&elf_header_for_machine(62)).expect("missing arch");
        assert_eq!(arch, ElfMachineArch::X86_64);
    }

    #[test]
    fn detects_aarch64_elf_machine() {
        let arch =
            detect_elf_machine_arch_from_bytes(&elf_header_for_machine(183)).expect("missing arch");
        assert_eq!(arch, ElfMachineArch::AArch64);
    }

    #[test]
    fn skips_non_elf_files() {
        let err = detect_elf_machine_arch_from_bytes(b"not an elf")
            .expect_err("expected non-elf file to fail");
        assert!(format!("{:#}", err).contains("not an ELF executable"));
    }
}
