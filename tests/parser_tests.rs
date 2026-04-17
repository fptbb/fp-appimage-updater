use fp_appimage_updater::parser::{ConfigPaths, load_app_configs, load_global_config};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

#[test]
fn infer_name_from_valid_yaml() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock went backwards")
        .as_nanos();
    let config_dir = std::env::temp_dir().join(format!("fp-appimage-updater-parser-{}", unique));
    let apps_dir = config_dir.join("apps");
    fs::create_dir_all(&apps_dir).expect("failed to create temp config dir");

    fs::write(
        apps_dir.join("whatpulse.yml"),
        "name: whatpulse\nstrategy:\n  strategy: direct\n  url: https://example.org/x.AppImage\n  check_method: etag\n",
    )
    .expect("failed to write config");

    let paths = ConfigPaths::with_config_dir(config_dir).expect("expected config paths");
    let result = load_app_configs(&paths).expect("expected config load");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.apps[0].name, "whatpulse");
}

#[test]
fn test_github_token_loading_priority() {
    let tmp = tempdir().unwrap();
    let config_dir = tmp.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();
    
    let paths = ConfigPaths {
        config_dir: config_dir.clone(),
        state_dir: tmp.path().join("state"),
    };

    // 1. Test loading from secrets.yml
    unsafe { std::env::remove_var("GITHUB_TOKEN") };
    fs::write(
        paths.secrets_path(),
        "github_token: \"token-from-file\"",
    ).unwrap();

    let config = load_global_config(&paths).unwrap();
    assert_eq!(config.github_token, Some("token-from-file".to_string()));

    // 2. Test env var override
    unsafe { std::env::set_var("GITHUB_TOKEN", "token-from-env") };
    let config = load_global_config(&paths).unwrap();
    assert_eq!(config.github_token, Some("token-from-env".to_string()));
    
    // Cleanup
    unsafe { std::env::remove_var("GITHUB_TOKEN") };
}
