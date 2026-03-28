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
    let mut perms = fs::metadata(appimage_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(appimage_path, perms).context("Failed to make AppImage executable")?;

    let should_symlink = app.create_symlink.unwrap_or(global.create_symlinks);

    let symlink_dir = expand_tilde(&global.symlink_dir);
    if !symlink_dir.exists() {
        fs::create_dir_all(&symlink_dir)?;
    }
    let symlink_path = symlink_dir.join(&app.name);

    if symlink_path.exists() || symlink_path.is_symlink() {
        fs::remove_file(&symlink_path).context("Failed to remove old symlink")?;
    }

    if should_symlink {
        symlink(appimage_path, &symlink_path).context("Failed to create symlink")?;
    }

    let should_integrate = app.integration.unwrap_or(global.manage_desktop_files);
    if should_integrate {
        integrate_desktop(app, appimage_path)?;
    }

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

    let mut icon_dest_path = PathBuf::new();
    let mut actual_icon_path = String::new();

    if let Some(parent_dir) = exec_path.parent() {
        let appimages_icons_dir = parent_dir.join(".icons");
        fs::create_dir_all(&appimages_icons_dir)?;
        icon_dest_path = appimages_icons_dir;
    }

    let tmp_dir = std::env::temp_dir().join(format!("fp-appimage-extractor-{}", app.name));
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir)?;
    }
    fs::create_dir_all(&tmp_dir)?;

    let output = std::process::Command::new(exec_path)
        .arg("--appimage-extract")
        .current_dir(&tmp_dir)
        .output()
        .with_context(|| {
            format!(
                "Failed to run --appimage-extract on AppImage {:?}",
                exec_path
            )
        })?;

    if !output.status.success() {
        eprintln!(
            "Warning: Failed to extract internal desktop/icon from {}, the .desktop will not be generated.",
            app.name
        );
        eprintln!(
            "Extraction error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = fs::remove_dir_all(&tmp_dir);
        return Ok(());
    }

    let extracted_root = tmp_dir.join("squashfs-root");

    let mut extracted_icon: Option<PathBuf> = None;
    if extracted_root.exists() {
        for ext in ["png", "svg"] {
            if let Ok(entries) = fs::read_dir(&extracted_root) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some(ext) {
                        extracted_icon = Some(path);
                        break;
                    }
                }
            }
            if extracted_icon.is_some() {
                break;
            }
        }
    }

    if let Some(icon_path) = extracted_icon {
        let new_icon_name = format!(
            "{}.{}",
            app.name,
            icon_path.extension().unwrap().to_str().unwrap()
        );
        let final_icon_path = icon_dest_path.join(&new_icon_name);

        fs::copy(&icon_path, &final_icon_path)?;
        actual_icon_path = final_icon_path.to_string_lossy().to_string();
    }

    let mut extracted_desktop: Option<PathBuf> = None;
    if extracted_root.exists()
        && let Ok(entries) = fs::read_dir(&extracted_root)
    {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                extracted_desktop = Some(path);
                break;
            }
        }
    }

    if let Some(desktop_path) = extracted_desktop {
        let mut conf = ini::Ini::load_from_file(&desktop_path)?;

        if let Some(section) = conf.section_mut(Some("Desktop Entry")) {
            // Preserve AppImage-specific args like `--no-sandbox %U` while rewriting paths.
            let existing_exec = section.get("Exec").unwrap_or("").to_string();
            let mut suffix_args = String::new();
            if let Some(space_idx) = existing_exec.find(' ') {
                suffix_args = existing_exec[space_idx..].to_string();
            }

            let absolute_exec = format!("{}{}", exec_path.display(), suffix_args);
            section.insert("Exec", absolute_exec.clone());
            section.insert("TryExec", exec_path.display().to_string());

            if !actual_icon_path.is_empty() {
                section.insert("Icon", actual_icon_path);
            }
        }

        let target_desktop_file = apps_dir.join(format!("{}.desktop", app.name));
        conf.write_to_file(&target_desktop_file)?;
    } else {
        eprintln!("Warning: No .desktop file found inside {}", app.name);
    }

    let _ = fs::remove_dir_all(&tmp_dir);

    Ok(())
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            let mut resolved = PathBuf::from(home);
            resolved.push(stripped);
            return resolved;
        }
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
        if let Err(e) = fs::remove_file(failed_new_appimage_path) {
            eprintln!(
                "Warning: Failed to remove new AppImage during rollback: {}",
                e
            );
        }
    }

    if let Some(old_path) = old_appimage_path {
        if old_path.exists() {
            if let Err(e) = integrate(app, global, old_path, None) {
                eprintln!(
                    "Warning: Failed to fully restore old AppImage during rollback: {}",
                    e
                );
            }
        }
    }
}
