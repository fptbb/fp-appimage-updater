use fp_appimage_updater::config::{AppConfig, CheckMethod, GlobalConfig, StrategyConfig};
use fp_appimage_updater::disintegrator::remove_app;
use fp_appimage_updater::integrator::{
    extract_appimage_root, find_best_icon, find_desktop_file, integrate,
    nixos_unsupported_appimage_message, parse_os_release_id, sanitized_app_name,
};
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    fn set_path_prepend(path: &Path) -> Self {
        let original = std::env::var_os("PATH");
        let mut joined = OsString::from(path.as_os_str());
        if let Some(existing) = &original {
            joined.push(":");
            joined.push(existing);
        }
        unsafe {
            std::env::set_var("PATH", &joined);
        }
        Self {
            key: "PATH",
            original,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

#[cfg(unix)]
fn write_executable(path: &Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, content).expect("failed to write executable");
    let mut permissions = fs::metadata(path)
        .expect("failed to stat executable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("failed to chmod executable");
}

#[test]
fn sanitizes_desktop_asset_names() {
    assert_eq!(sanitized_app_name("Foo Bar"), "foo-bar");
    assert_eq!(sanitized_app_name("My_App.Image"), "my-app-image");
    assert_eq!(sanitized_app_name("  Weird   Name  "), "weird-name");
    assert_eq!(sanitized_app_name("!!!"), "app");
}

#[test]
fn parses_nixos_os_release_id() {
    assert_eq!(
        parse_os_release_id("NAME=NixOS\nID=nixos\nPRETTY_NAME=NixOS\n"),
        Some("nixos".to_string())
    );
    assert_eq!(
        parse_os_release_id("NAME=\"Fedora Linux\"\nID=\"fedora\"\n"),
        Some("fedora".to_string())
    );
}

#[test]
fn detects_non_squashfs_appimage_message() {
    assert!(
        nixos_unsupported_appimage_message(
            "FATAL ERROR: Can't find a valid SQUASHFS superblock on /tmp/demo.AppImage"
        )
        .is_some()
    );
    assert!(nixos_unsupported_appimage_message("extract failed").is_none());
}

#[cfg(unix)]
#[test]
fn falls_back_to_appimage_run_extract_when_direct_extract_does_not_produce_root() {
    let _guard = env_lock().lock().expect("failed to lock env");
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let bin_dir = dir.path().join("bin");
    let exec_path = dir.path().join("test.AppImage");
    let tmp_dir = dir.path().join("tmp");
    let extracted_root = tmp_dir.join("squashfs-root");

    fs::create_dir_all(&bin_dir).expect("failed to create bin dir");
    fs::create_dir_all(&tmp_dir).expect("failed to create tmp dir");

    write_executable(
        &exec_path,
        "#!/bin/sh\nif [ \"$1\" = \"--appimage-extract\" ]; then\n  exit 0\nfi\nexit 1\n",
    );
    write_executable(
        &bin_dir.join("appimage-run"),
        "#!/bin/sh\nif [ \"$1\" = \"-x\" ]; then\n  dest=\"$2\"\n  mkdir -p \"$dest\"\n  printf '[Desktop Entry]\\nName=Demo\\n' > \"$dest/demo.desktop\"\n  printf '<svg/>' > \"$dest/demo.svg\"\n  exit 0\nfi\nexit 1\n",
    );
    let _path_guard = EnvVarGuard::set_path_prepend(&bin_dir);

    extract_appimage_root(&exec_path, &tmp_dir, &extracted_root)
        .expect("expected appimage-run fallback to succeed");

    assert!(extracted_root.exists());
    assert!(extracted_root.join("demo.desktop").exists());
    assert!(extracted_root.join("demo.svg").exists());
}

#[test]
fn finds_nested_desktop_and_icon_from_desktop_entry() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let root = dir.path().join("squashfs-root");
    let applications_dir = root.join("usr/share/applications");
    let icons_dir = root.join("usr/share/icons/hicolor/256x256/apps");

    fs::create_dir_all(&applications_dir).expect("failed to create applications dir");
    fs::create_dir_all(&icons_dir).expect("failed to create icons dir");

    let desktop_path = applications_dir.join("demo.desktop");
    let icon_path = icons_dir.join("demo-icon.png");
    fs::write(
        &desktop_path,
        "[Desktop Entry]\nName=Demo\nIcon=demo-icon\n",
    )
    .expect("failed to write desktop file");
    fs::write(&icon_path, "png").expect("failed to write icon file");

    let found_desktop = find_desktop_file(&root).expect("expected desktop file");
    let found_icon =
        find_best_icon(&root, Some(&found_desktop)).expect("expected icon from desktop entry");

    assert_eq!(found_desktop, desktop_path);
    assert_eq!(found_icon, icon_path);
}

#[test]
fn rollback_restores_backup_file_and_deletes_failed_binary() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let failed_path = dir.path().join("app.AppImage");
    let backup_path = dir.path().join("app.bak");

    // Write a dummy failed new binary
    fs::write(&failed_path, "failed new binary").expect("failed to write failed binary");
    // Write a dummy backup file (representing the old working binary)
    fs::write(&backup_path, "working backup binary").expect("failed to write backup binary");

    let app = fp_appimage_updater::config::AppConfig {
        config_dir: std::path::PathBuf::new(),
        name: "test-app".to_string(),
        ignore: None,
        zsync: None,
        integration: Some(false), // Disable desktop integration to keep it simple
        create_symlink: Some(false),
        segmented_downloads: None,
        respect_rate_limits: None,
        github_proxy: None,
        github_proxy_prefix: None,
        storage_dir: None,
        naming_format: None,
        inner_asset_match: None,
        strategy: fp_appimage_updater::config::StrategyConfig::Direct {
            url: String::new(),
            check_method: fp_appimage_updater::config::CheckMethod::Etag,
        },
    };
    let global = fp_appimage_updater::config::GlobalConfig::default();

    // Call rollback
    fp_appimage_updater::integrator::rollback(&app, &global, &failed_path, None, None);

    // Assert that the failed binary is deleted/restored
    assert!(failed_path.exists());
    let restored_content =
        fs::read_to_string(&failed_path).expect("failed to read restored binary");
    assert_eq!(restored_content, "working backup binary");
    assert!(!backup_path.exists());
}

