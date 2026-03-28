use anyhow::Result;
use serde::Serialize;
use std::io::IsTerminal;

pub mod human;
pub mod styling;
pub mod types;

pub use human::*;
pub use styling::*;
pub use types::*;

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    serde_json::to_writer_pretty(std::io::stdout(), value)?;
    println!();
    Ok(())
}

pub fn colors_enabled(json_output: bool) -> bool {
    !json_output && std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub fn print_progress(message: &str, colors: bool) {
    crate::downloader::suspend_for_print(|| {
        println!("{}", dim(message, colors));
    });
}

pub fn print_success(message: &str, colors: bool) {
    crate::downloader::suspend_for_print(|| {
        println!("{}", colorize(message, Color::Green, colors));
    });
}

pub fn print_warning(message: &str, colors: bool) {
    crate::downloader::suspend_for_print(|| {
        eprintln!("{}", colorize(message, Color::Yellow, colors));
    });
}
