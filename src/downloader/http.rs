use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Instant;
use ureq::Agent;

use crate::downloader::progress::*;
use crate::output::{print_progress, print_warning};

pub const MAX_SEGMENTED_WORKERS: usize = 4;
pub const DOWNLOAD_BUFFER_SIZE: usize = 64 * 1024;

/// Run the external `zsync` binary against an existing AppImage and a manifest URL.
///
/// If `zsync` fails or is missing, the caller should fall back to the normal HTTP path.
pub fn try_zsync(
    zsync_url: &str,
    old_file: &Path,
    target_file: &Path,
    quiet: bool,
    colors: bool,
) -> bool {
    if !quiet {
        print_progress(
            &format!("Attempting zsync update using: {}", zsync_url),
            colors,
        );
    }

    let status = Command::new("zsync")
        .arg("-i")
        .arg(old_file)
        .arg("-o")
        .arg(target_file)
        .arg(zsync_url)
        .status();

    match status {
        Ok(s) if s.success() => {
            if !quiet {
                print_progress("Successfully updated via zsync!", colors);
            }
            true
        }
        _ => {
            if !quiet {
                print_warning(
                    "zsync could not complete, falling back to full HTTP download.",
                    colors,
                );
            }
            false
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
