use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::{IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use ureq::Agent;

use crate::config::{AppConfig, ZsyncConfig};
use crate::output::{print_progress, print_warning};
use crate::resolvers::UpdateInfo;
use crate::state::AppState;

const MAX_SEGMENTED_WORKERS: usize = 4;
const DOWNLOAD_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug)]
pub struct DownloadResult {
    pub path: PathBuf,
    pub segmented_downloads: Option<bool>,
    pub progress_completion_rendered: bool,
    pub downloaded_bytes: u64,
    pub download_elapsed: Option<Duration>,
}

fn interactive_progress_enabled(quiet: bool) -> bool {
    !quiet && std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

#[derive(Debug)]
struct ProgressUi {
    enabled: bool,
    next_id: usize,
    rendered_lines: usize,
    last_draw: Instant,
    entries: Vec<ProgressEntry>,
}

#[derive(Debug)]
struct ProgressEntry {
    id: usize,
    name: String,
    total: u64,
    downloaded: u64,
    last_percent: u64,
    started_at: Instant,
    finished_summary: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ProgressHandle {
    id: usize,
}

impl ProgressUi {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            next_id: 0,
            rendered_lines: 0,
            last_draw: Instant::now(),
            entries: Vec::new(),
        }
    }

    fn begin(&mut self, total: u64, name: &str) -> Option<ProgressHandle> {
        if !self.enabled || total == 0 {
            return None;
        }

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(ProgressEntry {
            id,
            name: name.to_string(),
            total,
            downloaded: 0,
            last_percent: 0,
            started_at: Instant::now(),
            finished_summary: None,
        });
        Some(ProgressHandle { id })
    }

    fn inc(&mut self, id: usize, amount: u64) -> Result<()> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.downloaded = entry.downloaded.saturating_add(amount);
            let percent = (entry.downloaded.saturating_mul(100) / entry.total).min(100);
            if percent >= entry.last_percent.saturating_add(5)
                || self.last_draw.elapsed() >= Duration::from_millis(120)
            {
                entry.last_percent = percent;
                self.draw()?;
            }
            Ok(())
        } else {
            Ok(())
        }
    }

    fn finish(&mut self, id: usize, summary: String) -> Result<bool> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.downloaded = entry.total;
            entry.finished_summary = Some(summary);
            entry.last_percent = 100;
            self.draw()?;
            Ok(true)
        } else {
            return Ok(false);
        }
    }

    fn draw(&mut self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let mut stderr = std::io::stderr();
        if self.entries.is_empty() {
            if self.rendered_lines > 0 {
                write!(stderr, "\x1b[{}A", self.rendered_lines)?;
                for _ in 0..self.rendered_lines {
                    write!(stderr, "\x1b[2K\r\x1b[1B")?;
                }
                write!(stderr, "\x1b[{}A", self.rendered_lines)?;
            }
            stderr.flush()?;
            self.rendered_lines = 0;
            self.last_draw = Instant::now();
            return Ok(());
        }

        if self.entries.len() == 1 {
            write!(stderr, "\r\x1b[2K")?;
            if let Some(summary) = &self.entries[0].finished_summary {
                let _ = summary;
                stderr.flush()?;
                self.entries.clear();
                self.rendered_lines = 0;
                self.last_draw = Instant::now();
                return Ok(());
            } else {
                write!(stderr, "{}", format_progress_line(&self.entries[0]))?;
                stderr.flush()?;
            }
            self.rendered_lines = 1;
            self.last_draw = Instant::now();
            return Ok(());
        }

        if self.rendered_lines > 0 {
            write!(stderr, "\x1b[{}A", self.rendered_lines)?;
        }

        for entry in &self.entries {
            write!(stderr, "\x1b[2K\r")?;
            if let Some(summary) = &entry.finished_summary {
                write!(stderr, "{}", summary)?;
            } else {
                write!(stderr, "{}", format_progress_line(entry))?;
            }
            writeln!(stderr)?;
        }

        stderr.flush()?;
        self.rendered_lines = self.entries.len();
        self.last_draw = Instant::now();
        Ok(())
    }
}

