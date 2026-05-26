#![allow(dead_code)]

use super::types::{UpdateDownloadJob, UpdateWorkResult};
use crate::state::AppState;
use std::collections::VecDeque;
use std::time::Duration;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Files at or above this size are considered "large" and get reduced concurrency.
const LARGE_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB
/// Files at or above this size are considered "very large" — single-worker only.
const VERY_LARGE_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB

/// Speed thresholds for worker scaling (bytes/sec).
const FAST_DOWNLOAD_BPS: f64 = 40.0 * 1024.0 * 1024.0; // 40 MiB/s
const SLOW_DOWNLOAD_BPS: f64 = 10.0 * 1024.0 * 1024.0; // 10 MiB/s
const HIGH_SPEED_DOWNLOAD_BPS: f64 = 20.0 * 1024.0 * 1024.0; // 20 MiB/s

/// Consecutive fast-speed ticks required before scaling *up* (avoids thrashing
/// on a momentary burst).
const SCALE_UP_HYSTERESIS: u32 = 2;
/// Consecutive slow-speed ticks required before scaling *down* (gives a
/// sluggish connection a chance to recover before cutting workers).
const SCALE_DOWN_HYSTERESIS: u32 = 3;

/// Base delay for exponential back-off.
pub const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);
/// Hard ceiling on computed back-off delay.
pub const RETRY_MAX_DELAY: Duration = Duration::from_secs(60);

// ── Worker-count adaptation ───────────────────────────────────────────────────

/// Decide how many parallel workers to run based on observed download speed.
///
/// **Improvements over original:**
/// - Hysteresis counters (`fast_ticks` / `slow_ticks`) prevent the worker
///   count from oscillating when speed hovers near a threshold.
/// - Scale-up step is proportional to how far above the fast threshold the
///   speed is (up to +3 workers at once), so a fast connection ramps quickly
///   instead of creeping up one slot per tick.
/// - Returns updated hysteresis counters so the caller can persist them.
///
/// # Arguments
/// * `current`           – current worker count
/// * `download_bps`      – most recent speed sample (bytes/sec), if available
/// * `peak_download_bps` – historical peak speed for this session
/// * `pending`           – number of jobs waiting to be picked up
/// * `hard_max`          – absolute ceiling on worker count
/// * `fast_ticks`        – consecutive ticks where speed was "fast"
/// * `slow_ticks`        – consecutive ticks where speed was "slow"
///
/// # Returns
/// `(new_worker_count, new_fast_ticks, new_slow_ticks)`
pub fn adapt_worker_limit_for_speed(
    current: usize,
    download_bps: Option<f64>,
    peak_download_bps: Option<f64>,
    pending: usize,
    hard_max: usize,
    fast_ticks: u32,
    slow_ticks: u32,
) -> (usize, u32, u32) {
    let fast_threshold = peak_download_bps
        .map(|peak| (peak * 0.75).max(FAST_DOWNLOAD_BPS))
        .unwrap_or(FAST_DOWNLOAD_BPS);
    let slow_threshold = peak_download_bps
        .map(|peak| (peak * 0.30).max(1.0).min(SLOW_DOWNLOAD_BPS))
        .unwrap_or(SLOW_DOWNLOAD_BPS);

    let Some(bps) = download_bps else {
        // No speed data yet — hold steady.
        return (current.clamp(1, hard_max), fast_ticks, slow_ticks);
    };

    if bps >= fast_threshold && pending > current {
        let new_fast = fast_ticks + 1;
        if new_fast >= SCALE_UP_HYSTERESIS && current < hard_max {
            // Proportional step: 1× → +1 worker, 2× → +2, capped at +3.
            let ratio = (bps / fast_threshold).min(3.0);
            let step = (ratio as usize).max(1);
            let next = (current + step).min(hard_max);
            return (next, 0, 0);
        }
        return (current, new_fast, 0);
    }

    if bps <= slow_threshold {
        let new_slow = slow_ticks + 1;
        if new_slow >= SCALE_DOWN_HYSTERESIS && current > 1 {
            return (current - 1, 0, 0);
        }
        return (current, 0, new_slow);
    }

    // Mid-zone: speed is acceptable, hold current count and clear counters.
    (current.clamp(1, hard_max), 0, 0)
}

