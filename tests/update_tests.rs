use fp_appimage_updater::update::{
    adapt_download_limit, median_speed_bps, should_retry_download_error,
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
