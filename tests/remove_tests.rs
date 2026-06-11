use fp_appimage_updater::commands::remove::cleanup_orphan_appimage_files;
use fp_appimage_updater::commands::remove::clear_installed_state;
use fp_appimage_updater::config::{AppConfig, StrategyConfig};
use fp_appimage_updater::integrator::legacy_desktop_asset_names;
use fp_appimage_updater::state::{AppState, ForgePlatform, State};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[test]
fn clearing_installed_state_keeps_download_history() {
    let mut state = AppState {
        local_version: Some("1.2.3".to_string()),
        etag: Some("abc".to_string()),
        last_modified: Some("yesterday".to_string()),
        file_path: Some("/tmp/app.AppImage".to_string()),
        sanitized_name: Some("app".to_string()),
        rate_limited_until: Some(123),
        capabilities: vec!["segmented_downloads".to_string()],
        segmented_downloads: Some(true),
        forge_repository: Some("repo".to_string()),
        forge_platform: Some(ForgePlatform::GitHub),
        download_bytes: Some(42),
    };

    clear_installed_state(&mut state);

    assert_eq!(state.local_version, None);
    assert_eq!(state.etag, None);
    assert_eq!(state.last_modified, None);
    assert_eq!(state.file_path, None);
    assert_eq!(state.sanitized_name, None);
    assert_eq!(state.rate_limited_until, None);
    assert_eq!(state.download_bytes, Some(42));
    assert_eq!(state.capabilities, vec!["segmented_downloads".to_string()]);
    assert_eq!(state.segmented_downloads, Some(true));
    assert_eq!(state.forge_repository, Some("repo".to_string()));
    assert_eq!(state.forge_platform, Some(ForgePlatform::GitHub));
}

#[test]
fn legacy_desktop_asset_names_include_old_variants() {
    let app = AppConfig {
        config_dir: PathBuf::new(),
        name: "My App".to_string(),
        ignore: None,
        zsync: None,
        integration: None,
        create_symlink: None,
        segmented_downloads: None,
        respect_rate_limits: None,
        github_proxy: None,
        github_proxy_prefix: None,
        storage_dir: None,
        naming_format: None,
        inner_asset_match: None,
        strategy: StrategyConfig::Direct {
            url: "https://example.com/My.AppImage".to_string(),
            check_method: fp_appimage_updater::config::CheckMethod::Etag,
        },
    };
    let state = AppState {
        sanitized_name: Some("old-app".to_string()),
        ..AppState::default()
    };

    let names = legacy_desktop_asset_names(&app, Some(&state), "my-app");

    assert_eq!(names, vec!["My App".to_string(), "old-app".to_string()]);
}

#[test]
fn test_dummy_app_config_for_orphans() {
    let target_name = "orphaned-app".to_string();
    let dummy_app = AppConfig {
        config_dir: PathBuf::new(),
        name: target_name.clone(),
        ignore: None,
        zsync: None,
        integration: None,
        create_symlink: None,
        segmented_downloads: None,
        respect_rate_limits: None,
        github_proxy: None,
        github_proxy_prefix: None,
        storage_dir: None,
        naming_format: None,
        inner_asset_match: None,
        strategy: StrategyConfig::Direct {
            url: "https://example.com/My.AppImage".to_string(),
            check_method: fp_appimage_updater::config::CheckMethod::Etag,
        },
    };

    assert_eq!(dummy_app.name, "orphaned-app");
}

#[test]
fn cleanup_orphan_appimage_files_removes_only_unreferenced_appimages() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let storage_dir = dir.path().join("AppImages");
    let icons_dir = storage_dir.join(".icons");
    let kept = storage_dir.join("kept.AppImage");
    let orphan = storage_dir.join("orphan.AppImage");
    let note = storage_dir.join("notes.txt");

    fs::create_dir_all(&icons_dir).expect("failed to create icons dir");
    fs::write(&kept, "kept").expect("failed to write kept file");
    fs::write(&orphan, "orphan").expect("failed to write orphan file");
    fs::write(&note, "note").expect("failed to write note file");
    fs::write(icons_dir.join("icon.png"), "icon").expect("failed to write icon");

    let mut apps = HashMap::new();
    apps.insert(
        "kept-app".to_string(),
        AppState {
            file_path: Some(kept.to_string_lossy().to_string()),
            ..AppState::default()
        },
    );
    let state = State { apps };
    let mut results = Vec::new();

    cleanup_orphan_appimage_files(&storage_dir, &state, &mut results, false, true)
        .expect("cleanup should succeed");

    assert!(kept.exists());
    assert!(!orphan.exists());
    assert!(note.exists());
    assert!(icons_dir.join("icon.png").exists());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "orphan.AppImage");
}

#[test]
fn legacy_desktop_asset_names_do_not_block_sanitized_removal() {
    let app = AppConfig {
        config_dir: PathBuf::new(),
        name: "linux toys".to_string(),
        ignore: None,
        zsync: None,
        integration: None,
        create_symlink: None,
        segmented_downloads: None,
        respect_rate_limits: None,
        github_proxy: None,
        github_proxy_prefix: None,
        storage_dir: None,
        naming_format: None,
        inner_asset_match: None,
        strategy: StrategyConfig::Direct {
            url: "https://example.com/linux-toys.AppImage".to_string(),
            check_method: fp_appimage_updater::config::CheckMethod::Etag,
        },
    };

    let names = legacy_desktop_asset_names(&app, None, "linux-toys");

    assert_eq!(names, vec!["linux toys".to_string()]);
}
