/// Remove the managed AppImage and desktop integration for an app.
///
/// This is intentionally narrow: user config files are left intact.
use anyhow::Result;
use std::fs;

use crate::config::{AppConfig, GlobalConfig};
use crate::integrator::{expand_tilde, legacy_desktop_asset_names, sanitized_app_name};
use crate::output::{print_success, print_warning};
use crate::state::AppState;

pub fn remove_app(
    app: &AppConfig,
    global: &GlobalConfig,
    state: Option<&AppState>,
    quiet: bool,
    colors: bool,
) -> Result<()> {
    if let Some(s) = state
        && let Some(file_path_str) = &s.file_path
    {
        let file_path = std::path::Path::new(file_path_str);
        if file_path.exists() {
            if let Err(e) = fs::remove_file(file_path) {
                if !quiet {
                    print_warning(
                        &format!("Failed to delete AppImage binary {:?}: {}", file_path, e),
                        colors,
                    );
                }
            }
        } else if !quiet {
            print_warning(
                &format!(
                    "Warning: AppImage binary {:?} was already missing or deleted.",
                    file_path
                ),
                colors,
            );
        }
    }

    let symlink_dir = expand_tilde(&global.symlink_dir);
    let symlink_path = symlink_dir.join(&app.name);

    if symlink_path.exists() || symlink_path.is_symlink() {
        if let Err(e) = fs::remove_file(&symlink_path) {
            if !quiet {
                print_warning(
                    &format!("Failed to remove symlink {:?}: {}", symlink_path, e),
                    colors,
                );
            }
        }
    } else if !quiet {
        print_warning(
            &format!(
                "Warning: Symlink {:?} was already missing or deleted.",
                symlink_path
            ),
            colors,
        );
    }

    remove_desktop(app, state, quiet, colors)?;

    if !quiet {
        print_success(&format!("Removed {}", app.name), colors);
    }
    Ok(())
}

fn remove_desktop(
    app: &AppConfig,
    state: Option<&AppState>,
    quiet: bool,
    colors: bool,
) -> Result<()> {
    let data_local_dir = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| expand_tilde("~/.local/share"));

    let apps_dir = data_local_dir.join("applications");

    let sanitized_name = sanitized_app_name(&app.name);
    let mut desktop_names = vec![sanitized_name.clone()];
    desktop_names.extend(legacy_desktop_asset_names(app, state, &sanitized_name));

    for name in desktop_names {
        let desktop_path = apps_dir.join(format!("{}.desktop", name));
        if desktop_path.exists() {
            if let Err(e) = fs::remove_file(&desktop_path) {
                if !quiet {
                    print_warning(
                        &format!("Failed to remove desktop file {:?}: {}", desktop_path, e),
                        colors,
                    );
                }
            }
        } else if !quiet {
            print_warning(
                &format!(
                    "Warning: Desktop file {:?} was already missing or deleted.",
                    desktop_path
                ),
                colors,
            );
        }
    }

    if let Some(s) = state
        && let Some(file_path_str) = &s.file_path
    {
        let file_path = std::path::Path::new(file_path_str);
        if let Some(parent_dir) = file_path.parent() {
            let icons_dir = parent_dir.join(".icons");
            let mut icon_names = vec![sanitized_name];
            icon_names.extend(legacy_desktop_asset_names(app, state, &icon_names[0]));

            for name in icon_names {
                for ext in ["png", "svg"] {
                    let icon_path = icons_dir.join(format!("{}.{}", name, ext));
                    if icon_path.exists() {
                        if let Err(e) = fs::remove_file(&icon_path) {
                            if !quiet {
                                print_warning(
                                    &format!("Failed to remove icon file {:?}: {}", icon_path, e),
                                    colors,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
