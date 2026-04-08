use std::env;
use std::path::Path;

use crate::lock::{self, LockState};
use crate::parser::ConfigPaths;

#[derive(Debug)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
}

#[derive(Debug)]
pub enum DoctorStatus {
    Ok,
    Warn,
}

pub fn run(paths: &ConfigPaths, app_count: usize, parse_error_count: usize) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let config_dir = paths.config_dir.clone();
    let global_config = paths.global_config_path();
    let apps_dir = paths.apps_dir();
    let state_dir = paths.state_dir.clone();

    checks.push(path_check("config_dir", &config_dir));
    checks.push(path_check("apps_dir", &apps_dir));
    checks.push(path_check("global_config", &global_config));
    checks.push(path_check("state_dir", &state_dir));
    checks.push(lock_check(&paths.lock_path()));

    checks.push(DoctorCheck {
        name: "parsed_app_recipes".to_string(),
        status: if app_count > 0 {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        detail: format!("{app_count} valid recipe(s) loaded"),
    });

    checks.push(DoctorCheck {
        name: "invalid_app_recipes".to_string(),
        status: if parse_error_count == 0 {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        detail: format!("{parse_error_count} invalid recipe(s)"),
    });

    checks.push(DoctorCheck {
        name: "zsync_binary".to_string(),
        status: if command_exists("zsync") {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        detail: if command_exists("zsync") {
            "zsync found in PATH".to_string()
        } else {
            "zsync not found in PATH (delta updates may be unavailable)".to_string()
        },
    });

    checks
}

fn path_check(name: &str, path: &Path) -> DoctorCheck {
    let exists = path.exists();
    DoctorCheck {
        name: name.to_string(),
        status: if exists {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        detail: if exists {
            format!("exists: {}", path.display())
        } else {
            format!("missing: {}", path.display())
        },
    }
}

fn command_exists(cmd: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|dir| dir.join(cmd).is_file())
}

fn lock_check(path: &Path) -> DoctorCheck {
    match lock::FileLock::inspect(path) {
        Ok(LockState::Missing) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Ok,
            detail: format!("missing: {}", path.display()),
        },
        Ok(LockState::Active { pid }) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Ok,
            detail: format!("active lock held by pid {} at {}", pid, path.display()),
        },
        Ok(LockState::Stale { reason }) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("stale lock at {} ({})", path.display(), reason),
        },
        Err(err) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("failed to inspect {} ({})", path.display(), err),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn lock_check<'a>(checks: &'a [DoctorCheck]) -> &'a DoctorCheck {
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

        let checks = run(&paths, 0, 0);
        let lock_check = lock_check(&checks);

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
        let boot_id = {
            let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .expect("failed to read boot id");
            boot_id.trim().to_string()
        };
        fs::write(
            &lock_path,
            format!("pid={}\nboot_id={}\n", std::process::id(), boot_id),
        )
        .expect("failed to write lock");

        let paths = ConfigPaths {
            config_dir,
            state_dir,
        };

        let checks = run(&paths, 0, 0);
        let lock_check = lock_check(&checks);

        assert!(matches!(lock_check.status, DoctorStatus::Ok));
        assert!(lock_check.detail.contains("active lock held by pid"));

        let _ = fs::remove_dir_all(&root);
    }
}
