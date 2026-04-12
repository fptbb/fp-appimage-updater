use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use ureq::Agent;

use crate::commands::helpers::build_http_agent;
use crate::downloader::progress::*;
use crate::output::print_warning;
use zsync_rs::ZsyncAssembly;

pub const MAX_SEGMENTED_WORKERS: usize = 4;
pub const DOWNLOAD_BUFFER_SIZE: usize = 64 * 1024;

/// Run zsync-rs against an existing AppImage and a manifest URL.
///
/// If the zsync backend fails, the caller should fall back to the normal HTTP path.
pub fn try_zsync(
    zsync_url: &str,
    old_file: &Path,
    target_file: &Path,
    app_name: &str,
    version: &str,
    was_update: bool,
    quiet: bool,
    colors: bool,
) -> (bool, bool) {
    let started_at = Instant::now();
    let mut assembly = match ZsyncAssembly::from_url(zsync_url, target_file) {
        Ok(assembly) => assembly,
        Err(_) => {
            if !quiet {
                print_warning(
                    "zsync could not initialize, falling back to full HTTP download.",
                    colors,
                );
            }
            return (false, false);
        }
    };

    let mut progress = ProgressGuard::new(
        new_progress_bar(assembly.progress().1, app_name, quiet),
        app_name,
        version,
    );
    let progress_displayed = progress.handle.is_some();
    let last_reported = Arc::new(Mutex::new(0u64));

    if let Some(handle) = progress.handle {
        let last_reported_cb = Arc::clone(&last_reported);
        assembly.set_progress_callback(move |done, _total| {
            if let Ok(mut last) = last_reported_cb.lock() {
                let delta = done.saturating_sub(*last);
                *last = done;
                if delta > 0
                    && let Ok(mut ui) = progress_ui().lock()
                {
                    let _ = ui.inc(handle.id, delta);
                }
            }
        });
    }

    if let Err(_) = assembly.submit_source_file(old_file) {
        if !quiet {
            print_warning(
                "zsync could not seed from the existing AppImage, falling back to full HTTP download.",
                colors,
            );
        }
        assembly.abort();
        return (false, false);
    }

    if progress_displayed {
        let (done, _) = assembly.progress();
        if done > 0
            && let Some(handle) = progress.handle
            && let Ok(mut last) = last_reported.lock()
        {
            let delta = done.saturating_sub(*last);
            *last = done;
            if delta > 0
                && let Ok(mut ui) = progress_ui().lock()
            {
                let _ = ui.inc(handle.id, delta);
            }
        }
    }

    while !assembly.is_complete() {
        match assembly.download_missing_blocks() {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => {
                if !quiet {
                    print_warning(
                        "zsync could not complete, falling back to full HTTP download.",
                        colors,
                    );
                }
                assembly.abort();
                return (false, false);
            }
        }
    }

    match assembly.complete() {
        Ok(()) => {
            let downloaded_bytes = fs::metadata(target_file)
                .map(|meta| meta.len())
                .unwrap_or(0);
            let elapsed = started_at.elapsed();

            if progress_displayed {
                let action = if was_update {
                    "updated to"
                } else {
                    "downloaded"
                };
                let seconds = elapsed.as_secs_f64().max(0.001);
                let speed = downloaded_bytes as f64 / seconds;
                let summary = format!(
                    "{} {} {} in {:.2}s ({}, {})",
                    app_name,
                    action,
                    version,
                    seconds,
                    human_bytes(downloaded_bytes),
                    human_rate(speed)
                );
                let _ = progress.finish_with_summary(downloaded_bytes, elapsed, summary);
            }

            (true, progress_displayed)
        }
        Err(_) => {
            if !quiet {
                print_warning(
                    "zsync could not complete, falling back to full HTTP download.",
                    colors,
                );
            }
            let _ = std::fs::remove_file(target_file.with_extension("zsync-tmp"));
            (false, false)
        }
    }
}