// ── Download-level concurrency cap ───────────────────────────────────────────

/// Derive the maximum concurrent downloads, accounting for both real-time
/// speed signals and how large the in-flight download is.
///
/// **Improvements over original:**
/// - Large-file cap is *relaxed* when speed is low: if a single stream on a
///   512 MiB file is slow, allowing up to 3 workers may recover bandwidth.
/// - Threads hysteresis counters through so callers can track state.
///
/// # Returns
/// `(new_limit, new_fast_ticks, new_slow_ticks)`
pub fn adapt_download_limit(
    current: usize,
    downloaded_bytes: u64,
    download_bps: Option<f64>,
    peak_download_bps: Option<f64>,
    pending: usize,
    hard_max: usize,
    fast_ticks: u32,
    slow_ticks: u32,
) -> (usize, u32, u32) {
    let (mut next, new_fast, new_slow) = match download_bps {
        Some(bps) => adapt_worker_limit_for_speed(
            current,
            Some(bps),
            peak_download_bps,
            pending,
            hard_max,
            fast_ticks,
            slow_ticks,
        ),
        None => (current, fast_ticks, slow_ticks),
    };

    if downloaded_bytes >= VERY_LARGE_DOWNLOAD_BYTES {
        // A single saturated stream is almost always more efficient than
        // competing connections for files this size.
        next = 1;
    } else if downloaded_bytes >= LARGE_DOWNLOAD_BYTES {
        let is_high_speed = download_bps.is_some_and(|bps| {
            let peak = peak_download_bps.unwrap_or(bps);
            bps >= (peak * 0.75).max(HIGH_SPEED_DOWNLOAD_BPS)
        });
        // High speed + large file: server is already saturated — stay narrow.
        // Low speed + large file: open another connection to recover throughput.
        next = next.min(if is_high_speed { 2 } else { 3 });
    }

    (next.clamp(1, hard_max), new_fast, new_slow)
}

// ── Speed estimation ──────────────────────────────────────────────────────────

/// Compute the median of a sample slice. Returns `None` for empty input.
///
/// Good for stable, outlier-resistant speed display.  Feed into `ema_speed_bps`
/// for a smooth real-time signal.
pub fn median_speed_bps(samples: &[f64]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut values = samples.to_vec();
    values.sort_by(|a, b| a.total_cmp(b));
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

/// Exponential moving average (EMA) of download speed.
///
/// `alpha` ∈ (0, 1]: lower values react slowly (smooth), higher values track
/// bursts more tightly.  A value of `0.2` is a reasonable starting point.
///
/// Suggested usage: feed the short-window median into this EMA each tick, then
/// use the EMA output as `download_bps` in the adaptation functions.
pub fn ema_speed_bps(previous_ema: Option<f64>, new_sample: f64, alpha: f64) -> f64 {
    let alpha = alpha.clamp(0.01, 1.0);
    match previous_ema {
        None => new_sample,
        Some(prev) => alpha * new_sample + (1.0 - alpha) * prev,
    }
}

/// Compute the Nth percentile of a sample slice (0.0–100.0).
///
/// Use this to track peak speed (e.g. p95) without being skewed by transient
/// bursts — pass the result as `peak_download_bps` to the adaptation functions.
pub fn percentile_speed_bps(samples: &[f64], percentile: f64) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut values = samples.to_vec();
    values.sort_by(|a, b| a.total_cmp(b));
    let index = ((percentile / 100.0) * (values.len() - 1) as f64).round() as usize;
    Some(values[index.min(values.len() - 1)])
}

