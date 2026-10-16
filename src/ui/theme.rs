use crate::model::CheckState;
use ratatui::style::{Color, Modifier, Style};

// Only palette.md colors; interpolated bar fills are the requested exception.
pub const BG: Color = Color::Rgb(11, 15, 13);
pub const PANEL: Color = Color::Rgb(22, 30, 26);
pub const BORDER: Color = Color::Rgb(42, 68, 16);
pub const TEXT: Color = Color::Rgb(248, 250, 249);
pub const MUTED: Color = Color::Rgb(148, 163, 155);
pub const ACCENT: Color = Color::Rgb(118, 185, 0);
pub const ACCENT_HI: Color = Color::Rgb(155, 214, 58);
pub const WARN: Color = Color::Rgb(245, 158, 11);
pub const CRITICAL: Color = Color::Rgb(239, 68, 68);

pub const GRADIENT_STOPS: [(f64, (u8, u8, u8)); 3] = [
    (0.0, (118, 185, 0)),
    (0.6, (245, 158, 11)),
    (1.0, (239, 68, 68)),
];

/// Position in the entire track, independent of how much of it is filled.
pub fn gradient_color(position: f64) -> Color {
    let p = if position.is_nan() {
        0.0
    } else {
        position.clamp(0.0, 1.0)
    };
    let (a, b) = if p <= GRADIENT_STOPS[1].0 {
        (GRADIENT_STOPS[0], GRADIENT_STOPS[1])
    } else {
        (GRADIENT_STOPS[1], GRADIENT_STOPS[2])
    };
    let t = (p - a.0) / (b.0 - a.0);
    let mix = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Color::Rgb(
        mix(a.1 .0, b.1 .0),
        mix(a.1 .1, b.1 .1),
        mix(a.1 .2, b.1 .2),
    )
}

pub fn base() -> Style {
    Style::default().fg(TEXT).bg(BG)
}
pub fn panel() -> Style {
    Style::default().fg(TEXT).bg(PANEL)
}
pub fn muted() -> Style {
    Style::default().fg(MUTED)
}
pub fn border() -> Style {
    Style::default().fg(BORDER)
}
pub fn strong(color: Color) -> Style {
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}
pub fn check_color(state: CheckState) -> Color {
    match state {
        CheckState::Ok => ACCENT,
        CheckState::Bad => CRITICAL,
        CheckState::Unknown => WARN,
    }
}
pub fn check_label(state: CheckState) -> &'static str {
    match state {
        CheckState::Ok => "[OK]",
        CheckState::Bad => "[BAD]",
        CheckState::Unknown => "[?]",
    }
}