pub fn try_segmented_http_download(
    client: &Agent,
    app_name: &str,
    version: &str,
    url: &str,
    target_path: &Path,
    cached_support: Option<bool>,
    quiet: bool,
    colors: bool,
) -> (bool, Option<bool>, bool) {
    if matches!(cached_support, Some(false)) {
        return (false, cached_support, false);
    }

    if let Some(true) = cached_support {
        match head_content_length(client, url) {
            Ok(Some(total_len)) => {
                return match segmented_http_download(
                    client,
                    app_name,
                    version,
                    url,
                    target_path,
                    total_len,
                    quiet,
                    colors,
                ) {
                    Ok((progress_displayed, progress_completion_rendered)) => (
                        true,
                        Some(true),
                        progress_displayed || progress_completion_rendered,
                    ),
                    Err(e) => (
                        false,
                        segmented_support_from_error(&e).or(Some(true)),
                        false,
                    ),
                };
            }
            Ok(None) => {}
            Err(_) => {}
        }
    }

    let (total_len, support) = match probe_range_support(client, url) {
        Ok(SegmentProbe::Supported(total_len)) => (total_len, Some(true)),
        Ok(SegmentProbe::Unsupported) => {
            return (false, Some(false), false);
        }
        Err(_) => {
            return (false, cached_support, false);
        }
    };

    if total_len < 2 {
        return (false, support, false);
    }

    match segmented_http_download(
        client,
        app_name,
        version,
        url,
        target_path,
        total_len,
        quiet,
        colors,
    ) {
        Ok((progress_displayed, progress_completion_rendered)) => (
            true,
            support,
            progress_displayed || progress_completion_rendered,
        ),
        Err(e) => (false, segmented_support_from_error(&e).or(support), false),
    }
}

pub enum SegmentProbe {
    Supported(u64),
    Unsupported,
}

pub fn probe_range_support(client: &Agent, url: &str) -> Result<SegmentProbe> {
    let response = client
        .get(url)
        .header("Range", "bytes=0-0")
        .call()
        .with_context(|| format!("Failed to probe range support for {}", url))?;

    if response.status().as_u16() != 206 {
        return Ok(SegmentProbe::Unsupported);
    }

    let content_range = response
        .headers()
        .get("Content-Range")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("Missing Content-Range header in ranged response"))?;

    let total_len = parse_total_length_from_content_range(content_range)?;

    let mut body = response.into_body().into_reader();
    let mut sink = std::io::sink();
    std::io::copy(&mut body, &mut sink)?;

    Ok(SegmentProbe::Supported(total_len))
}

pub fn head_content_length(client: &Agent, url: &str) -> Result<Option<u64>> {
    let response = client
        .head(url)
        .call()
        .with_context(|| format!("Failed to read content length for {}", url))?;

    let content_length = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    Ok(content_length)
}

fn segmented_support_from_error(error: &anyhow::Error) -> Option<bool> {
    let message = format!("{:#}", error);
    if message.contains("instead of 206") || message.contains("ignored HTTP range requests") {
        Some(false)
    } else {
        None
    }
}

fn parse_total_length_from_content_range(content_range: &str) -> Result<u64> {
    let (_, total) = content_range
        .rsplit_once('/')
        .ok_or_else(|| anyhow::anyhow!("Invalid Content-Range header: {}", content_range))?;

    if total == "*" {
        bail!("Content-Range did not include a total length");
    }

    total
        .parse::<u64>()
        .with_context(|| format!("Invalid total length in Content-Range: {}", content_range))
}

pub fn segmented_http_download(
    client: &Agent,
    app_name: &str,
    version: &str,
    url: &str,
    target_path: &Path,
    total_len: u64,
    quiet: bool,
    _colors: bool,
) -> Result<(bool, bool)> {
    let segment_count = if total_len < MAX_SEGMENTED_WORKERS as u64 {
        total_len as usize
    } else {
        MAX_SEGMENTED_WORKERS
    };

    if segment_count < 2 {
        bail!("File too small for segmented download");
    }

    let segment_count_u64 = segment_count as u64;
    let chunk_size = total_len.div_ceil(segment_count_u64);
    let started_at = Instant::now();

    let file = File::create(target_path)?;
    file.set_len(total_len)?;
    drop(file);

    let mut results = Vec::with_capacity(segment_count);
    let mut progress = ProgressGuard::new(
        new_progress_bar(total_len, app_name, quiet),
        app_name,
        version,
    );
    let progress_displayed = progress.handle.is_some();

    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(segment_count);

        for index in 0..segment_count {
            let client = client.clone();
            let url = url.to_string();
            let target_path = target_path.to_path_buf();
            let handle = progress.handle;
            let start = index as u64 * chunk_size;
            let mut end = start + chunk_size - 1;
            if end >= total_len {
                end = total_len - 1;
            }

            if start > end {
                continue;
            }

            handles.push(
                scope.spawn(move || {
                    download_range(&client, &url, &target_path, start, end, &handle)
                }),
            );
        }

        for handle in handles {
            results.push(handle.join().expect("segmented download worker panicked"));
        }
    });

    for result in results {
        result?;
    }

    if progress_displayed {
        let bytes = fs::metadata(target_path)
            .map(|meta| meta.len())
            .unwrap_or(total_len);
        let completion_rendered = progress.finish(bytes, started_at.elapsed())?;
        return Ok((progress_displayed, completion_rendered));
    }

    Ok((progress_displayed, false))
}