// ── Provider utilities ────────────────────────────────────────────────────────

/// Canonicalize a download URL to a short provider key.
///
/// **Improvement over original:** strips port numbers so that
/// `"github.com:443"` correctly maps to `"github"` rather than a new key.
pub fn download_provider_key(url: &str) -> String {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();

    // Strip port suffix before pattern matching.
    let host = host.split(':').next().unwrap_or(&host);

    if host.ends_with("github.com") || host.ends_with("githubusercontent.com") {
        "github".to_string()
    } else {
        host.to_string()
    }
}

// ── Job queue ─────────────────────────────────────────────────────────────────

/// Insert `job` into the queue while maintaining ascending sort order.
///
/// Sort priority (lowest key = highest priority):
///   1. Small files before large — faster time-to-completion.
///   2. Within a size bucket: smallest estimated bytes first.
///   3. Non-retry jobs before retry jobs — retries have already failed once.
///   4. Provider key — stable tie-breaker, promotes round-robin across providers.
pub(crate) fn insert_download_job_sorted(
    queue: &mut VecDeque<UpdateDownloadJob>,
    job: UpdateDownloadJob,
) {
    let new_key = download_job_sort_key(&job);
    let index = queue
        .iter()
        .position(|existing| download_job_sort_key(existing) > new_key)
        .unwrap_or(queue.len());
    queue.insert(index, job);
}

fn download_job_sort_key(job: &UpdateDownloadJob) -> (u8, u64, u8, String) {
    let large_bucket = u8::from(is_large_download(job.estimated_download_bytes));
    // Unknown size sorts last within its bucket (treat as the largest).
    let size_key = job.estimated_download_bytes.unwrap_or(u64::MAX);
    let retry_key = u8::from(job.retry_without_segmented_downloads);
    let provider_key = job.provider.clone();
    (large_bucket, size_key, retry_key, provider_key)
}

/// Returns `true` if the estimated size qualifies as a "large" download.
pub(crate) fn is_large_download(estimated_download_bytes: Option<u64>) -> bool {
    estimated_download_bytes.is_some_and(|b| b >= LARGE_DOWNLOAD_BYTES)
}

// ── AppState helpers ──────────────────────────────────────────────────────────

pub fn estimate_download_bytes(state: &AppState, current_path: Option<&str>) -> Option<u64> {
    state.download_bytes.or_else(|| {
        current_path
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|meta| meta.len())
    })
}

pub fn update_work_elapsed(result: &UpdateWorkResult) -> Duration {
    match result {
        UpdateWorkResult::ReadyToDownload { elapsed, .. } => *elapsed,
        UpdateWorkResult::Updated { elapsed, .. }
        | UpdateWorkResult::UpToDate { elapsed, .. }
        | UpdateWorkResult::Error { elapsed, .. }
        | UpdateWorkResult::RateLimited { elapsed, .. } => *elapsed,
    }
}

// ── Retry logic ───────────────────────────────────────────────────────────────

/// Returns `true` if the error is a transient network problem worth retrying.
///
/// **New patterns vs original:**
/// - `"operation timed out"` (macOS phrasing for `ETIMEDOUT`)
/// - `"end of file"` (alternative EOF phrasing)
/// - `"incomplete message"` (partial TLS record)
/// - `"os error 32"` (`EPIPE` on Linux)
/// - `"os error 104"` (`ECONNRESET` on Linux)
pub fn should_retry_download_error(error: &anyhow::Error) -> bool {
    let message = format!("{:#}", error).to_ascii_lowercase();
    RETRYABLE_ERROR_NEEDLES
        .iter()
        .any(|needle| message.contains(needle))
}

