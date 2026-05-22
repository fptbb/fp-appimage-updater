/// Install a downloaded AppImage into the user's environment.
///
/// This handles the executable bit, symlink updates, desktop entry extraction,
/// icon copying, and cleanup of the previous version.
use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{AppConfig, GlobalConfig};
use crate::state::AppState;

pub fn integrate(
    app: &AppConfig,
    global: &GlobalConfig,
    appimage_path: &Path,
    old_appimage_path: Option<&Path>,
    state: Option<&AppState>,
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
    if should_integrate && let Err(e) = integrate_desktop(app, appimage_path, state) {
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

fn integrate_desktop(app: &AppConfig, exec_path: &Path, state: Option<&AppState>) -> Result<()> {
    let data_local_dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| expand_tilde("~/.local/share"));
    let apps_dir = data_local_dir.join("applications");
    fs::create_dir_all(&apps_dir)?;
    let sanitized_name = sanitized_app_name(&app.name);

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

    let extracted_root = tmp_dir.join("squashfs-root");
    if let Err(err) = extract_appimage_root(exec_path, &tmp_dir, &extracted_root) {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(err);
    }

    // Find and rewrite desktop file
    let desktop_path = find_desktop_file(&extracted_root);
    let mut actual_icon_path = String::new();
    if let Some(icon_path) = find_best_icon(&extracted_root, desktop_path.as_deref())
        && let Some(ext) = icon_path.extension()
    {
        let final_icon_path =
            icon_dest_dir.join(format!("{}.{}", sanitized_name, ext.to_string_lossy()));
        if fs::copy(&icon_path, &final_icon_path).is_ok() {
            actual_icon_path = final_icon_path.to_string_lossy().to_string();
        }
    }

    if let Some(desktop_path) = desktop_path
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
        let target_desktop = apps_dir.join(format!("{}.desktop", sanitized_name));
        let _ = conf.write_to_file(target_desktop);
    }

    cleanup_legacy_desktop_assets(app, exec_path, state, &sanitized_name, &apps_dir);

    let _ = fs::remove_dir_all(&tmp_dir);
    Ok(())
}

pub fn extract_appimage_root(
    exec_path: &Path,
    tmp_dir: &Path,
    extracted_root: &Path,
) -> Result<()> {
    if is_nixos_host() {
        return try_appimage_run_extract(exec_path, extracted_root).context(
            "NixOS detected: desktop integration requires appimage-run extraction support",
        );
    }

    // Try the upstream AppImage runtime contract first.
    let direct = Command::new(exec_path)
        .arg("--appimage-extract")
        .current_dir(tmp_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    match direct {
        Ok(output) if output.status.success() && extracted_root.exists() => return Ok(()),
        Ok(output) => {
            // On NixOS with binfmt enabled, execution is typically routed through appimage-run.
            // In that mode, forwarding --appimage-extract does not necessarily produce the local
            // squashfs-root directory that upstream AppImages normally create.
            if try_appimage_run_extract(exec_path, extracted_root).is_ok()
                && extracted_root.exists()
            {
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            anyhow::bail!(
                "AppImage extraction failed via --appimage-extract{}",
                format_stderr_detail(stderr)
            );
        }
        Err(direct_err) => {
            if try_appimage_run_extract(exec_path, extracted_root).is_ok()
                && extracted_root.exists()
            {
                return Ok(());
            }

            anyhow::bail!("Failed to run --appimage-extract ({})", direct_err);
        }
    }
}

fn try_appimage_run_extract(exec_path: &Path, extracted_root: &Path) -> Result<()> {
    let output = Command::new("appimage-run")
        .arg("-x")
        .arg(extracted_root)
        .arg(exec_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("Failed to invoke appimage-run fallback")?;

    if output.status.success() && extracted_root.exists() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if let Some(message) = nixos_unsupported_appimage_message(stderr) {
            anyhow::bail!("{}", message);
        }
        anyhow::bail!(
            "appimage-run fallback failed{}",
            format_stderr_detail(stderr)
        );
    }
}

fn format_stderr_detail(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(" ({})", first_line(stderr))
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

pub fn nixos_unsupported_appimage_message(stderr: &str) -> Option<String> {
    let lowered = stderr.to_ascii_lowercase();
    if lowered.contains("can't find a valid squashfs superblock") {
        Some(
            "this AppImage format is not supported on NixOS in this app yet: appimage-run expected a SquashFS-based AppImage, but this file does not appear to use SquashFS"
                .to_string(),
        )
    } else {
        None
    }
}

pub(crate) fn is_nixos_host() -> bool {
    if let Some(override_id) = std::env::var_os("FP_APPIMAGE_UPDATER_OS_ID") {
        return override_id.to_string_lossy().eq_ignore_ascii_case("nixos");
    }

    parse_os_release_id(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
        .is_some_and(|id| id == "nixos")
        || Path::new("/run/current-system/nixos-version").exists()
}

pub fn parse_os_release_id(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let value = line.strip_prefix("ID=")?;
        Some(value.trim_matches('"').to_ascii_lowercase())
    })
}

pub fn sanitized_app_name(name: &str) -> String {
    let mut sanitized = String::new();
    let mut last_was_dash = false;

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if (ch.is_ascii_whitespace() || matches!(ch, '-' | '_' | '.')) && !last_was_dash {
            sanitized.push('-');
            last_was_dash = true;
        }
    }

    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "app".to_string()
    } else {
        sanitized.to_string()
    }
}

