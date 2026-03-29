use fp_appimage_updater::parser::{ConfigPaths, load_app_configs};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

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
