use std::path::Path;

use crate::config::{self, GlobalConfig};
use crate::lock::{self, LockState};
use crate::parser::ConfigPaths;
use ureq::Agent;

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

pub fn run(
    paths: &ConfigPaths,
    global_config: &GlobalConfig,
    client: &Agent,
    app_count: usize,
    parse_error_count: usize,
) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let config_dir = paths.config_dir.clone();
    let global_config_path = paths.global_config_path();
    let apps_dir = paths.apps_dir();
    let state_dir = paths.state_dir.clone();

    checks.push(path_check("config_dir", &config_dir));
    checks.push(path_check("apps_dir", &apps_dir));
    checks.push(path_check("global_config", &global_config_path));
    checks.push(path_check("state_dir", &state_dir));
    checks.push(lock_check(&paths.lock_path()));
    checks.push(github_api_check(paths, global_config, client));

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

fn github_api_check(
    paths: &ConfigPaths,
    global_config: &config::GlobalConfig,
    client: &Agent,
) -> DoctorCheck {
    let source = if std::env::var("GITHUB_TOKEN").is_ok() {
        "environment variable GITHUB_TOKEN"
    } else if paths.secrets_path().exists() {
        "secrets.yml"
    } else {
        "none"
    };

    let mut request = client
        .get("https://api.github.com/rate_limit")
        .config()
        .http_status_as_error(false)
        .build();

    if let Some(token) = &global_config.github_token {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    match request.call() {
        Ok(resp) => {
            let headers = resp.headers();
            let limit = headers
                .get("x-ratelimit-limit")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("unknown");
            let remaining = headers
                .get("x-ratelimit-remaining")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("unknown");

            DoctorCheck {
                name: "github_api".to_string(),
                status: DoctorStatus::Ok,
                detail: format!(
                    "source: {}, working (limit: {}, remaining: {})",
                    source, limit, remaining
                ),
            }
        }
        Err(err) => DoctorCheck {
            name: "github_api".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("source: {}, failed ({})", source, err),
        },
    }
}