#[cfg(unix)]
#[test]
fn integrates_and_removes_symlink_using_sanitized_name() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let symlink_dir = dir.path().join("links");
    let appimage_path = dir.path().join("My App.AppImage");

    fs::create_dir_all(&symlink_dir).expect("failed to create symlink dir");
    write_executable(&appimage_path, "#!/bin/sh\nexit 0\n");

    let app = AppConfig {
        config_dir: PathBuf::new(),
        name: "My App".to_string(),
        ignore: None,
        zsync: None,
        integration: Some(false),
        create_symlink: Some(true),
        segmented_downloads: None,
        respect_rate_limits: None,
        github_proxy: None,
        github_proxy_prefix: None,
        storage_dir: None,
        naming_format: None,
        inner_asset_match: None,
        strategy: StrategyConfig::Direct {
            url: "https://example.com/My App.AppImage".to_string(),
            check_method: CheckMethod::Etag,
        },
    };
    let mut global = GlobalConfig::default();
    global.symlink_dir = symlink_dir.to_string_lossy().to_string();
    global.create_symlinks = true;
    global.manage_desktop_files = false;

    integrate(&app, &global, &appimage_path, None, None).expect("expected integration to succeed");

    let sanitized_symlink = symlink_dir.join("my-app");
    assert!(sanitized_symlink.exists());
    assert!(sanitized_symlink.is_symlink());
    assert!(!symlink_dir.join("My App").exists());

    remove_app(&app, &global, None, true, true).expect("expected removal to succeed");

    assert!(!sanitized_symlink.exists());
}
