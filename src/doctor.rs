use std::path::Path;

use crate::config::{self, GlobalConfig};
use crate::integrator::expand_tilde;
use crate::lock::{self, LockState};
use crate::parser::{self, ConfigPaths};
use crate::{config::StrategyConfig, state::State};
use ureq::Agent;

#[derive(Debug)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy)]
pub enum DoctorStatus {
    Ok,
    Warn,
}

pub fn run(paths: &ConfigPaths, global_config: &GlobalConfig, client: &Agent) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let config_dir = paths.config_dir.clone();
    let apps_dir = paths.apps_dir();
    let state_dir = paths.state_dir.clone();
    let storage_dir = expand_tilde(&global_config.storage_dir);
    let symlink_dir = expand_tilde(&global_config.symlink_dir);
    let desktop_applications_dir = desktop_applications_dir();

    checks.push(dir_access_check("config_dir_access", &config_dir));
    checks.push(dir_access_check("apps_dir_access", &apps_dir));
    checks.push(dir_access_check("state_dir_access", &state_dir));
    checks.push(dir_access_check("storage_dir_access", &storage_dir));
    checks.push(dir_access_check("symlink_dir_access", &symlink_dir));
    checks.push(dir_access_check(
        "desktop_applications_dir_access",
        &desktop_applications_dir,
    ));
    checks.push(cache_file_access_check(&paths.cache_path()));
    checks.push(config_parse_check(paths));
    let app_load = parser::load_app_configs(paths);
    checks.push(app_recipes_scan_check(app_load.as_ref()));
    checks.push(script_resolvers_access_check(app_load.as_ref()));
    checks.push(general_check(paths, global_config, app_load.as_ref()));
    checks.push(lock_check(&paths.lock_path()));
    checks.push(github_api_check(global_config, client));

    checks
}

fn dir_access_check(name: &str, path: &Path) -> DoctorCheck {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            let writable = !metadata.permissions().readonly();
            DoctorCheck {
                name: name.to_string(),
                status: if writable {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                detail: if writable {
                    format!("exists and writable: {}", display_path(path))
                } else {
                    format!("exists but not writable: {}", display_path(path))
                },
            }
        }
        Err(_) => DoctorCheck {
            name: name.to_string(),
            status: DoctorStatus::Warn,
            detail: format!("missing: {}", display_path(path)),
        },
    }
}

fn cache_file_access_check(path: &Path) -> DoctorCheck {
    if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<State>(&content) {
                Ok(_) => DoctorCheck {
                    name: "cache_file_access".to_string(),
                    status: DoctorStatus::Ok,
                    detail: format!("exists, readable, and valid JSON: {}", display_path(path)),
                },
                Err(err) => DoctorCheck {
                    name: "cache_file_access".to_string(),
                    status: DoctorStatus::Warn,
                    detail: format!("invalid JSON at {} ({})", display_path(path), err),
                },
            },
            Err(err) => DoctorCheck {
                name: "cache_file_access".to_string(),
                status: DoctorStatus::Warn,
                detail: format!("not readable: {} ({})", display_path(path), err),
            },
        }
    } else if let Some(parent) = path.parent() {
        let parent_check = dir_access_check("cache_file_access", parent);
        DoctorCheck {
            name: "cache_file_access".to_string(),
            status: parent_check.status,
            detail: match parent_check.status {
                DoctorStatus::Ok => {
                    format!("missing, parent is writable: {}", display_path(path))
                }
                DoctorStatus::Warn => {
                    format!("missing, parent is not writable: {}", display_path(path))
                }
            },
        }
    } else {
        DoctorCheck {
            name: "cache_file_access".to_string(),
            status: DoctorStatus::Warn,
            detail: format!(
                "missing and parent could not be determined: {}",
                display_path(path)
            ),
        }
    }
}

fn config_parse_check(paths: &ConfigPaths) -> DoctorCheck {
    match parser::load_global_config(paths) {
        Ok(_) => DoctorCheck {
            name: "config_parse".to_string(),
            status: DoctorStatus::Ok,
            detail: format!(
                "config and secrets parsed successfully from {}",
                display_path(&paths.config_dir)
            ),
        },
        Err(err) => DoctorCheck {
            name: "config_parse".to_string(),
            status: DoctorStatus::Warn,
            detail: format!(
                "failed to load config from {} ({})",
                display_path(&paths.config_dir),
                err
            ),
        },
    }
}

fn app_recipes_scan_check(
    app_load: Result<&parser::AppConfigLoadResult, &anyhow::Error>,
) -> DoctorCheck {
    match app_load {
        Ok(result) => {
            let valid = result.apps.len();
            let invalid = result.errors.len();
            DoctorCheck {
                name: "app_recipes_scan".to_string(),
                status: if valid > 0 && invalid == 0 {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                },
                detail: format!("{valid} valid recipe(s), {invalid} invalid recipe(s)"),
            }
        }
        Err(err) => DoctorCheck {
            name: "app_recipes_scan".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("failed to scan app recipes ({})", err),
        },
    }
}