pub fn finalize_progress_output() -> Result<()> {
    if let Ok(mut ui) = progress_ui().lock() {
        ui.rendered_lines = 0;
    }
    Ok(())
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;

    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{}{}", bytes, UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

fn human_bytes_precise(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes.max(0.0);
    let mut unit = 0usize;

    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

fn human_rate(bytes_per_second: f64) -> String {
    if bytes_per_second <= 0.0 {
        return "0B/s".to_string();
    }

    format!("{}/s", human_bytes_precise(bytes_per_second))
}

fn format_progress_line(entry: &ProgressEntry) -> String {
    let percent = (entry.downloaded.saturating_mul(100) / entry.total).min(100);
    let bar_width = 24usize;
    let filled = ((percent as usize) * bar_width / 100).min(bar_width);
    let empty = bar_width.saturating_sub(filled);
    let elapsed = entry.started_at.elapsed().as_secs_f64().max(0.001);
    let speed = entry.downloaded as f64 / elapsed;

    format!(
        "{} [{}{}] {:>3}% ({}/{}, {})",
        entry.name,
        "=".repeat(filled),
        " ".repeat(empty),
        percent,
        human_bytes(entry.downloaded),
        human_bytes(entry.total),
        human_rate(speed)
    )
}

fn format_finished_line(name: &str, version: &str, bytes: u64, elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64().max(0.001);
    let speed = bytes as f64 / seconds;
    format!(
        "{} downloaded {} in {:.2}s ({}, {})",
        name,
        version,
        seconds,
        human_bytes(bytes),
        human_rate(speed)
    )
}

fn progress_ui() -> &'static Mutex<ProgressUi> {
    static UI: OnceLock<Mutex<ProgressUi>> = OnceLock::new();
    UI.get_or_init(|| Mutex::new(ProgressUi::new(false)))
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
    if let Some(handle) = handle {
        if let Ok(mut ui) = progress_ui().lock() {
            ui.inc(handle.id, amount)?;
        }
    }
    Ok(())
}

fn progress_finish(
    handle: Option<ProgressHandle>,
    name: &str,
    version: &str,
    bytes: u64,
    elapsed: Duration,
) -> Result<bool> {
    if let Some(handle) = handle {
        let summary = format_finished_line(name, version, bytes, elapsed);
        if let Ok(mut ui) = progress_ui().lock() {
            return ui.finish(handle.id, summary);
        }
    }
    Ok(false)
}

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
) -> Result<DownloadResult> {
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
    let mut segmented_downloads_support = state.and_then(|s| s.segmented_downloads);

    if let Some(zurl) = zsync_url
        && let Some(old_path_str) = state.and_then(|s| s.file_path.as_ref())
    {
        let old_path = Path::new(old_path_str);
        if old_path.exists() && try_zsync(&zurl, old_path, &tmp_path, quiet, colors) {
            zsync_success = true;
        }
    }

    if !zsync_success {
        let download_started = Instant::now();
        let mut progress_completion_rendered = false;
        let (segmented_success, segmented_support, segmented_progress_displayed) = if segmented_downloads {
            try_segmented_http_download(
                client,
                &app.name,
                &update_info.version,
                &update_info.download_url,
                &tmp_path,
                segmented_downloads_support,
                quiet,
                colors,
            )
        } else {
            (false, segmented_downloads_support, false)
        };
        segmented_downloads_support = segmented_support;
        progress_completion_rendered |= segmented_progress_displayed;

        if !segmented_success {
            let (_download_progress_displayed, download_progress_completion_rendered) = download_http(
                client,
                &app.name,
                &update_info.version,
                &update_info.download_url,
                &tmp_path,
                quiet,
                colors,
            )?;
            progress_completion_rendered |= download_progress_completion_rendered;
        }

        std::fs::rename(&tmp_path, &final_path)
            .context("Failed to rename tmp file to final destination")?;

        let downloaded_bytes = fs::metadata(&final_path).map(|meta| meta.len()).unwrap_or(0);

        return Ok(DownloadResult {
            path: final_path,
            segmented_downloads: segmented_downloads_support,
            progress_completion_rendered,
            downloaded_bytes,
            download_elapsed: Some(download_started.elapsed()),
        });
    }

    std::fs::rename(&tmp_path, &final_path)
        .context("Failed to rename tmp file to final destination")?;

    let downloaded_bytes = fs::metadata(&final_path).map(|meta| meta.len()).unwrap_or(0);

    Ok(DownloadResult {
        path: final_path,
        segmented_downloads: segmented_downloads_support,
        progress_completion_rendered: false,
        downloaded_bytes,
        download_elapsed: None,
    })
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
                    Ok((progress_displayed, progress_completion_rendered)) => {
                        (
                            true,
                            Some(true),
                            progress_displayed || progress_completion_rendered,
                        )
                    }
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
                        (false, segmented_support_from_error(&e).or(Some(true)), false)
                    }
                };
            }
            Ok(None) => {}
            Err(e) => {
                if !quiet {
                    print_warning(
                        &format!(
                            "Segmented download HEAD probe failed, falling back to range probe. ({:#})",
                            e
                        ),
                        colors,
                    );
                }
            }
        }
    }

    let (total_len, support) = match probe_range_support(client, url) {
        Ok(SegmentProbe::Supported(total_len)) => (total_len, Some(true)),
        Ok(SegmentProbe::Unsupported) => {
            return (false, Some(false), false);
        }
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
            return (false, cached_support, false);
        }
    };

    if total_len < 2 {
        if !quiet {
            print_warning(
                "Server supports ranges, but the file is too small to benefit. Falling back to full HTTP download.",
                colors,
            );
        }
        return (false, support, false);
    }

    match segmented_http_download(client, app_name, version, url, target_path, total_len, quiet, colors) {
        Ok((progress_displayed, progress_completion_rendered)) => {
            (
                true,
                support,
                progress_displayed || progress_completion_rendered,
            )
        }
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
            (false, segmented_support_from_error(&e).or(support), false)
        }
    }
}

