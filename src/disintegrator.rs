use anyhow::Result;
use directories::UserDirs;
use std::fs;

use crate::config::{AppConfig, GlobalConfig};
use crate::integrator::expand_tilde;
use crate::state::AppState;

pub fn remove_app(
    app: &AppConfig,
    global: &GlobalConfig,
    state: Option<&AppState>,
) -> Result<()> {
    // 1. Delete AppImage binary
    if let Some(s) = state
        && let Some(file_path_str) = &s.file_path {
            let file_path = std::path::Path::new(file_path_str);
            if file_path.exists()
                && let Err(e) = fs::remove_file(file_path) {
                    eprintln!("Warning: Failed to delete AppImage binary {:?}: {}", file_path, e);
                }
        }

    // 2. Remove symlink
    let symlink_dir = expand_tilde(&global.symlink_dir);
    let symlink_path = symlink_dir.join(&app.name);

    if (symlink_path.exists() || symlink_path.is_symlink())
        && let Err(e) = fs::remove_file(&symlink_path) {
            eprintln!("Warning: Failed to remove symlink {:?}: {}", symlink_path, e);
        }

    // 3. Remove Desktop Integration
    remove_desktop(app, state)?;

    println!("Successfully removed {}", app.name);
    Ok(())
}

fn remove_desktop(app: &AppConfig, state: Option<&AppState>) -> Result<()> {
    let data_local_dir = UserDirs::new()
        .and_then(|u| u.document_dir().map(|d| d.parent().unwrap().join(".local/share")))
        .unwrap_or_else(|| expand_tilde("~/.local/share"));

    let apps_dir = data_local_dir.join("applications");

    let desktop_path = apps_dir.join(format!("{}.desktop", app.name));
    if desktop_path.exists() && let Err(e) = fs::remove_file(&desktop_path) {
        eprintln!("Warning: Failed to remove desktop file {:?}: {}", desktop_path, e);
    }

    // Attempt to remove icon files from the AppImages local .icons folder
    if let Some(s) = state && let Some(file_path_str) = &s.file_path {
        let file_path = std::path::Path::new(file_path_str);
        if let Some(parent_dir) = file_path.parent() {
            let icons_dir = parent_dir.join(".icons");
            let png_icon = icons_dir.join(format!("{}.png", app.name));
            let svg_icon = icons_dir.join(format!("{}.svg", app.name));
            
            for icon_path in [png_icon, svg_icon] {
                if icon_path.exists() {
                     let _ = fs::remove_file(&icon_path);
                }
            }
        }
    }

    Ok(())
}
