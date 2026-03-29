use crate::commands::helpers::*;
use crate::config;
use crate::output::{CheckApp, CheckResponse, CheckStatus, print_check_human, print_json};
use crate::parser::AppConfigLoadError;
use crate::resolvers;
use crate::state::{AppState, StateManager};
use anyhow::Result;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub enum CheckWorkResult {
    Ok {
        app: CheckApp,
        elapsed: Duration,
        cache_capabilities: Vec<String>,
        segmented_downloads: Option<bool>,
    },
    RateLimited {
        app: CheckApp,
        elapsed: Duration,
        rate_limited_until: Option<u64>,
    },
    Err {
        app: CheckApp,
        elapsed: Duration,
    },
}

pub fn run(
    app_configs: &[config::AppConfig],
    app_config_errors: &[AppConfigLoadError],
    global_config: &config::GlobalConfig,
    state_manager: &mut StateManager,
    client: &ureq::Agent,
    app_name: Option<&str>,
    json_output: bool,
    color_output: bool,
) -> Result<()> {
    let mut found = false;
    let mut results = Vec::new();
    let mut check_jobs = Vec::new();
    let mut rate_limit_note_needed = false;
    let now = now_epoch_seconds();

    for app in app_configs {
        if let Some(target) = app_name
            && app.name != *target
        {
            continue;
        }
        found = true;
        let state = snapshot_app_state(state_manager, &app.name, now);
        let rate_limited_until = state.rate_limited_until;
        let github_proxy = github_proxy_enabled(app, global_config);
        if rate_limit_enabled(app, global_config)
            && !(github_proxy && app_uses_github_forge(app))
            && matches!(rate_limited_until, Some(until) if until > now)
        {
            rate_limit_note_needed = true;
            results.push(CheckApp {
                name: app.name.clone(),
                status: CheckStatus::RateLimited,
                local_version: state.local_version.clone(),
                remote_version: None,
                download_url: None,
                rate_limited_until,
                capabilities: state.capabilities.clone(),
                error: None,
            });
            continue;
        }
        check_jobs.push((app.clone(), Some(state)));
    }

    let hard_max = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(MAX_CONCURRENT_CHECK_JOBS)
        .max(1);
    let mut worker_limit = hard_max;

    let (tx, rx) = mpsc::channel();
    let mut pending = check_jobs.into_iter();
    let mut active = 0usize;

    let spawn_check_worker = |app: config::AppConfig,
                              state: Option<AppState>,
                              client: ureq::Agent,
                              github_proxy: bool,
                              github_proxy_prefixes: Vec<String>,
                              global_config: config::GlobalConfig,
                              tx: mpsc::Sender<CheckWorkResult>| {
        std::thread::spawn(move || {
            let _ = tx.send(process_check_job(
                app,
                state,
                &client,
                github_proxy,
                github_proxy_prefixes,
                &global_config,
            ));
        });
    };

    while active < worker_limit {
        if let Some((app, state)) = pending.next() {
            let github_proxy = github_proxy_enabled(&app, global_config);
            let github_proxy_prefixes = github_proxy_prefixes(&app, global_config);
            spawn_check_worker(
                app,
                state,
                client.clone(),
                github_proxy,
                github_proxy_prefixes,
                global_config.clone(),
                tx.clone(),
            );
            active += 1;
        } else {
            break;
        }
    }

    while active > 0 {
        let result = rx.recv().expect("check worker panicked");
        active -= 1;

        match result {
            CheckWorkResult::Ok {
                app: app_result,
                elapsed,
                cache_capabilities,
                segmented_downloads,
            } => {
                let state = state_manager.get_app_mut(&app_result.name);
                cache_app_metadata(state, cache_capabilities, segmented_downloads);
                results.push(app_result);
                worker_limit = adapt_worker_limit(worker_limit, elapsed, pending.len(), hard_max);
            }
            CheckWorkResult::RateLimited {
                app: app_result,
                elapsed,
                rate_limited_until,
            } => {
                let state = state_manager.get_app_mut(&app_result.name);
                if let Some(until) = rate_limited_until {
                    state.rate_limited_until = Some(until);
                }
                results.push(app_result);
                rate_limit_note_needed = true;
                worker_limit = adapt_worker_limit(worker_limit, elapsed, pending.len(), hard_max);
            }
            CheckWorkResult::Err {
                app: app_result,
                elapsed,
            } => {
                results.push(app_result);
                worker_limit = adapt_worker_limit(worker_limit, elapsed, pending.len(), hard_max);
            }
        }

        if let Some((app, state)) = pending.next() {
            let github_proxy = github_proxy_enabled(&app, global_config);
            let github_proxy_prefixes = github_proxy_prefixes(&app, global_config);
            spawn_check_worker(
                app,
                state,
                client.clone(),
                github_proxy,
                github_proxy_prefixes,
                global_config.clone(),
                tx.clone(),
            );
            active += 1;
        }
    }

    for parse_error in app_config_errors {
        if !matches_target(app_name, parse_error.app_name.as_deref()) {
            continue;
        }
        found = true;
        results.push(CheckApp {
            name: parse_error
                .app_name
                .clone()
                .unwrap_or_else(|| parse_error.path.display().to_string()),
            status: CheckStatus::Error,
            local_version: None,
            remote_version: None,
            download_url: None,
            rate_limited_until: None,
            capabilities: Vec::new(),
            error: Some(format!(
                "Failed to parse app config at {}: {}",
                parse_error.path.display(),
                parse_error.message
            )),
        });
    }

    let error = if let Some(target) = app_name
        && !found
    {
        Some(format!("App '{}' not found in configuration.", target))
    } else {
        None
    };

    if json_output {
        print_json(&CheckResponse {
            command: "check",
            apps: results,
            error,
        })?;
    } else {
        print_check_human(
            &results,
            error.as_deref(),
            if rate_limit_note_needed {
                Some(rate_limit_note())
            } else {
                None
            },
            color_output,
        );
    }
    Ok(())
}