fn script_resolvers_access_check(
    app_load: Result<&parser::AppConfigLoadResult, &anyhow::Error>,
) -> DoctorCheck {
    let Ok(result) = app_load else {
        return DoctorCheck {
            name: "script_resolvers_access".to_string(),
            status: DoctorStatus::Warn,
            detail: "skipped because app recipes could not be loaded".to_string(),
        };
    };

    let mut script_count = 0usize;
    for app in &result.apps {
        let StrategyConfig::Script { script_path } = &app.strategy else {
            continue;
        };
        script_count += 1;
        let resolved = app.config_dir.join(script_path);
        match std::fs::metadata(&resolved) {
            Ok(metadata) => {
                if !metadata.is_file() {
                    return DoctorCheck {
                        name: "script_resolvers_access".to_string(),
                        status: DoctorStatus::Warn,
                        detail: format!(
                            "script resolver for {} is not a file: {}",
                            app.name,
                            display_path(&resolved)
                        ),
                    };
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o111 == 0 {
                        return DoctorCheck {
                            name: "script_resolvers_access".to_string(),
                            status: DoctorStatus::Warn,
                            detail: format!(
                                "script resolver for {} is not executable: {}",
                                app.name,
                                display_path(&resolved)
                            ),
                        };
                    };
                }
            }
            Err(err) => {
                return DoctorCheck {
                    name: "script_resolvers_access".to_string(),
                    status: DoctorStatus::Warn,
                    detail: format!(
                        "script resolver for {} is not accessible: {} ({})",
                        app.name,
                        display_path(&resolved),
                        err
                    ),
                };
            }
        }
    }

    DoctorCheck {
        name: "script_resolvers_access".to_string(),
        status: DoctorStatus::Ok,
        detail: format!("{script_count} script resolver(s) accessible"),
    }
}

fn general_check(
    paths: &ConfigPaths,
    global_config: &GlobalConfig,
    app_load: Result<&parser::AppConfigLoadResult, &anyhow::Error>,
) -> DoctorCheck {
    let mut problems = Vec::new();
    let home_ok = std::env::var_os("HOME").is_some();
    if !home_ok {
        problems.push("HOME missing".to_string());
    }

    let temp_dir = std::env::temp_dir();
    let temp_access = dir_access_check("general_check", &temp_dir);
    if !matches!(temp_access.status, DoctorStatus::Ok) {
        problems.push(format!("temp dir unavailable: {}", display_path(&temp_dir)));
    }

    let mut desktop_apps_checked = 0usize;
    let mut desktop_apps_ok = 0usize;
    let state = std::fs::read_to_string(paths.cache_path())
        .ok()
        .and_then(|content| serde_json::from_str::<State>(&content).ok())
        .unwrap_or_default();

    if let Ok(result) = app_load {
        for app in &result.apps {
            let should_integrate = app
                .integration
                .unwrap_or(global_config.manage_desktop_files);
            if !should_integrate {
                continue;
            }
            let Some(file_path) = state
                .apps
                .get(&app.name)
                .and_then(|app_state| app_state.file_path.as_deref())
            else {
                continue;
            };
            desktop_apps_checked += 1;
            let path = Path::new(file_path);
            match std::fs::metadata(path) {
                Ok(metadata) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if metadata.permissions().mode() & 0o111 != 0 {
                            desktop_apps_ok += 1;
                        } else {
                            problems.push(format!(
                                "installed desktop AppImage not executable: {}",
                                display_path(path)
                            ));
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = metadata;
                        desktop_apps_ok += 1;
                    }
                }
                Err(_) => problems.push(format!(
                    "installed desktop AppImage missing: {}",
                    display_path(path)
                )),
            }
        }
    }

    if problems.is_empty() {
        DoctorCheck {
            name: "general_check".to_string(),
            status: DoctorStatus::Ok,
            detail: format!(
                "HOME set, temp dir writable, desktop integrations checked: {}/{}",
                desktop_apps_ok, desktop_apps_checked
            ),
        }
    } else {
        DoctorCheck {
            name: "general_check".to_string(),
            status: DoctorStatus::Warn,
            detail: problems.join("; "),
        }
    }
}

fn desktop_applications_dir() -> std::path::PathBuf {
    let data_local_dir = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"));
    data_local_dir.join("applications")
}

fn display_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        if path == home {
            return "~".to_string();
        }
        if let Ok(stripped) = path.strip_prefix(&home) {
            if stripped.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

fn lock_check(path: &Path) -> DoctorCheck {
    match lock::FileLock::inspect(path) {
        Ok(LockState::Missing) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Ok,
            detail: format!("missing: {}", display_path(path)),
        },
        Ok(LockState::Active { pid }) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Ok,
            detail: format!("active lock held by pid {} at {}", pid, display_path(path)),
        },
        Ok(LockState::Stale { reason }) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("stale lock at {} ({})", display_path(path), reason),
        },
        Err(err) => DoctorCheck {
            name: "process_lock".to_string(),
            status: DoctorStatus::Warn,
            detail: format!("failed to inspect {} ({})", display_path(path), err),
        },
    }
}

fn github_api_check(global_config: &config::GlobalConfig, client: &Agent) -> DoctorCheck {
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
                    "working {} token (limit: {}, remaining: {})",
                    if global_config.github_token.is_some() {
                        "with"
                    } else {
                        "without"
                    },
                    limit,
                    remaining
                ),
            }
        }
        Err(err) => DoctorCheck {
            name: "github_api".to_string(),
            status: DoctorStatus::Warn,
            detail: format!(
                "failed {} token ({})",
                if global_config.github_token.is_some() {
                    "with"
                } else {
                    "without"
                },
                err
            ),
        },
    }
}
