use super::theme;
use ratatui::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

/// A one-row gauge whose colors stay anchored to positions in the full track.
#[derive(Clone, Copy, Debug)]
pub struct GradientBar {
    pub ratio: f64,
}

impl Widget for GradientBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let clipped = area.intersection(buf.area);
        if clipped.is_empty() || clipped.y != area.y {
            return;
        }
        let ratio = if self.ratio.is_nan() {
            0.0
        } else {
            self.ratio.clamp(0.0, 1.0)
        };
        let filled = ratio * f64::from(area.width);
        const EIGHTHS: [&str; 9] = ["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
        for x in clipped.x..clipped.right() {
            let index = x - area.x;
            let eighths = ((filled - f64::from(index)).clamp(0.0, 1.0) * 8.0).floor() as usize;
            let color = if eighths == 0 {
                theme::BORDER
            } else {
                theme::gradient_color(
                    f64::from(index) / f64::from(area.width.saturating_sub(1).max(1)),
                )
            };
            buf[(x, area.y)]
                .set_symbol(EIGHTHS[eighths])
                .set_style(Style::default().fg(color).bg(theme::PANEL));
        }
    }
}
