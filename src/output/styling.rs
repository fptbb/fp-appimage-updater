#[derive(Clone, Copy)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Cyan,
    Magenta,
    White,
}

pub fn bold(text: &str, colors: bool) -> String {
    style(text, "1", colors)
}

pub fn dim(text: &str, colors: bool) -> String {
    style(text, "2", colors)
}

pub fn colorize(text: &str, color: Color, colors: bool) -> String {
    let code = match color {
        Color::Red => "31",
        Color::Green => "32",
        Color::Yellow => "33",
        Color::Blue => "34",
        Color::Cyan => "36",
        Color::Magenta => "35",
        Color::White => "37",
    };
    style(text, code, colors)
}

pub fn style(text: &str, code: &str, colors: bool) -> String {
    if colors {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}

pub fn status_text(text: &str, color: Color) -> String {
    colorize(text, color, true)
}

pub fn bracketed(text: &str, colors: bool) -> String {
    if colors {
        format!("[{}]", text)
    } else {
        format!("[{}]", strip_ansi(text))
    }
}

pub fn strip_ansi(text: &str) -> String {
    text.replace("\x1b[0m", "")
        .replace("\x1b[31m", "")
        .replace("\x1b[32m", "")
        .replace("\x1b[33m", "")
        .replace("\x1b[34m", "")
        .replace("\x1b[35m", "")
        .replace("\x1b[36m", "")
        .replace("\x1b[37m", "")
        .replace("\x1b[1m", "")
        .replace("\x1b[2m", "")
}
