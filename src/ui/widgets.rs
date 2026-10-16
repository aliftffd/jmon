use super::theme;
use crate::app::App;
use ratatui::{
    layout::Rect,
    style::Color,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Every child rectangle is clipped to its parent, including zero-sized terminals.
pub fn region(area: Rect, x: u16, y: u16, width: u16, height: u16) -> Rect {
    let x = x.min(area.width);
    let y = y.min(area.height);
    Rect::new(
        area.x.saturating_add(x),
        area.y.saturating_add(y),
        width.min(area.width - x),
        height.min(area.height - y),
    )
}
pub fn row(area: Rect, y: u16) -> Rect {
    region(area, 0, y, area.width, 1)
}
pub fn line<'a>(f: &mut Frame, area: Rect, text: impl Into<Line<'a>>) {
    if !area.is_empty() {
        f.render_widget(Paragraph::new(text.into()), area);
    }
}

pub fn elide(text: &str, width: u16) -> String {
    let text = text.replace(['\n', '\r', '\t'], " ");
    if Line::from(text.as_str()).width() <= width as usize {
        return text;
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut result = String::new();
    for ch in text.chars() {
        let cell_width = Span::raw(ch.to_string()).width();
        if used + cell_width > usize::from(width - 1) {
            break;
        }
        result.push(ch);
        used += cell_width;
    }
    result.push('…');
    result
}
pub fn panel(f: &mut Frame, area: Rect, title: impl Into<String>, color: Color) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .style(theme::panel())
        .border_style(ratatui::style::Style::default().fg(color))
        .title(Line::styled(
            format!(" {} ", title.into()),
            theme::strong(if color == theme::BORDER {
                theme::MUTED
            } else {
                color
            }),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let pad = u16::from(inner.width > 2);
    region(
        inner,
        pad,
        0,
        inner.width.saturating_sub(pad * 2),
        inner.height,
    )
}
pub fn columns(area: Rect, left_percent: u16, gap: u16) -> (Rect, Rect) {
    let gap = gap.min(area.width);
    let left = ((u32::from(area.width - gap) * u32::from(left_percent.min(100))) / 100) as u16;
    (
        region(area, 0, 0, left, area.height),
        region(area, left + gap, 0, area.width - left - gap, area.height),
    )
}
pub fn rate(app: &App) -> String {
    if app.sample_ms == 0 {
        return "n/a".into();
    }
    let rate = 1000.0 / app.sample_ms as f64;
    format!("{rate:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
pub fn oc_text(app: &App) -> String {
    if app.snap.oc_events.is_empty() {
        return "OC events n/a".into();
    }
    format!(
        "OC events {}",
        app.snap
            .oc_events
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(" / ")
    )
}
pub fn temp_value(value: Option<f64>) -> String {
    value
        .filter(|v| v.is_finite())
        .map_or_else(|| "n/a".into(), |v| format!("{v:.1}"))
}