fn process_check_job(
    app: config::AppConfig,
    state: Option<AppState>,
    client: &ureq::Agent,
    github_proxy: bool,
    github_proxy_prefixes: Vec<String>,
    global_config: &config::GlobalConfig,
) -> CheckWorkResult {
    let started_at = Instant::now();
    let local_version = state.as_ref().and_then(|s| s.local_version.clone());

    match resolvers::check_for_updates(
        &app,
        state.as_ref(),
        client,
        github_proxy,
        &github_proxy_prefixes,
        global_config,
    ) {
        Ok(result) => {
            let mut capabilities = result.capabilities;
            let cache_capabilities = capabilities.clone();
            if matches!(
                app.zsync,
                Some(config::ZsyncConfig::Enabled(true)) | Some(config::ZsyncConfig::Url(_))
            ) {
                capabilities.push("zsync".to_string());
            }
            resolvers::dedupe_capabilities(&mut capabilities);
            let elapsed = started_at.elapsed();

            match result.update {
                Some(info) => CheckWorkResult::Ok {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::UpdateAvailable,
                        local_version,
                        remote_version: Some(info.version),
                        download_url: Some(info.download_url),
                        rate_limited_until: None,
                        capabilities,
                        error: None,
                    },
                    elapsed,
                    cache_capabilities,
                    segmented_downloads: result.segmented_downloads,
                },
                None => CheckWorkResult::Ok {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::UpToDate,
                        local_version,
                        remote_version: None,
                        download_url: None,
                        rate_limited_until: None,
                        capabilities,
                        error: None,
                    },
                    elapsed,
                    cache_capabilities,
                    segmented_downloads: result.segmented_downloads,
                },
            }
        }
        Err(e) => {
            let elapsed = started_at.elapsed();
            let rate_limited_until =
                rate_limit_from_error(&e).and_then(|info| info.until_epoch_seconds());

            if rate_limited_until.is_some() {
                CheckWorkResult::RateLimited {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::RateLimited,
                        local_version,
                        remote_version: None,
                        download_url: None,
                        rate_limited_until,
                        capabilities: Vec::new(),
                        error: None,
                    },
                    elapsed,
                    rate_limited_until,
                }
            } else {
                CheckWorkResult::Err {
                    app: CheckApp {
                        name: app.name,
                        status: CheckStatus::Error,
                        local_version,
                        remote_version: None,
                        download_url: None,
                        rate_limited_until: None,
                        capabilities: Vec::new(),
                        error: Some(format!("{:#}", e)),
                    },
                    elapsed,
                }
            }
        }
    }
}