fn download_range(
    client: &Agent,
    url: &str,
    target_path: &Path,
    start: u64,
    end: u64,
    handle: &Option<ProgressHandle>,
) -> Result<()> {
    let range_header = format!("bytes={}-{}", start, end);
    let response = client
        .get(url)
        .header("Range", &range_header)
        .call()
        .with_context(|| format!("Failed to download range {} for {}", range_header, url))?;

    if response.status().as_u16() != 206 {
        bail!(
            "Server returned {} instead of 206 for {}",
            response.status(),
            range_header
        );
    }

    let mut file = OpenOptions::new()
        .write(true)
        .open(target_path)
        .with_context(|| {
            format!(
                "Failed to open {} for segmented download",
                target_path.display()
            )
        })?;
    file.seek(SeekFrom::Start(start))?;

    let mut reader = response.into_body().into_reader();
    let mut buffer = [0u8; DOWNLOAD_BUFFER_SIZE];
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        file.write_all(&buffer[..bytes_read])?;
        progress_update(*handle, bytes_read as u64)?;
    }
    Ok(())
}

pub fn download_http(
    client: &Agent,
    app_name: &str,
    version: &str,
    url: &str,
    target_path: &Path,
    quiet: bool,
    _colors: bool,
) -> Result<(bool, bool)> {
    match download_http_once(client, app_name, version, url, target_path, quiet, _colors) {
        Ok(result) => Ok(result),
        Err(err) if is_retryable_chunk_completion_error(&format!("{:#}", err)) => {
            let fresh_client = build_http_agent();
            download_http_once(
                &fresh_client,
                app_name,
                version,
                url,
                target_path,
                quiet,
                _colors,
            )
        }
        Err(err) => Err(err),
    }
}

fn download_http_once(
    client: &Agent,
    app_name: &str,
    version: &str,
    url: &str,
    target_path: &Path,
    quiet: bool,
    _colors: bool,
) -> Result<(bool, bool)> {
    let response = client.get(url).call()?;
    let total_len = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let mut file = File::create(target_path)?;
    let mut progress = ProgressGuard::new(
        new_progress_bar(total_len.unwrap_or(0), app_name, quiet),
        app_name,
        version,
    );
    let progress_displayed = progress.handle.is_some();
    let started_at = Instant::now();

    if let Some(_total_len) = total_len {
        let mut reader = response.into_body().into_reader();
        let mut buffer = [0u8; DOWNLOAD_BUFFER_SIZE];
        let mut bytes_written = 0u64;

        loop {
            let bytes_read = match reader.read(&mut buffer) {
                Ok(bytes_read) => bytes_read,
                Err(err) => {
                    if total_len.is_some_and(|expected_len| bytes_written == expected_len)
                        && is_retryable_chunk_completion_error(&err.to_string())
                    {
                        break;
                    }
                    return Err(err.into());
                }
            };
            if bytes_read == 0 {
                break;
            }
            file.write_all(&buffer[..bytes_read])?;
            bytes_written = bytes_written.saturating_add(bytes_read as u64);
            progress_update(progress.handle, bytes_read as u64)?;
        }
    } else {
        std::io::copy(&mut response.into_body().into_reader(), &mut file)?;
    }

    if progress_displayed {
        let bytes = fs::metadata(target_path)
            .map(|meta| meta.len())
            .unwrap_or(0);
        let completion_rendered = progress.finish(bytes, started_at.elapsed())?;
        return Ok((progress_displayed, completion_rendered));
    }

    Ok((progress_displayed, false))
}

fn new_progress_bar(total: u64, app_name: &str, quiet: bool) -> Option<ProgressHandle> {
    let enabled = interactive_progress_enabled(quiet);
    let mut ui = progress_ui().lock().ok()?;
    if enabled {
        ui.enabled = true;
    }
    ui.begin(total, app_name)
}

fn progress_update(handle: Option<ProgressHandle>, amount: u64) -> Result<()> {
    if let Some(handle) = handle
        && let Ok(mut ui) = progress_ui().lock()
    {
        ui.inc(handle.id, amount)?;
    }
    Ok(())
}

fn is_retryable_chunk_completion_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
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
    ]
    .iter()
    .any(|needle| message.contains(needle))
}