/// Returns `true` if the error looks like a server-side rate limit (HTTP 429,
/// `Retry-After`, etc.) — distinct from a transient network failure.
///
/// Callers should apply a longer back-off for rate-limit errors and avoid
/// opening new connections to the same provider for a cooling-off period.
pub fn is_rate_limit_error(error: &anyhow::Error) -> bool {
    let message = format!("{:#}", error).to_ascii_lowercase();
    [
        "429",
        "too many requests",
        "rate limit",
        "rate-limit",
        "retry-after",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

/// Compute an exponential back-off delay for retry attempt `attempt` (0-indexed).
///
/// Caps at [`RETRY_MAX_DELAY`].  Add random jitter at the call-site (e.g. ±20 %)
/// to prevent a thundering herd when multiple workers retry simultaneously:
///
/// ```rust,ignore
/// let jitter = rand::random::<f64>() * 0.4 - 0.2; // −20 % … +20 %
/// let delay = retry_backoff_delay(attempt).mul_f64(1.0 + jitter);
/// ```
pub fn retry_backoff_delay(attempt: u32) -> Duration {
    let secs = RETRY_BASE_DELAY.as_secs_f64() * 2_f64.powi(attempt as i32);
    Duration::from_secs_f64(secs.min(RETRY_MAX_DELAY.as_secs_f64()))
}

const RETRYABLE_ERROR_NEEDLES: &[&str] = &[
    // HTTP chunked-encoding corruption
    "chunk length cannot be read as a number",
    "chunk expected crlf as next character",
    "chunk length is not ascii",
    "body content after finish",
    "body is chunked",
    // Connection-level resets
    "unexpected eof",
    "end of file",
    "incomplete message",
    "connection reset",
    "connection aborted",
    "broken pipe",
    "os error 32",  // EPIPE   (Linux)
    "os error 104", // ECONNRESET (Linux)
    // Timeouts
    "timed out",
    "timeout",
    "operation timed out", // macOS phrasing
    // Buffer / read errors
    "failed to fill whole buffer",
];

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── adapt_worker_limit_for_speed ─────────────────────────────────────────

    #[test]
    fn scales_up_proportionally_after_hysteresis() {
        // First tick at fast speed — should not scale yet.
        let (count, fast, slow) =
            adapt_worker_limit_for_speed(2, Some(80.0 * 1024.0 * 1024.0), None, 5, 8, 0, 0);
        assert_eq!(count, 2, "should not scale on first fast tick");
        assert_eq!(fast, 1);
        assert_eq!(slow, 0);

        // Second tick — hysteresis met, should scale up proportionally (2× fast threshold → +2).
        let (count2, fast2, _) =
            adapt_worker_limit_for_speed(2, Some(80.0 * 1024.0 * 1024.0), None, 5, 8, fast, slow);
        assert!(count2 > 2, "should scale up after hysteresis met");
        assert_eq!(fast2, 0, "fast counter resets after scaling");
    }

    #[test]
    fn scales_down_only_after_slow_hysteresis() {
        // Ticks 1 & 2 — should not scale yet.
        let (c1, f1, s1) =
            adapt_worker_limit_for_speed(4, Some(1.0 * 1024.0 * 1024.0), None, 2, 8, 0, 0);
        assert_eq!(c1, 4);
        let (c2, _, s2) =
            adapt_worker_limit_for_speed(4, Some(1.0 * 1024.0 * 1024.0), None, 2, 8, f1, s1);
        assert_eq!(c2, 4);
        // Tick 3 — threshold reached, scale down.
        let (c3, _, _) =
            adapt_worker_limit_for_speed(4, Some(1.0 * 1024.0 * 1024.0), None, 2, 8, 0, s2);
        assert_eq!(c3, 3);
    }

    #[test]
    fn no_scale_up_without_pending_jobs() {
        let (count, _, _) =
            adapt_worker_limit_for_speed(4, Some(80.0 * 1024.0 * 1024.0), None, 0, 8, 2, 0);
        assert_eq!(count, 4, "should not add workers if no pending jobs");
    }

    #[test]
    fn never_exceeds_hard_max() {
        let (count, _, _) =
            adapt_worker_limit_for_speed(8, Some(80.0 * 1024.0 * 1024.0), None, 10, 8, 5, 0);
        assert_eq!(count, 8);
    }

    // ── adapt_download_limit ─────────────────────────────────────────────────

    #[test]
    fn very_large_download_forces_single_worker() {
        let (limit, _, _) = adapt_download_limit(
            4,
            VERY_LARGE_DOWNLOAD_BYTES,
            Some(50.0 * 1024.0 * 1024.0),
            None,
            6,
            8,
            0,
            0,
        );
        assert_eq!(limit, 1);
    }

    #[test]
    fn large_slow_download_allows_up_to_three() {
        // Low speed on a large file → relax cap to 3.
        let (limit, _, _) = adapt_download_limit(
            1,
            LARGE_DOWNLOAD_BYTES,
            Some(2.0 * 1024.0 * 1024.0), // well below slow threshold
            None,
            6,
            8,
            0,
            SCALE_DOWN_HYSTERESIS, // trigger scale-down to 0 → expect clamp at 1
        );
        assert!(limit >= 1 && limit <= 3);
    }

    // ── speed helpers ────────────────────────────────────────────────────────

    #[test]
    fn median_even_samples() {
        let samples = vec![1.0, 3.0, 5.0, 7.0];
        assert_eq!(median_speed_bps(&samples), Some(4.0));
    }

    #[test]
    fn median_odd_samples() {
        let samples = vec![1.0, 2.0, 9.0];
        assert_eq!(median_speed_bps(&samples), Some(2.0));
    }

    #[test]
    fn ema_initialises_on_first_sample() {
        assert_eq!(ema_speed_bps(None, 100.0, 0.3), 100.0);
    }

    #[test]
    fn ema_smooths_correctly() {
        let v = ema_speed_bps(Some(100.0), 200.0, 0.5);
        assert_eq!(v, 150.0);
    }

    #[test]
    fn percentile_p50_equals_median() {
        let samples = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(
            percentile_speed_bps(&samples, 50.0),
            median_speed_bps(&samples)
        );
    }

    // ── download_provider_key ────────────────────────────────────────────────

    #[test]
    fn strips_port_from_provider_key() {
        assert_eq!(
            download_provider_key("https://github.com:443/foo/bar"),
            "github"
        );
    }

    #[test]
    fn github_raw_maps_to_github() {
        assert_eq!(
            download_provider_key("https://raw.githubusercontent.com/user/repo/file"),
            "github"
        );
    }

    #[test]
    fn non_github_host_returned_as_is() {
        assert_eq!(
            download_provider_key("https://releases.example.com/app.AppImage"),
            "releases.example.com"
        );
    }

    // ── retry helpers ────────────────────────────────────────────────────────

    #[test]
    fn retry_backoff_grows_exponentially() {
        let d0 = retry_backoff_delay(0);
        let d1 = retry_backoff_delay(1);
        let d2 = retry_backoff_delay(2);
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    #[test]
    fn retry_backoff_caps_at_max() {
        assert_eq!(retry_backoff_delay(100), RETRY_MAX_DELAY);
    }

    #[test]
    fn rate_limit_not_treated_as_retryable_network_error() {
        // Rate-limit errors should be handled separately, not retried immediately.
        let err = anyhow::anyhow!("HTTP 429 Too Many Requests");
        assert!(!should_retry_download_error(&err));
        assert!(is_rate_limit_error(&err));
    }

    #[test]
    fn connection_reset_is_retryable() {
        let err = anyhow::anyhow!("connection reset by peer");
        assert!(should_retry_download_error(&err));
    }

    #[test]
    fn os_error_104_is_retryable() {
        let err = anyhow::anyhow!("os error 104");
        assert!(should_retry_download_error(&err));
    }
}
