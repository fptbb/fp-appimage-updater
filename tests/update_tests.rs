use fp_appimage_updater::output::{UpdateApp, UpdateStatus};
use fp_appimage_updater::update::{
    adapt_download_limit, effective_show_all, filter_update_apps, median_speed_bps,
    should_retry_download_error,
};

#[test]
fn large_downloads_reduce_concurrency_more_aggressively() {
    assert_eq!(
        adapt_download_limit(
            6,
            512 * 1024 * 1024,
            Some(20.0 * 1024.0 * 1024.0),
            None,
            4,
            6
        ),
        2
    );
    assert_eq!(
        adapt_download_limit(
            6,
            1024 * 1024 * 1024,
            Some(20.0 * 1024.0 * 1024.0),
            None,
            4,
            6
        ),
        1
    );
}

#[test]
fn retryable_transport_errors_are_detected() {
    let err = anyhow::anyhow!("protocol: chunk length cannot be read as a number");
    assert!(should_retry_download_error(&err));
}

#[test]
fn median_speed_handles_even_and_odd_samples() {
    let odd = median_speed_bps(&[1.0, 9.0, 3.0]).expect("missing median");
    assert_eq!(odd, 3.0);

    let even = median_speed_bps(&[1.0, 9.0, 3.0, 7.0]).expect("missing median");
    assert_eq!(even, 5.0);
}

#[test]
fn update_results_hide_up_to_date_apps_by_default() {
    let apps = vec![
        UpdateApp {
            name: "updated".to_string(),
            status: UpdateStatus::Updated,
            from_version: Some("1.0.0".to_string()),
            to_version: Some("1.1.0".to_string()),
            path: Some("/tmp/updated.AppImage".to_string()),
            rate_limited_until: None,
            duration_seconds: Some(1.5),
            error: None,
        },
        UpdateApp {
            name: "current".to_string(),
            status: UpdateStatus::UpToDate,
            from_version: Some("1.1.0".to_string()),
            to_version: None,
            path: Some("/tmp/current.AppImage".to_string()),
            rate_limited_until: None,
            duration_seconds: None,
            error: None,
        },
        UpdateApp {
            name: "failed".to_string(),
            status: UpdateStatus::Error,
            from_version: Some("1.0.0".to_string()),
            to_version: Some("1.1.0".to_string()),
            path: None,
            rate_limited_until: None,
            duration_seconds: None,
            error: Some("boom".to_string()),
        },
    ];

    let filtered = filter_update_apps(&apps, false);
    assert_eq!(filtered.len(), 2);
    assert!(
        filtered
            .iter()
            .all(|app| !matches!(app.status, UpdateStatus::UpToDate))
    );

    let show_all = filter_update_apps(&apps, true);
    assert_eq!(show_all.len(), 3);
}

#[test]
fn show_all_is_enabled_by_either_config_or_cli_flag() {
    assert!(effective_show_all(true, false));
    assert!(effective_show_all(false, true));
    assert!(effective_show_all(true, true));
    assert!(!effective_show_all(false, false));
}
