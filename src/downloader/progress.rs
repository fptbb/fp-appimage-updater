use anyhow::Result;
use std::io::{IsTerminal, Write};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub struct ProgressUi {
    pub enabled: bool,
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
}

#[derive(Debug, Clone, Copy)]
pub struct ProgressHandle {
    pub id: usize,
}

pub struct ProgressGuard {
    pub handle: Option<ProgressHandle>,
    pub app_name: String,
    pub version: String,
    finished: bool,
}

impl ProgressGuard {
    pub fn new(handle: Option<ProgressHandle>, app_name: &str, version: &str) -> Self {
        Self {
            handle,
            app_name: app_name.to_string(),
            version: version.to_string(),
            finished: false,
        }
    }

    pub fn finish(&mut self, bytes: u64, elapsed: Duration) -> Result<bool> {
        if self.finished {
            return Ok(false);
        }
        self.finished = true;
        if let Some(handle) = self.handle {
            let summary = format_finished_line(&self.app_name, &self.version, bytes, elapsed);
            if let Ok(mut ui) = progress_ui().lock() {
                return ui.finish(handle.id, summary);
            }
        }
        Ok(false)
    }
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        if !self.finished
            && let Some(handle) = self.handle
            && let Ok(mut ui) = progress_ui().lock()
        {
            let _ = ui.abandon(handle.id);
        }
    }
}

impl ProgressUi {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            next_id: 0,
            rendered_lines: 0,
            last_draw: Instant::now(),
            entries: Vec::new(),
        }
    }

    pub fn begin(&mut self, total: u64, name: &str) -> Option<ProgressHandle> {
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
        });
        Some(ProgressHandle { id })
    }

    pub fn inc(&mut self, id: usize, amount: u64) -> Result<()> {
        let mut needs_draw = false;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.downloaded = entry.downloaded.saturating_add(amount);
            let percent = (entry.downloaded.saturating_mul(100) / entry.total).min(100);
            if percent >= entry.last_percent.saturating_add(5)
                || self.last_draw.elapsed() >= Duration::from_millis(120)
            {
                entry.last_percent = percent;
                needs_draw = true;
            }
        }
        if needs_draw {
            self.draw()?;
        }
        Ok(())
    }

    pub fn finish(&mut self, id: usize, summary: String) -> Result<bool> {
        if let Some(index) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(index);
            self.clear_rendered()?;

            if self.enabled {
                let mut stderr = std::io::stderr();
                writeln!(stderr, "{}", green(&summary))?;
                stderr.flush()?;
            }

            self.draw()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn abandon(&mut self, id: usize) -> Result<bool> {
        if let Some(index) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(index);
            self.clear_rendered()?;
            self.draw()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn clear_rendered(&mut self) -> Result<()> {
        if self.rendered_lines == 0 || !self.enabled {
            return Ok(());
        }
        let mut stderr = std::io::stderr();
        for _ in 0..self.rendered_lines {
            write!(stderr, "\x1b[1A\x1b[2K\r")?;
        }
        stderr.flush()?;
        self.rendered_lines = 0;
        Ok(())
    }

    pub fn clear_all(&mut self) -> Result<()> {
        self.clear_rendered()?;
        self.entries.clear();
        Ok(())
    }

    pub fn draw(&mut self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        self.clear_rendered()?;

        if self.entries.is_empty() {
            return Ok(());
        }

        let mut stderr = std::io::stderr();
        for entry in &self.entries {
            writeln!(stderr, "{}", format_progress_line(entry))?;
        }

        stderr.flush()?;
        self.rendered_lines = self.entries.len();
        self.last_draw = Instant::now();
        Ok(())
    }
}

pub fn progress_ui() -> &'static Mutex<ProgressUi> {
    static UI: OnceLock<Mutex<ProgressUi>> = OnceLock::new();
    UI.get_or_init(|| Mutex::new(ProgressUi::new(false)))
}

pub fn interactive_progress_enabled(quiet: bool) -> bool {
    !quiet && std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub fn human_bytes(bytes: u64) -> String {
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

pub fn human_bytes_precise(bytes: f64) -> String {
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

pub fn human_rate(bytes_per_second: f64) -> String {
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

pub fn format_finished_line(name: &str, version: &str, bytes: u64, elapsed: Duration) -> String {
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

pub fn green(text: &str) -> String {
    format!("\x1b[32m{}\x1b[0m", text)
}
