use crate::output::types::*;
use crate::output::styling::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn print_list_human(apps: &[ListApp], colors: bool) {
    println!("{}", bold("Configured apps", colors));
    println!("{}", dim(&format!("{} app(s)", apps.len()), colors));

    for app in apps {
        let strategy = colorize(&app.strategy, color_for_strategy(&app.strategy), colors);
        let version = app
            .local_version
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "not installed".to_string());
        let integration = if app.integration {
            "enabled"
        } else {
            "disabled"
        };
        let symlink = if app.symlink { "enabled" } else { "disabled" };

        println!(
            "- {} {} | local: {} | integration: {} | symlink: {}",
            bold(&app.name, colors),
            bracketed(&strategy, colors),
            colorize(&version, color_for_version(&version), colors),
            colorize(
                integration,
                if app.integration {
                    Color::Green
                } else {
                    Color::Red
                },
                colors
            ),
            colorize(
                symlink,
                if app.symlink {
                    Color::Green
                } else {
                    Color::Red
                },
                colors
            ),
        );
    }
}

pub fn print_check_human(
    apps: &[CheckApp],
    error: Option<&str>,
    rate_limit_note: Option<&str>,
    colors: bool,
) {
    print_command_header("check", apps.len(), colors);
    let mut available = 0usize;
    let mut current = 0usize;
    let mut rate_limited = 0usize;
    let mut failed = 0usize;

    for app in apps {
        match app.status {
            CheckStatus::UpdateAvailable => {
                available += 1;
                let local = app.local_version.as_deref().unwrap_or("unknown");
                let remote = app.remote_version.as_deref().unwrap_or("unknown");
                println!(
                    "- {} {} {} -> {}",
                    bold(&app.name, colors),
                    bracketed(&status_text("update available", Color::Yellow), colors),
                    colorize(local, Color::Blue, colors),
                    colorize(remote, Color::Green, colors),
                );
                if let Some(url) = &app.download_url {
                    println!("  download: {}", dim(url, colors));
                }
            }
            CheckStatus::UpToDate => {
                current += 1;
                let local = app.local_version.as_deref().unwrap_or("unknown");
                println!(
                    "- {} {} {}",
                    bold(&app.name, colors),
                    bracketed(&status_text("up to date", Color::Green), colors),
                    colorize(local, Color::Blue, colors),
                );
            }
            CheckStatus::RateLimited => {
                rate_limited += 1;
                println!(
                    "- {} {} {}",
                    bold(&app.name, colors),
                    bracketed(&status_text("rate limited", Color::Yellow), colors),
                    format_rate_limit_retry_text(app.rate_limited_until),
                );
            }
            CheckStatus::Error => {
                failed += 1;
                println!(
                    "- {} {} {}",
                    bold(&app.name, colors),
                    bracketed(&status_text("error", Color::Red), colors),
                    app.error.as_deref().unwrap_or("unknown error"),
                );
            }
        }

        if !app.capabilities.is_empty() {
            println!(
                "  capabilities: {}",
                dim(&app.capabilities.join(", "), colors)
            );
        }
    }

    println!(
        "{}",
        dim(
            &format!(
                "summary: {} available, {} current, {} rate limited, {} failed",
                available, current, rate_limited, failed
            ),
            colors
        )
    );

    if let Some(error) = error {
        println!(
            "{}",
            colorize(&format!("note: {}", error), Color::Red, colors)
        );
    }

    if let Some(note) = rate_limit_note {
        println!("{}", colorize(&format!("note: {}", note), Color::Yellow, colors));
    }
}

pub fn print_validate_human(apps: &[ValidateApp], error: Option<&str>, colors: bool) {
    print_command_header("validate", apps.len(), colors);
    let mut valid = 0usize;
    let mut invalid = 0usize;

    for app in apps {
        let label = app.name.as_deref().unwrap_or("<unknown>");
        match app.status {
            ValidateStatus::Valid => {
                valid += 1;
                println!(
                    "- {} {} {}",
                    bold(label, colors),
                    bracketed(&status_text("valid", Color::Green), colors),
                    dim(&app.file, colors)
                );
            }
            ValidateStatus::Invalid => {
                invalid += 1;
                println!(
                    "- {} {} {}",
                    bold(label, colors),
                    bracketed(&status_text("invalid", Color::Red), colors),
                    dim(&app.file, colors)
                );
                if let Some(msg) = &app.error {
                    println!("  {}", msg);
                }
            }
        }
    }

    println!(
        "{}",
        dim(
            &format!("summary: {} valid, {} invalid", valid, invalid),
            colors
        )
    );

    if let Some(error) = error {
        println!(
            "{}",
            colorize(&format!("note: {}", error), Color::Red, colors)
        );
    }
}

pub fn print_doctor_human(checks: &[DoctorCheck], colors: bool) {
    print_command_header("doctor", checks.len(), colors);
    let mut ok = 0usize;
    let mut warn = 0usize;

    for check in checks {
        match check.status {
            DoctorStatus::Ok => {
                ok += 1;
                println!(
                    "- {} {} {}",
                    bold(&check.name, colors),
                    bracketed(&status_text("ok", Color::Green), colors),
                    check.detail
                );
            }
            DoctorStatus::Warn => {
                warn += 1;
                println!(
                    "- {} {} {}",
                    bold(&check.name, colors),
                    bracketed(&status_text("warn", Color::Yellow), colors),
                    check.detail
                );
            }
        }
    }

    println!(
        "{}",
        dim(&format!("summary: {} ok, {} warn", ok, warn), colors)
    );
}

pub fn format_rate_limit_retry_text(until: Option<u64>) -> String {
    match until {
        Some(until) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let secs = until.saturating_sub(now);
            if secs < 60 {
                format!("retry in {}s", secs)
            } else if secs < 3600 {
                format!("retry in {}m", secs / 60)
            } else {
                format!("retry in {}h {}m", secs / 3600, (secs % 3600) / 60)
            }
        }
        None => "retry later".to_string(),
    }
}

pub fn print_self_update_start(kind: &str, current: &str, colors: bool) {
    println!(
        "{}",
        bold(
            &format!("Checking for {} updates (current: v{})", kind, current),
            colors
        )
    );
}

pub fn print_self_update_current(current: &str, colors: bool) {
    println!(
        "{}",
        colorize(
            &format!("Already up to date (v{})", current),
            Color::Green,
            colors
        )
    );
}

pub fn print_self_update_available(current: &str, latest: &str, colors: bool) {
    println!(
        "{}",
        colorize(
            &format!("New version available: {} -> {}", current, latest),
            Color::Yellow,
            colors
        )
    );
}

pub fn print_self_update_download(url: &str, colors: bool) {
    println!("{}", dim(&format!("Downloading: {}", url), colors));
}

pub fn print_self_update_success(tag: &str, colors: bool) {
    println!(
        "{}",
        colorize(
            &format!("Updated successfully to {}!", tag),
            Color::Green,
            colors
        )
    );
}

fn print_command_header(command: &str, count: usize, colors: bool) {
    println!(
        "{}",
        bold(
            &format!(
                "{} results ({} app{})",
                command,
                count,
                if count == 1 { "" } else { "s" }
            ),
            colors
        )
    );
}

fn color_for_strategy(strategy: &str) -> Color {
    match strategy.to_ascii_lowercase().as_str() {
        "forge" => Color::Cyan,
        "direct" => Color::Blue,
        "script" => Color::Magenta,
        _ => Color::White,
    }
}

fn color_for_version(version: &str) -> Color {
    if version == "not installed" {
        Color::Yellow
    } else {
        Color::Blue
    }
}
