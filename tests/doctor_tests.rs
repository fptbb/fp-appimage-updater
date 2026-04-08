use fp_appimage_updater::doctor::{self, DoctorStatus};
use fp_appimage_updater::parser::ConfigPaths;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "fp-appimage-updater-doctor-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn process_lock_check<'a>(checks: &'a [doctor::DoctorCheck]) -> &'a doctor::DoctorCheck {
    checks
        .iter()
        .find(|check| check.name == "process_lock")
        .expect("missing process_lock check")
}

#[test]
fn reports_stale_lock_as_warn() {
    let root = unique_temp_dir("stale");
    let config_dir = root.join("config");
    let state_dir = root.join("state");
    fs::create_dir_all(&config_dir).expect("failed to create config dir");
    fs::create_dir_all(&state_dir).expect("failed to create state dir");

    let lock_path = state_dir.join("process.lock");
    fs::write(
        &lock_path,
        "pid=1\nboot_id=00000000-0000-0000-0000-000000000000\n",
    )
    .expect("failed to write lock");

    let paths = ConfigPaths {
        config_dir,
        state_dir,
    };

    let checks = doctor::run(&paths, 0, 0);
    let lock_check = process_lock_check(&checks);

    assert!(matches!(lock_check.status, DoctorStatus::Warn));
    assert!(lock_check.detail.contains("stale lock"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_active_lock_as_ok() {
    let root = unique_temp_dir("active");
    let config_dir = root.join("config");
    let state_dir = root.join("state");
    fs::create_dir_all(&config_dir).expect("failed to create config dir");
    fs::create_dir_all(&state_dir).expect("failed to create state dir");

    let lock_path = state_dir.join("process.lock");
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .expect("failed to read boot id")
        .trim()
        .to_string();
    fs::write(
        &lock_path,
        format!("pid={}\nboot_id={}\n", std::process::id(), boot_id),
    )
    .expect("failed to write lock");

    let paths = ConfigPaths {
        config_dir,
        state_dir,
    };

    let checks = doctor::run(&paths, 0, 0);
    let lock_check = process_lock_check(&checks);

    assert!(matches!(lock_check.status, DoctorStatus::Ok));
    assert!(lock_check.detail.contains("active lock held by pid"));

    let _ = fs::remove_dir_all(&root);
}
