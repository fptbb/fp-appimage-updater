use fp_appimage_updater::config::{AppConfig, StrategyConfig};
use fp_appimage_updater::resolvers::script::resolve;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use ureq::Agent;

fn temp_config_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock went backwards")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("fp-appimage-updater-script-{}", unique));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn script_failure_reports_full_context() {
    let config_dir = temp_config_dir();
    let script_path = config_dir.join("resolver.sh");

    fs::write(
        &script_path,
        "#!/bin/sh\necho \"this script is broken\"\necho \"and exits non-zero\" >&2\nexit 42\n",
    )
    .expect("failed to write script");

    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let app = AppConfig {
        config_dir: config_dir.clone(),
        name: "broken-script".to_string(),
        zsync: None,
        integration: None,
        create_symlink: None,
        segmented_downloads: None,
        github_proxy: None,
        github_proxy_prefix: None,
        respect_rate_limits: None,
        storage_dir: None,
        strategy: StrategyConfig::Script {
            script_path: "./resolver.sh".to_string(),
        },
    };
    let client = Agent::new_with_defaults();

    let err = resolve(&client, &app, "./resolver.sh", None).expect_err("expected script failure");

    let message = format!("{:#}", err);
    assert!(message.contains("Resolver script for 'broken-script' failed"));
    assert!(message.contains("exit: 42"));
    assert!(message.contains("stdout:"));
    assert!(message.contains("stderr:"));
}
