/// Install a downloaded AppImage into the user's environment.
///
/// This handles the executable bit, symlink updates, desktop entry extraction,
/// icon copying, and cleanup of the previous version.
use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, GlobalConfig};

pub fn integrate(
    app: &AppConfig,
    global: &GlobalConfig,
    appimage_path: &Path,
    old_appimage_path: Option<&Path>,
) -> Result<()> {
    // Make executable
    let mut perms = fs::metadata(appimage_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(appimage_path, perms).context("Failed to make AppImage executable")?;

    // Handle symlink
    let should_symlink = app.create_symlink.unwrap_or(global.create_symlinks);
    if should_symlink {
        let symlink_dir = expand_tilde(&global.symlink_dir);
        if !symlink_dir.exists() {
            fs::create_dir_all(&symlink_dir)?;
        }
        let symlink_path = symlink_dir.join(&app.name);

        // Atomic-ish symlink update
        let tmp_symlink = symlink_path.with_extension("tmp_symlink");
        if tmp_symlink.exists() || tmp_symlink.is_symlink() {
            let _ = fs::remove_file(&tmp_symlink);
        }
        symlink(appimage_path, &tmp_symlink).context("Failed to create temporary symlink")?;
        fs::rename(&tmp_symlink, &symlink_path).context("Failed to update symlink")?;
    }

    // Desktop integration is best-effort because some AppImages ship broken metadata.
    let should_integrate = app.integration.unwrap_or(global.manage_desktop_files);
    if should_integrate && let Err(e) = integrate_desktop(app, appimage_path) {
        eprintln!(
            "Warning: Desktop integration failed for {}: {:#}",
            app.name, e
        );
    }

    // Remove the old AppImage only after the new one has been installed successfully.
    if let Some(old_path) = old_appimage_path
        && old_path != appimage_path
        && old_path.exists()
        && let Err(e) = fs::remove_file(old_path)
    {
        eprintln!(
            "Warning: Failed to delete old AppImage {:?}: {}",
            old_path, e
        );
    }

    Ok(())
}

fn integrate_desktop(app: &AppConfig, exec_path: &Path) -> Result<()> {
    let data_local_dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| expand_tilde("~/.local/share"));
    let apps_dir = data_local_dir.join("applications");
    fs::create_dir_all(&apps_dir)?;

    let icon_dest_dir = exec_path
        .parent()
        .map(|p| p.join(".icons"))
        .unwrap_or_default();
    if !icon_dest_dir.as_os_str().is_empty() {
        fs::create_dir_all(&icon_dest_dir)?;
    }

    // Use a unique temp dir for extraction
    let tmp_dir = std::env::temp_dir().join(format!("fp-appimage-int-{}", app.name));
    if tmp_dir.exists() {
        let _ = fs::remove_dir_all(&tmp_dir);
    }
    fs::create_dir_all(&tmp_dir)?;

    // Optimization: we only extract what we need instead of full extract if possible
    // But standard --appimage-extract is more reliable across different appimage types
    // We limit it by extracting to a temp dir and then only picking what we want.
    let status = std::process::Command::new(exec_path)
        .arg("--appimage-extract")
        .current_dir(&tmp_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .context("Failed to run --appimage-extract")?;

    if !status.success() {
        let _ = fs::remove_dir_all(&tmp_dir);
        anyhow::bail!("AppImage extraction failed");
    }

    let extracted_root = tmp_dir.join("squashfs-root");
    if !extracted_root.exists() {
        let _ = fs::remove_dir_all(&tmp_dir);
        anyhow::bail!("Extraction root not found");
    }

    // Find and copy icon
    let mut actual_icon_path = String::new();
    if let Some(icon_path) = find_best_icon(&extracted_root)
        && let Some(ext) = icon_path.extension()
    {
        let final_icon_path = icon_dest_dir.join(format!("{}.{}", app.name, ext.to_string_lossy()));
        if fs::copy(&icon_path, &final_icon_path).is_ok() {
            actual_icon_path = final_icon_path.to_string_lossy().to_string();
        }
    }

    // Find and rewrite desktop file
    if let Some(desktop_path) = find_desktop_file(&extracted_root)
        && let Ok(mut conf) = ini::Ini::load_from_file(&desktop_path)
    {
        if let Some(section) = conf.section_mut(Some("Desktop Entry")) {
            let existing_exec = section.get("Exec").unwrap_or("");
            let args = existing_exec
                .find(' ')
                .map(|idx| &existing_exec[idx..])
                .unwrap_or("");

            section.insert("Exec", format!("{}{}", exec_path.display(), args));
            section.insert("TryExec", exec_path.display().to_string());
            if !actual_icon_path.is_empty() {
                section.insert("Icon", actual_icon_path);
            }
        }
        let target_desktop = apps_dir.join(format!("{}.desktop", app.name));
        let _ = conf.write_to_file(target_desktop);
    }

    let _ = fs::remove_dir_all(&tmp_dir);
    Ok(())
}

fn find_best_icon(root: &Path) -> Option<PathBuf> {
    // Priority: SVG -> PNG
    for ext in ["svg", "png"] {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(ext) {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn find_desktop_file(root: &Path) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                return Some(path);
            }
        }
    }
    None
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(stripped);
    }
    PathBuf::from(path)
}

pub fn rollback(
    app: &AppConfig,
    global: &GlobalConfig,
    failed_new_appimage_path: &Path,
    old_appimage_path: Option<&Path>,
) {
    if failed_new_appimage_path.exists() {
        let _ = fs::remove_file(failed_new_appimage_path);
    }

    if let Some(old_path) = old_appimage_path
        && old_path.exists()
    {
        let _ = integrate(app, global, old_path, None);
    }
}
