use super::types::{UpdateDownloadJob, UpdateWorkResult};
use crate::state::AppState;
use std::collections::VecDeque;
use std::time::Duration;

pub fn adapt_worker_limit_for_speed(
    current: usize,
    download_bps: Option<f64>,
    peak_download_bps: Option<f64>,
    pending: usize,
    hard_max: usize,
) -> usize {
    let mut next = current;
    let fast_threshold = peak_download_bps
        .map(|peak| (peak * 0.75).max(FAST_DOWNLOAD_BPS))
        .unwrap_or(FAST_DOWNLOAD_BPS);
    let slow_threshold = peak_download_bps
        .map(|peak| (peak * 0.30).max(1.0).min(SLOW_DOWNLOAD_BPS))
        .unwrap_or(SLOW_DOWNLOAD_BPS);

    if let Some(bps) = download_bps {
        if bps >= fast_threshold && pending > current && next < hard_max {
            next += 1;
        } else if bps <= slow_threshold && next > 1 {
            next -= 1;
        }
    }

    next.clamp(1, hard_max)
}

pub fn adapt_download_limit(
    current: usize,
    downloaded_bytes: u64,
    download_bps: Option<f64>,
    peak_download_bps: Option<f64>,
    pending: usize,
    hard_max: usize,
) -> usize {
    let mut next = match download_bps {
        Some(download_bps) => adapt_worker_limit_for_speed(
            current,
            Some(download_bps),
            peak_download_bps,
            pending,
            hard_max,
        ),
        None => current,
    };

    if downloaded_bytes >= VERY_LARGE_DOWNLOAD_BYTES {
        next = 1;
    } else if downloaded_bytes >= LARGE_DOWNLOAD_BYTES {
        if download_bps.is_some_and(|bps| {
            let peak = peak_download_bps.unwrap_or(bps);
            bps >= (peak * 0.75).max(HIGH_SPEED_DOWNLOAD_BPS)
        }) {
            next = next.min(2);
        } else {
            next = next.min(3);
        }
    }

    next.clamp(1, hard_max)
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

pub fn download_provider_key(url: &str) -> String {
    let host = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();

    if host.ends_with("github.com") || host.ends_with("githubusercontent.com") {
        "github".to_string()
    } else {
        host
    }
}

pub fn estimate_download_bytes(state: &AppState, current_path: Option<&str>) -> Option<u64> {
    state.download_bytes.or_else(|| {
        current_path
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|meta| meta.len())
    })
}

pub(crate) fn is_large_download(estimated_download_bytes: Option<u64>) -> bool {
    estimated_download_bytes.is_some_and(|bytes| bytes >= LARGE_DOWNLOAD_BYTES)
}

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
    let large_bucket = if is_large_download(job.estimated_download_bytes) {
        1
    } else {
        0
    };
    let size_key = job.estimated_download_bytes.unwrap_or(u64::MAX);
    let retry_key = if job.retry_without_segmented_downloads {
        1
    } else {
        0
    };
    let provider_key = job.provider.clone();
    (large_bucket, size_key, retry_key, provider_key)
}

pub fn should_retry_download_error(error: &anyhow::Error) -> bool {
    let message = format!("{:#}", error).to_ascii_lowercase();
    [
        "chunk length cannot be read as a number",
        "chunk expected crlf as next character",
        "chunk length is not ascii",
        "body content after finish",
        "unexpected eof",
        "connection reset",
        "connection aborted",
        "broken pipe",
        "timed out",
        "failed to fill whole buffer",
        "body is chunked",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

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

const LARGE_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const VERY_LARGE_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const HIGH_SPEED_DOWNLOAD_BPS: f64 = 20.0 * 1024.0 * 1024.0;
const FAST_DOWNLOAD_BPS: f64 = 40.0 * 1024.0 * 1024.0;
const SLOW_DOWNLOAD_BPS: f64 = 10.0 * 1024.0 * 1024.0;
