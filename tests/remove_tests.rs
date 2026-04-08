use fp_appimage_updater::commands::remove::clear_installed_state;
use fp_appimage_updater::state::{AppState, ForgePlatform};

#[test]
fn clearing_installed_state_keeps_download_history() {
    let mut state = AppState {
        local_version: Some("1.2.3".to_string()),
        etag: Some("abc".to_string()),
        last_modified: Some("yesterday".to_string()),
        file_path: Some("/tmp/app.AppImage".to_string()),
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
    assert_eq!(state.rate_limited_until, None);
    assert_eq!(state.download_bytes, Some(42));
    assert_eq!(state.capabilities, vec!["segmented_downloads".to_string()]);
    assert_eq!(state.segmented_downloads, Some(true));
    assert_eq!(state.forge_repository, Some("repo".to_string()));
    assert_eq!(state.forge_platform, Some(ForgePlatform::GitHub));
}
