use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use ureq::Agent;

use crate::config::{AppConfig, ZsyncConfig};
use crate::output::{print_progress, print_warning};
use crate::resolvers::UpdateInfo;
use crate::state::AppState;

const MAX_SEGMENTED_WORKERS: usize = 4;

pub fn download_app(
    client: &Agent,
    app: &AppConfig,
    update_info: &UpdateInfo,
    storage_dir: &Path,
    naming_format: &str,
    state: Option<&AppState>,
    segmented_downloads: bool,
    quiet: bool,
    colors: bool,
) -> Result<PathBuf> {
    let actual_storage_dir = app
        .storage_dir
        .as_ref()
        .map(|s| crate::integrator::expand_tilde(s))
        .unwrap_or_else(|| storage_dir.to_path_buf());

    let file_name = naming_format
        .replace("{name}", &app.name)
        .replace("{version}", &update_info.version);

    let final_path = actual_storage_dir.join(&file_name);
    let tmp_path = actual_storage_dir.join(format!("{}.tmp", file_name));

    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let zsync_url = match &app.zsync {
        Some(ZsyncConfig::Enabled(true)) => Some(format!("{}.zsync", update_info.download_url)),
        Some(ZsyncConfig::Url(url)) => Some(url.clone()),
        _ => None,
    };

    let mut zsync_success = false;

    if let Some(zurl) = zsync_url
        && let Some(old_path_str) = state.and_then(|s| s.file_path.as_ref())
    {
        let old_path = Path::new(old_path_str);
        if old_path.exists() && try_zsync(&zurl, old_path, &tmp_path, quiet, colors) {
            zsync_success = true;
        }
    }

    if !zsync_success {
        let segmented_success = if segmented_downloads {
            try_segmented_http_download(
                client,
                &app.name,
                &update_info.download_url,
                &tmp_path,
                quiet,
                colors,
            )
        } else {
            false
        };

        if !segmented_success {
            download_http(client, &update_info.download_url, &tmp_path)?;
        }
    }

    std::fs::rename(&tmp_path, &final_path)
        .context("Failed to rename tmp file to final destination")?;

    Ok(final_path)
}

fn try_zsync(
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
                    "zsync failed or not found, falling back to full HTTP download.",
                    colors,
                );
            }
            false
        }
    }
}

fn try_segmented_http_download(
    client: &Agent,
    app_name: &str,
    url: &str,
    target_path: &Path,
    quiet: bool,
    colors: bool,
) -> bool {
    let total_len = match probe_range_support(client, url) {
        Ok(total_len) => total_len,
        Err(e) => {
            if !quiet {
                print_warning(
                    &format!(
                        "Segmented download probe failed, falling back to full HTTP download. ({:#})",
                        e
                    ),
                    colors,
                );
            }
            return false;
        }
    };

    if total_len < 2 {
        if !quiet {
            print_warning(
                "Server supports ranges, but the file is too small to benefit. Falling back to full HTTP download.",
                colors,
            );
        }
        return false;
    }

    match download_segmented_http(client, app_name, url, target_path, total_len, quiet, colors) {
        Ok(()) => true,
        Err(e) => {
            if !quiet {
                print_warning(
                    &format!(
                        "Segmented download failed, falling back to full HTTP download. ({:#})",
                        e
                    ),
                    colors,
                );
            }
            false
        }
    }
}

fn probe_range_support(client: &Agent, url: &str) -> Result<u64> {
    let response = client
        .get(url)
        .header("Range", "bytes=0-0")
        .call()
        .with_context(|| format!("Failed to probe range support for {}", url))?;

    if response.status().as_u16() != 206 {
        bail!("Server ignored HTTP range requests");
    }

    let content_range = response
        .headers()
        .get("Content-Range")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("Missing Content-Range header in ranged response"))?;

    let total_len = parse_total_length_from_content_range(content_range)?;

    // Consume the body so the connection can be returned to the pool.
    let mut body = response.into_body().into_reader();
    let mut sink = std::io::sink();
    std::io::copy(&mut body, &mut sink)?;

    Ok(total_len)
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

fn download_segmented_http(
    client: &Agent,
    app_name: &str,
    url: &str,
    target_path: &Path,
    total_len: u64,
    quiet: bool,
    colors: bool,
) -> Result<()> {
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

    let file = File::create(target_path)?;
    file.set_len(total_len)?;
    drop(file);

    let mut results = Vec::with_capacity(segment_count);
    let completed_segments = Arc::new(AtomicUsize::new(0));
    let app_name = app_name.to_string();

    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(segment_count);

        for index in 0..segment_count {
            let client = client.clone();
            let url = url.to_string();
            let target_path = target_path.to_path_buf();
            let app_name = app_name.clone();
            let completed_segments = Arc::clone(&completed_segments);
            let start = index as u64 * chunk_size;
            let mut end = start + chunk_size - 1;
            if end >= total_len {
                end = total_len - 1;
            }

            if start > end {
                continue;
            }

            handles
                .push(scope.spawn(move || {
                    let result = download_range(&client, &url, &target_path, start, end);
                    if result.is_ok() {
                        let completed = completed_segments.fetch_add(1, Ordering::SeqCst) + 1;
                        if !quiet {
                            print_progress(
                                &format!("{}: {}/{} chunks", app_name, completed, segment_count),
                                colors,
                            );
                        }
                    }
                    result
                }));
        }

        for handle in handles {
            results.push(handle.join().expect("segmented download worker panicked"));
        }
    });

    for result in results {
        result?;
    }

    Ok(())
}

fn download_range(
    client: &Agent,
    url: &str,
    target_path: &Path,
    start: u64,
    end: u64,
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
    std::io::copy(&mut reader, &mut file)?;
    Ok(())
}

fn download_http(client: &Agent, url: &str, target_path: &Path) -> Result<()> {
    let response = client.get(url).call()?;
    let mut file = File::create(target_path)?;

    std::io::copy(&mut response.into_body().into_reader(), &mut file)?;

    Ok(())
}
