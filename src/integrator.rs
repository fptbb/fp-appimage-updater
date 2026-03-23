use anyhow::{Context, Result};
use directories::UserDirs;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, GlobalConfig};

pub async fn integrate(
    app: &AppConfig,
    global: &GlobalConfig,
    appimage_path: &Path,
    old_appimage_path: Option<&Path>,
) -> Result<()> {
    // 1. Chmod +x
    let mut perms = fs::metadata(appimage_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(appimage_path, perms).context("Failed to make AppImage executable")?;

    // 2. Symlink
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

    // 3. Desktop Integration
    let should_integrate = app.integration.unwrap_or(global.manage_desktop_files);
    if should_integrate {
        integrate_desktop(app, appimage_path).await?;
    }

    // 4. Delete old AppImage if it's different
    if let Some(old_path) = old_appimage_path && old_path != appimage_path && old_path.exists() && let Err(e) = fs::remove_file(old_path) {
        eprintln!("Warning: Failed to delete old AppImage {:?}: {}", old_path, e);
    }

    Ok(())
}

async fn integrate_desktop(app: &AppConfig, exec_path: &Path) -> Result<()> {
    // Determine data storage directory
    let data_local_dir = UserDirs::new()
        .and_then(|u| u.document_dir().map(|d| d.parent().unwrap().join(".local/share")))
        .unwrap_or_else(|| expand_tilde("~/.local/share"));
    let apps_dir = data_local_dir.join("applications");
    fs::create_dir_all(&apps_dir)?;

    // Determine icons directory mapping relative to AppImage actual directory
    // According to specs, the .icons folder should sit next to the AppImage
    let mut icon_dest_path = PathBuf::new();
    let mut actual_icon_path = String::new();

    if let Some(parent_dir) = exec_path.parent() {
        let appimages_icons_dir = parent_dir.join(".icons");
        fs::create_dir_all(&appimages_icons_dir)?;
        icon_dest_path = appimages_icons_dir;
    }

    // 1. Create a temporary directory to extract into
    let tmp_dir = std::env::temp_dir().join(format!("fp-appimage-extractor-{}", app.name));
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir)?;
    }
    fs::create_dir_all(&tmp_dir)?;

    // 2. Extract .desktop and related icons from the AppImage
    let output = std::process::Command::new(exec_path)
        .arg("--appimage-extract")
        .current_dir(&tmp_dir)
        .output()
        .with_context(|| format!("Failed to run --appimage-extract on AppImage {:?}", exec_path))?;

    if !output.status.success() {
        // Fallback: Just let users know extraction failed if the AppImage is malformed or standard overrides are required
        eprintln!("Warning: Failed to extract internal desktop/icon from {}, the .desktop will not be generated.", app.name);
        eprintln!("Extraction error: {}", String::from_utf8_lossy(&output.stderr));
        let _ = fs::remove_dir_all(&tmp_dir);
        return Ok(());
    }

    let extracted_root = tmp_dir.join("squashfs-root");
    
    // 3. Find the extracted icon
    let mut extracted_icon: Option<PathBuf> = None;
    if extracted_root.exists() {
        for ext in ["png", "svg"] {
            // Find an icon matching the app name or just any icon exported at root
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

    // Move the icon into the `.icons` folder
    if let Some(icon_path) = extracted_icon {
        let new_icon_name = format!("{}.{}", app.name, icon_path.extension().unwrap().to_str().unwrap());
        let final_icon_path = icon_dest_path.join(&new_icon_name);
        
        fs::copy(&icon_path, &final_icon_path)?;
        actual_icon_path = final_icon_path.to_string_lossy().to_string();
    }

    // 4. Find the .desktop file
    let mut extracted_desktop: Option<PathBuf> = None;
    if extracted_root.exists() && let Ok(entries) = fs::read_dir(&extracted_root) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                extracted_desktop = Some(path);
                break;
            }
        }
    }

    // 5. Rewrite desktop file
    if let Some(desktop_path) = extracted_desktop {
        let mut conf = ini::Ini::load_from_file(&desktop_path)?;
        
        if let Some(section) = conf.section_mut(Some("Desktop Entry")) {
            // Overwrite Exec, TryExec and Icon with absolute paths
            
            // Retain any specific AppImage arguments like `--no-sandbox %U`
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

    // Cleanup extraction tmp directory
    let _ = fs::remove_dir_all(&tmp_dir);

    Ok(())
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") && let Some(user_dirs) = UserDirs::new() {
        let mut resolved = user_dirs.home_dir().to_path_buf();
        resolved.push(stripped);
        return resolved;
    }
    PathBuf::from(path)
}

pub async fn rollback(
    app: &AppConfig,
    global: &GlobalConfig,
    failed_new_appimage_path: &Path,
    old_appimage_path: Option<&Path>,
) {
    if failed_new_appimage_path.exists() {
        if let Err(e) = fs::remove_file(failed_new_appimage_path) {
            eprintln!("Warning: Failed to remove new AppImage during rollback: {}", e);
        }
    }

    if let Some(old_path) = old_appimage_path {
        if old_path.exists() {
            // Re-integrate the old AppImage to restore symlink and desktop file
            if let Err(e) = integrate(app, global, old_path, None).await {
                eprintln!("Warning: Failed to fully restore old AppImage during rollback: {}", e);
            }
        }
    }
}
