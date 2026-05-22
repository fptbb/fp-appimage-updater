use fp_appimage_updater::self_updater::{
    SelfUpdateMode, SelfUpdatePlan, is_update_available, plan_self_update,
    should_print_current_message, should_print_start_message,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn stable_release_matching_current_version_is_not_an_update() {
    assert!(!is_update_available(&format!(
        "v{}",
        env!("CARGO_PKG_VERSION")
    )));
}

#[test]
fn prerelease_with_same_semver_is_not_an_update() {
    assert!(!is_update_available(&format!(
        "v{}-RC1",
        env!("CARGO_PKG_VERSION")
    )));
}

#[test]
fn newer_release_is_an_update() {
    assert!(is_update_available("v999.0.0"));
}

#[test]
fn unwritable_binary_only_warns_when_update_exists() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let binary_path = temp_dir.path().join(env!("CARGO_PKG_NAME"));
    fs::write(&binary_path, b"binary").expect("write test binary");

    let mut permissions = fs::metadata(&binary_path).expect("metadata").permissions();
    permissions.set_mode(0o444);
    fs::set_permissions(&binary_path, permissions).expect("set readonly perms");

    assert!(matches!(
        plan_self_update(&binary_path, &format!("v{}", env!("CARGO_PKG_VERSION"))),
        SelfUpdatePlan::AlreadyCurrent
    ));

    assert!(matches!(
        plan_self_update(&binary_path, "v999.0.0"),
        SelfUpdatePlan::UpdateAvailableButBinaryNotWritable
    ));
}

#[test]
fn quiet_mode_suppresses_routine_self_update_messages() {
    assert!(!should_print_start_message(SelfUpdateMode::QuietIfCurrent));
    assert!(!should_print_current_message(
        SelfUpdateMode::QuietIfCurrent
    ));
    assert!(should_print_start_message(SelfUpdateMode::Interactive));
    assert!(should_print_current_message(SelfUpdateMode::Interactive));
}