fn cleanup_legacy_desktop_assets(
    app: &AppConfig,
    exec_path: &Path,
    state: Option<&AppState>,
    sanitized_name: &str,
    apps_dir: &Path,
) {
    for name in legacy_desktop_asset_names(app, state, sanitized_name) {
        let _ = fs::remove_file(apps_dir.join(format!("{}.desktop", name)));

        if let Some(icons_dir) = exec_path.parent().map(|p| p.join(".icons")) {
            for ext in ["png", "svg"] {
                let _ = fs::remove_file(icons_dir.join(format!("{}.{}", name, ext)));
            }
        }
    }
}

pub fn legacy_desktop_asset_names(
    app: &AppConfig,
    state: Option<&AppState>,
    sanitized_name: &str,
) -> Vec<String> {
    let mut names = Vec::new();

    if app.name != sanitized_name {
        names.push(app.name.clone());
    }

    if let Some(previous) = state.and_then(|s| s.sanitized_name.as_deref())
        && previous != sanitized_name
        && previous != app.name
    {
        names.push(previous.to_string());
    }

    names
}

pub fn find_best_icon(root: &Path, desktop_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(desktop_path) = desktop_path
        && let Ok(conf) = ini::Ini::load_from_file(desktop_path)
        && let Some(section) = conf.section(Some("Desktop Entry"))
        && let Some(icon_value) = section.get("Icon")
        && let Some(icon_path) = find_icon_for_desktop_entry(root, icon_value)
    {
        return Some(icon_path);
    }

    find_first_file_with_extensions(root, &["svg", "png", "xpm"])
}

pub fn find_desktop_file(root: &Path) -> Option<PathBuf> {
    find_first_file_with_extensions(root, &["desktop"])
}

fn find_icon_for_desktop_entry(root: &Path, icon_value: &str) -> Option<PathBuf> {
    let icon_path = Path::new(icon_value);

    if icon_path.is_absolute() {
        let relative = icon_path.strip_prefix("/").ok()?;
        let candidate = root.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
    } else if icon_value.contains('/') {
        let candidate = root.join(icon_path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let icon_name = icon_path.file_name()?.to_str()?;
    let extensions = ["svg", "png", "xpm"];

    if icon_path.extension().is_some()
        && let Some(candidate) = find_file_by_name(root, icon_name)
    {
        return Some(candidate);
    }

    for ext in extensions {
        let with_ext = format!("{icon_name}.{ext}");
        if let Some(candidate) = find_file_by_name(root, &with_ext) {
            return Some(candidate);
        }
    }

    find_file_by_stem(root, icon_name, &extensions)
}

fn find_first_file_with_extensions(root: &Path, extensions: &[&str]) -> Option<PathBuf> {
    visit_files(root, &mut |path| {
        let ext = path.extension().and_then(|value| value.to_str())?;
        extensions
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
            .then(|| path.to_path_buf())
    })
}

fn find_file_by_name(root: &Path, file_name: &str) -> Option<PathBuf> {
    visit_files(root, &mut |path| {
        (path.file_name().and_then(|value| value.to_str()) == Some(file_name))
            .then(|| path.to_path_buf())
    })
}

fn find_file_by_stem(root: &Path, stem: &str, extensions: &[&str]) -> Option<PathBuf> {
    visit_files(root, &mut |path| {
        let ext = path.extension().and_then(|value| value.to_str())?;
        let file_stem = path.file_stem().and_then(|value| value.to_str())?;
        (file_stem == stem
            && extensions
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate)))
        .then(|| path.to_path_buf())
    })
}

fn visit_files(root: &Path, visitor: &mut impl FnMut(&Path) -> Option<PathBuf>) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = visit_files(&path, visitor) {
                return Some(found);
            }
        } else if path.is_file()
            && let Some(found) = visitor(&path)
        {
            return Some(found);
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
    state: Option<&AppState>,
) {
    if failed_new_appimage_path.exists() {
        let _ = fs::remove_file(failed_new_appimage_path);
    }

    if let Some(old_path) = old_appimage_path
        && old_path.exists()
    {
        let _ = integrate(app, global, old_path, None, state);
    }
}