enum SegmentProbe {
    Supported(u64),
    Unsupported,
}

fn probe_range_support(client: &Agent, url: &str) -> Result<SegmentProbe> {
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

fn head_content_length(client: &Agent, url: &str) -> Result<Option<u64>> {
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

fn segmented_http_download(
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
    let progress = new_progress_bar(total_len, app_name, quiet);
    let progress_displayed = progress.is_some();

    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(segment_count);

        for index in 0..segment_count {
            let client = client.clone();
            let url = url.to_string();
            let target_path = target_path.to_path_buf();
            let progress = progress;
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
                    let result = download_range(
                        &client,
                        &url,
                        &target_path,
                        start,
                        end,
                        &progress,
                    );
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

    if progress_displayed {
        let bytes = fs::metadata(target_path).map(|meta| meta.len()).unwrap_or(total_len);
        let completion_rendered =
            progress_finish(progress, app_name, version, bytes, started_at.elapsed())?;
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
    progress: &Option<ProgressHandle>,
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
        progress_update(*progress, bytes_read as u64)?;
    }
    Ok(())
}

fn download_http(
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
    let progress = new_progress_bar(total_len.unwrap_or(0), app_name, quiet);
    let progress_displayed = progress.is_some();
    let started_at = Instant::now();

    if let Some(_total_len) = total_len {
        let mut reader = response.into_body().into_reader();
        let mut buffer = [0u8; DOWNLOAD_BUFFER_SIZE];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            file.write_all(&buffer[..bytes_read])?;
            progress_update(progress, bytes_read as u64)?;
        }
    } else {
        std::io::copy(&mut response.into_body().into_reader(), &mut file)?;
    }

    if progress_displayed {
        let bytes = fs::metadata(target_path).map(|meta| meta.len()).unwrap_or(0);
        let completion_rendered = progress_finish(progress, app_name, version, bytes, started_at.elapsed())?;
        return Ok((progress_displayed, completion_rendered));
    }

    Ok((progress_displayed, false))
}
