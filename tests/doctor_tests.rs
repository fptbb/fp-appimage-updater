use fp_appimage_updater::config::GlobalConfig;
use fp_appimage_updater::doctor::{self, DoctorStatus};
use fp_appimage_updater::parser::ConfigPaths;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
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

fn doctor_check<'a>(checks: &'a [doctor::DoctorCheck], name: &str) -> &'a doctor::DoctorCheck {
    checks
        .iter()
        .find(|check| check.name == name)
        .expect("missing doctor check")
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, content: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, content).expect("failed to write executable");
    let mut permissions = fs::metadata(path)
        .expect("failed to stat executable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("failed to chmod executable");
}

fn write_global_config(paths: &ConfigPaths) {
    fs::create_dir_all(&paths.config_dir).expect("failed to create config dir");
    let content =
        serde_yaml::to_string(&GlobalConfig::default()).expect("failed to serialize config");
    fs::write(paths.global_config_path(), content).expect("failed to write global config");
}

fn write_app_recipe(paths: &ConfigPaths, file_name: &str, content: &str) {
    let apps_dir = paths.config_dir.join("apps");
    fs::create_dir_all(&apps_dir).expect("failed to create apps dir");
    fs::write(apps_dir.join(file_name), content).expect("failed to write app recipe");
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
        let original = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, original }
    }

    fn set_value(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, original }
    }

    fn prepend_path(path: &std::path::Path) -> Self {
        let original = std::env::var_os("PATH");
        let mut joined = std::ffi::OsString::from(path.as_os_str());
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

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();

    write_global_config(&paths);
    let checks = doctor::run(&paths, &global_config, &client);
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

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();

    write_global_config(&paths);
    let checks = doctor::run(&paths, &global_config, &client);
    let lock_check = process_lock_check(&checks);

    assert!(matches!(lock_check.status, DoctorStatus::Ok));
    assert!(lock_check.detail.contains("active lock held by pid"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_directory_access_checks_as_ok_when_present_and_writable() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_temp_dir("dir-access-ok");
    let config_dir = root.join("config");
    let state_dir = root.join("state");
    let storage_dir = root.join("storage");
    let symlink_dir = root.join("symlinks");
    let xdg_data_home = root.join("xdg-data");
    let desktop_dir = xdg_data_home.join("applications");
    fs::create_dir_all(&config_dir).expect("failed to create config dir");
    fs::create_dir_all(&state_dir).expect("failed to create state dir");
    fs::create_dir_all(&storage_dir).expect("failed to create storage dir");
    fs::create_dir_all(&symlink_dir).expect("failed to create symlink dir");
    fs::create_dir_all(&desktop_dir).expect("failed to create desktop dir");
    let _xdg_data_home = EnvVarGuard::set("XDG_DATA_HOME", &xdg_data_home);

    let paths = ConfigPaths {
        config_dir,
        state_dir,
    };

    let global_config = GlobalConfig {
        storage_dir: storage_dir.to_string_lossy().to_string(),
        symlink_dir: symlink_dir.to_string_lossy().to_string(),
        ..GlobalConfig::default()
    };
    let client = ureq::Agent::new_with_defaults();
    write_global_config(&paths);
    let checks = doctor::run(&paths, &global_config, &client);

    let config_check = doctor_check(&checks, "config_dir_access");
    assert!(matches!(config_check.status, DoctorStatus::Ok));
    assert!(config_check.detail.contains("exists and writable"));

    let apps_check = doctor_check(&checks, "apps_dir_access");
    assert!(matches!(apps_check.status, DoctorStatus::Warn));
    assert!(apps_check.detail.contains("missing"));

    let state_check = doctor_check(&checks, "state_dir_access");
    assert!(matches!(state_check.status, DoctorStatus::Ok));
    assert!(state_check.detail.contains("exists and writable"));

    let storage_check = doctor_check(&checks, "storage_dir_access");
    assert!(matches!(storage_check.status, DoctorStatus::Ok));
    assert!(storage_check.detail.contains("exists and writable"));

    let symlink_check = doctor_check(&checks, "symlink_dir_access");
    assert!(matches!(symlink_check.status, DoctorStatus::Ok));
    assert!(symlink_check.detail.contains("exists and writable"));

    let desktop_check = doctor_check(&checks, "desktop_applications_dir_access");
    assert!(matches!(desktop_check.status, DoctorStatus::Ok));
    assert!(desktop_check.detail.contains("exists and writable"));

    let cache_check = doctor_check(&checks, "cache_file_access");
    assert!(matches!(cache_check.status, DoctorStatus::Ok));
    assert!(cache_check.detail.contains("parent is writable"));

    let config_check = doctor_check(&checks, "config_parse");
    assert!(matches!(config_check.status, DoctorStatus::Ok));

    let recipes_check = doctor_check(&checks, "app_recipes_scan");
    assert!(matches!(recipes_check.status, DoctorStatus::Warn));
    assert!(
        recipes_check
            .detail
            .contains("0 valid recipe(s), 0 invalid recipe(s)")
    );

    let script_check = doctor_check(&checks, "script_resolvers_access");
    assert!(matches!(script_check.status, DoctorStatus::Ok));
    assert!(
        script_check
            .detail
            .contains("0 script resolver(s) accessible")
    );

    let general_check = doctor_check(&checks, "general_check");
    assert!(matches!(general_check.status, DoctorStatus::Ok));
    assert!(general_check.detail.contains("HOME set"));
    assert!(general_check.detail.contains("temp dir writable"));

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_directory_access_check_as_warn_when_not_writable() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_temp_dir("dir-access-readonly");
    let config_dir = root.join("config");
    let state_dir = root.join("state");
    let xdg_data_home = root.join("xdg-data");
    let desktop_dir = xdg_data_home.join("applications");
    fs::create_dir_all(&config_dir).expect("failed to create config dir");
    fs::create_dir_all(&state_dir).expect("failed to create state dir");
    fs::create_dir_all(&desktop_dir).expect("failed to create desktop dir");
    let _xdg_data_home = EnvVarGuard::set("XDG_DATA_HOME", &xdg_data_home);

    let mut permissions = fs::metadata(&desktop_dir)
        .expect("failed to stat desktop dir")
        .permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(&desktop_dir, permissions).expect("failed to make desktop dir read-only");

    let paths = ConfigPaths {
        config_dir,
        state_dir,
    };

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    write_global_config(&paths);
    let checks = doctor::run(&paths, &global_config, &client);

    let desktop_check = doctor_check(&checks, "desktop_applications_dir_access");
    assert!(matches!(desktop_check.status, DoctorStatus::Warn));
    assert!(desktop_check.detail.contains("exists but not writable"));

    let mut permissions = fs::metadata(&desktop_dir)
        .expect("failed to stat desktop dir")
        .permissions();
    permissions.set_mode(0o755);
    let _ = fs::set_permissions(&desktop_dir, permissions);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_config_and_apps_directory_access_as_warn_when_not_writable() {
    use std::os::unix::fs::PermissionsExt;

    let root = unique_temp_dir("config-apps-readonly");
    let config_dir = root.join("config");
    let state_dir = root.join("state");
    let apps_dir = config_dir.join("apps");
    fs::create_dir_all(&config_dir).expect("failed to create config dir");
    fs::create_dir_all(&state_dir).expect("failed to create state dir");
    fs::create_dir_all(&apps_dir).expect("failed to create apps dir");
    write_global_config(&ConfigPaths {
        config_dir: config_dir.clone(),
        state_dir: state_dir.clone(),
    });

    for path in [&config_dir, &apps_dir] {
        let mut permissions = fs::metadata(path)
            .expect("failed to stat dir")
            .permissions();
        permissions.set_mode(0o555);
        fs::set_permissions(path, permissions).expect("failed to make dir read-only");
    }

    let paths = ConfigPaths {
        config_dir: config_dir.clone(),
        state_dir,
    };
    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let config_check = doctor_check(&checks, "config_dir_access");
    assert!(matches!(config_check.status, DoctorStatus::Warn));
    assert!(config_check.detail.contains("exists but not writable"));

    let apps_check = doctor_check(&checks, "apps_dir_access");
    assert!(matches!(apps_check.status, DoctorStatus::Warn));
    assert!(apps_check.detail.contains("exists but not writable"));

    for path in [&config_dir, &apps_dir] {
        let mut permissions = fs::metadata(path)
            .expect("failed to stat dir")
            .permissions();
        permissions.set_mode(0o755);
        let _ = fs::set_permissions(path, permissions);
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn displays_home_paths_with_tilde() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_temp_dir("tilde-paths");
    let home_dir = root.join("home");
    let config_dir = home_dir.join(".config/fp-appimage-updater");
    let state_dir = home_dir.join(".local/state/fp-appimage-updater");
    let xdg_data_home = home_dir.join(".local/share");
    let desktop_dir = xdg_data_home.join("applications");
    fs::create_dir_all(&config_dir).expect("failed to create config dir");
    fs::create_dir_all(&state_dir).expect("failed to create state dir");
    fs::create_dir_all(&desktop_dir).expect("failed to create desktop dir");
    let _home = EnvVarGuard::set("HOME", &home_dir);
    let _xdg_data_home = EnvVarGuard::set("XDG_DATA_HOME", &xdg_data_home);

    let paths = ConfigPaths {
        config_dir: config_dir.clone(),
        state_dir,
    };
    write_global_config(&paths);

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let config_check = doctor_check(&checks, "config_dir_access");
    assert!(
        config_check
            .detail
            .contains("~/.config/fp-appimage-updater")
    );

    let desktop_check = doctor_check(&checks, "desktop_applications_dir_access");
    assert!(desktop_check.detail.contains("~/.local/share/applications"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_config_parse_as_warn_for_invalid_config() {
    let root = unique_temp_dir("invalid-config");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&paths.config_dir).expect("failed to create config dir");
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    fs::write(paths.global_config_path(), "storage_dir: [\n").expect("failed to write config");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let config_check = doctor_check(&checks, "config_parse");
    assert!(matches!(config_check.status, DoctorStatus::Warn));
    assert!(config_check.detail.contains("failed to load config"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_general_check_as_warn_when_installed_desktop_appimage_is_missing() {
    let root = unique_temp_dir("general-missing-appimage");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    write_app_recipe(
        &paths,
        "desktop.yml",
        "name: desktop-app\nstrategy:\n  strategy: direct\n  url: https://example.org/app.AppImage\n  check_method: etag\n",
    );
    fs::write(
        paths.cache_path(),
        "{\n  \"apps\": {\n    \"desktop-app\": {\n      \"file_path\": \"/tmp/does-not-exist.AppImage\"\n    }\n  }\n}\n",
    )
    .expect("failed to write cache");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let general_check = doctor_check(&checks, "general_check");
    assert!(matches!(general_check.status, DoctorStatus::Warn));
    assert!(
        general_check
            .detail
            .contains("installed desktop AppImage missing")
    );

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_appimage_runtime_as_ok_when_installed_appimage_responds() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_temp_dir("appimage-runtime-ok");
    let appimage_path = root.join("working.AppImage");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    write_app_recipe(
        &paths,
        "working.yml",
        "name: working-app\nstrategy:\n  strategy: direct\n  url: https://example.org/working.AppImage\n  check_method: etag\n",
    );
    write_executable(
        &appimage_path,
        "#!/bin/sh\nif [ \"$1\" = \"--appimage-version\" ]; then\n  exit 0\nfi\nexit 1\n",
    );
    fs::write(
        paths.cache_path(),
        format!(
            "{{\n  \"apps\": {{\n    \"working-app\": {{\n      \"file_path\": \"{}\"\n    }}\n  }}\n}}\n",
            appimage_path.display()
        ),
    )
    .expect("failed to write cache");
    let _os = EnvVarGuard::set_value("FP_APPIMAGE_UPDATER_OS_ID", "fedora");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let runtime_check = doctor_check(&checks, "appimage_runtime");
    assert!(matches!(runtime_check.status, DoctorStatus::Ok));
    assert!(runtime_check.detail.contains("runtime responds correctly"));

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_appimage_runtime_as_ok_on_nixos_when_appimage_run_extracts() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_temp_dir("appimage-runtime-nixos-ok");
    let bin_dir = root.join("bin");
    let appimage_path = root.join("nixos.AppImage");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&bin_dir).expect("failed to create bin dir");
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    write_app_recipe(
        &paths,
        "nixos.yml",
        "name: nixos-app\nstrategy:\n  strategy: direct\n  url: https://example.org/nixos.AppImage\n  check_method: etag\n",
    );
    write_executable(&appimage_path, "#!/bin/sh\nexit 1\n");
    write_executable(
        &bin_dir.join("appimage-run"),
        "#!/bin/sh\nif [ \"$1\" = \"-x\" ]; then\n  dest=\"$2\"\n  mkdir -p \"$dest\"\n  printf demo > \"$dest/extracted\"\n  exit 0\nfi\nexit 1\n",
    );
    let _path = EnvVarGuard::prepend_path(&bin_dir);
    let _os = EnvVarGuard::set_value("FP_APPIMAGE_UPDATER_OS_ID", "nixos");
    fs::write(
        paths.cache_path(),
        format!(
            "{{\n  \"apps\": {{\n    \"nixos-app\": {{\n      \"file_path\": \"{}\"\n    }}\n  }}\n}}\n",
            appimage_path.display()
        ),
    )
    .expect("failed to write cache");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let runtime_check = doctor_check(&checks, "appimage_runtime");
    assert!(matches!(runtime_check.status, DoctorStatus::Ok));
    assert!(
        runtime_check
            .detail
            .contains("appimage-run extracted metadata successfully")
    );

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_appimage_runtime_as_warn_when_installed_appimage_fails() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_temp_dir("appimage-runtime-fail");
    let appimage_path = root.join("broken.AppImage");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    write_app_recipe(
        &paths,
        "broken.yml",
        "name: broken-app\nstrategy:\n  strategy: direct\n  url: https://example.org/broken.AppImage\n  check_method: etag\n",
    );
    write_executable(
        &appimage_path,
        "#!/bin/sh\nif [ \"$1\" = \"--appimage-version\" ]; then\n  echo 'missing runtime support' >&2\n  exit 1\nfi\nexit 1\n",
    );
    fs::write(
        paths.cache_path(),
        format!(
            "{{\n  \"apps\": {{\n    \"broken-app\": {{\n      \"file_path\": \"{}\"\n    }}\n  }}\n}}\n",
            appimage_path.display()
        ),
    )
    .expect("failed to write cache");
    let _os = EnvVarGuard::set_value("FP_APPIMAGE_UPDATER_OS_ID", "fedora");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let runtime_check = doctor_check(&checks, "appimage_runtime");
    assert!(matches!(runtime_check.status, DoctorStatus::Warn));
    assert!(runtime_check.detail.contains("runtime check failed"));
    assert!(runtime_check.detail.contains("missing runtime support"));

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_appimage_runtime_as_warn_on_nixos_when_appimage_run_fails() {
    let _guard = env_lock().lock().unwrap_or_else(|err| err.into_inner());
    let root = unique_temp_dir("appimage-runtime-nixos-fail");
    let bin_dir = root.join("bin");
    let appimage_path = root.join("nixos-broken.AppImage");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&bin_dir).expect("failed to create bin dir");
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    write_app_recipe(
        &paths,
        "nixos-broken.yml",
        "name: nixos-broken-app\nstrategy:\n  strategy: direct\n  url: https://example.org/nixos-broken.AppImage\n  check_method: etag\n",
    );
    write_executable(&appimage_path, "#!/bin/sh\nexit 1\n");
    write_executable(
        &bin_dir.join("appimage-run"),
        "#!/bin/sh\nif [ \"$1\" = \"-x\" ]; then\n  echo 'extract failed' >&2\n  exit 127\nfi\nexit 1\n",
    );
    let _path = EnvVarGuard::prepend_path(&bin_dir);
    let _os = EnvVarGuard::set_value("FP_APPIMAGE_UPDATER_OS_ID", "nixos");
    fs::write(
        paths.cache_path(),
        format!(
            "{{\n  \"apps\": {{\n    \"nixos-broken-app\": {{\n      \"file_path\": \"{}\"\n    }}\n  }}\n}}\n",
            appimage_path.display()
        ),
    )
    .expect("failed to write cache");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let runtime_check = doctor_check(&checks, "appimage_runtime");
    assert!(matches!(runtime_check.status, DoctorStatus::Warn));
    assert!(runtime_check.detail.contains("extract failed"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_appimage_runtime_as_warn_when_no_installed_appimage_exists() {
    let root = unique_temp_dir("appimage-runtime-missing");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    write_app_recipe(
        &paths,
        "configured.yml",
        "name: configured-app\nstrategy:\n  strategy: direct\n  url: https://example.org/configured.AppImage\n  check_method: etag\n",
    );

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let runtime_check = doctor_check(&checks, "appimage_runtime");
    assert!(matches!(runtime_check.status, DoctorStatus::Warn));
    assert!(runtime_check.detail.contains("no installed AppImage found"));

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_symlink_directory_access_as_warn_when_not_writable() {
    use std::os::unix::fs::PermissionsExt;

    let root = unique_temp_dir("symlink-readonly");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    let symlink_dir = root.join("symlinks");
    fs::create_dir_all(&paths.config_dir).expect("failed to create config dir");
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    fs::create_dir_all(&symlink_dir).expect("failed to create symlink dir");
    write_global_config(&paths);

    let mut permissions = fs::metadata(&symlink_dir)
        .expect("failed to stat symlink dir")
        .permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(&symlink_dir, permissions).expect("failed to make symlink dir read-only");

    let global_config = GlobalConfig {
        symlink_dir: symlink_dir.to_string_lossy().to_string(),
        ..GlobalConfig::default()
    };
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let symlink_check = doctor_check(&checks, "symlink_dir_access");
    assert!(matches!(symlink_check.status, DoctorStatus::Warn));
    assert!(symlink_check.detail.contains("exists but not writable"));

    let mut permissions = fs::metadata(&symlink_dir)
        .expect("failed to stat symlink dir")
        .permissions();
    permissions.set_mode(0o755);
    let _ = fs::set_permissions(&symlink_dir, permissions);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_cache_file_access_as_warn_for_invalid_json() {
    let root = unique_temp_dir("invalid-cache");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    fs::write(paths.cache_path(), "{not-json").expect("failed to write cache");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let cache_check = doctor_check(&checks, "cache_file_access");
    assert!(matches!(cache_check.status, DoctorStatus::Warn));
    assert!(cache_check.detail.contains("invalid JSON"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_app_recipe_scan_counts_in_one_check() {
    let root = unique_temp_dir("recipes-scan");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    let apps_dir = paths.config_dir.join("apps");
    fs::create_dir_all(&apps_dir).expect("failed to create apps dir");
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    fs::write(
        apps_dir.join("valid.yml"),
        "name: valid-app\nstrategy:\n  strategy: direct\n  url: https://example.org/app.AppImage\n  check_method: etag\n",
    )
    .expect("failed to write valid app");
    fs::write(
        apps_dir.join("invalid.yml"),
        "name: invalid-app\nstrategy:\n  strategy: direct\n  url: [\n",
    )
    .expect("failed to write invalid app");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let recipes_check = doctor_check(&checks, "app_recipes_scan");
    assert!(matches!(recipes_check.status, DoctorStatus::Warn));
    assert!(
        recipes_check
            .detail
            .contains("1 valid recipe(s), 1 invalid recipe(s)")
    );

    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn reports_script_resolvers_access_as_warn_for_missing_script() {
    let root = unique_temp_dir("missing-script");
    let paths = ConfigPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    let apps_dir = paths.config_dir.join("apps");
    fs::create_dir_all(&apps_dir).expect("failed to create apps dir");
    fs::create_dir_all(&paths.state_dir).expect("failed to create state dir");
    write_global_config(&paths);
    fs::write(
        apps_dir.join("script.yml"),
        "name: script-app\nstrategy:\n  strategy: script\n  script_path: ./script-app/resolver.sh\n",
    )
    .expect("failed to write script app");

    let global_config = GlobalConfig::default();
    let client = ureq::Agent::new_with_defaults();
    let checks = doctor::run(&paths, &global_config, &client);

    let script_check = doctor_check(&checks, "script_resolvers_access");
    assert!(matches!(script_check.status, DoctorStatus::Warn));
    assert!(script_check.detail.contains("not accessible"));

    let _ = fs::remove_dir_all(&root);
}
