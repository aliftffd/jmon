use super::{
    chart,
    gauge::GradientBar,
    theme,
    widgets::{line, region, row, temp_value},
};
use crate::app::App;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Widget,
    Frame,
};

pub const ROLE_MASK: [&str; 7] = [
    "BBBSSSSSSSSSBBB",
    "BBSSBBBBBBBSSBB",
    "BSBBBBBBBBBBBSB",
    "BSBBBBBHBBBBBSB",
    "BSBBBBBBBBBBBSB",
    "BBSSBBBBBBBSSBB",
    "BBBSSSSSSSSSBBB",
];

pub struct FanArt<'a> {
    pub app: &'a App,
}

impl Widget for FanArt<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let fan = self.app.snap.fan.as_ref();
        let stalled = fan.is_some_and(|v| v.stalled());
        let running = fan.and_then(|v| v.rpm).is_some_and(|rpm| rpm > 0);
        let index = if stalled || !running || self.app.bench {
            0
        } else {
            self.app.fan_frame % FRAMES.len()
        };
        let blade = if stalled {
            theme::CRITICAL
        } else if running {
            theme::ACCENT
        } else {
            theme::MUTED
        };
        let hub = if stalled {
            theme::CRITICAL
        } else if running {
            theme::ACCENT_HI
        } else {
            theme::TEXT
        };
        let clipped = area.intersection(buf.area);
        for (y, (art, roles)) in FRAMES[index]
            .iter()
            .zip(ROLE_MASK)
            .enumerate()
            .take(usize::from(area.height))
        {
            for (x, (glyph, role)) in art
                .chars()
                .zip(roles.chars())
                .enumerate()
                .take(usize::from(area.width))
            {
                let point = (
                    area.x.saturating_add(x as u16),
                    area.y.saturating_add(y as u16),
                );
                if !clipped.contains(point.into()) {
                    continue;
                }
                let color = match role {
                    'S' => theme::BORDER,
                    'H' => hub,
                    _ => blade,
                };
                buf[point]
                    .set_char(glyph)
                    .set_style(Style::default().fg(color).bg(theme::PANEL));
            }
        }
    }
}

fn status(app: &App) -> (String, Color) {
    match app.snap.fan.as_ref() {
        Some(fan) if fan.stalled() => ("[BAD] FAN STALLED  0 RPM".into(), theme::CRITICAL),
        Some(fan) => match fan.rpm {
            Some(0) => ("STOPPED / AUTO  0 RPM".into(), theme::MUTED),
            Some(rpm) => (format!("[OK] {rpm} RPM"), theme::ACCENT),
            None => ("[WARN] TACH n/a  rpm n/a".into(), theme::WARN),
        },
        None => ("[WARN] FAN n/a  rpm n/a".into(), theme::WARN),
    }
}

pub fn summary(app: &App) -> Line<'static> {
    let (status, color) = status(app);
    let duty = app.snap.fan.as_ref().map_or_else(
        || "PWM n/a".into(),
        |v| format!("PWM {:.0}% ({}/255)", v.pwm_pct, v.pwm),
    );
    let tj = temp_value(
        app.snap
            .temps
            .iter()
            .find(|t| t.name == "tj")
            .map(|t| t.celsius),
    );
    Line::from(vec![
        Span::styled(status, theme::strong(color)),
        Span::styled(format!("  {duty}  AUTO  tj {tj}°C"), theme::muted()),
    ])
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    // Art beside the text when wide; stacked on top when narrow but tall (e.g. ~40-col side column);
    // text only when neither fits. (lead fix: art used to vanish below 52 cols.)
    let content = if area.width >= 52 && area.height >= 7 {
        f.render_widget(FanArt { app }, region(area, 0, 0, 15, 7));
        region(area, 17, 0, area.width.saturating_sub(17), area.height)
    } else if area.width >= 15 && area.height >= 7 + 4 {
        f.render_widget(
            FanArt { app },
            region(area, (area.width - 15) / 2, 0, 15, 7),
        );
        region(area, 0, 7, area.width, area.height - 7)
    } else {
        area
    };
    let (status, color) = status(app);
    let tj = temp_value(
        app.snap
            .temps
            .iter()
            .find(|t| t.name == "tj")
            .map(|t| t.celsius),
    );
    line(
        f,
        row(content, 0),
        Line::from(vec![
            Span::styled(status, theme::strong(color)),
            Span::styled(format!("  tj {tj}°C"), theme::muted()),
        ]),
    );
    if let Some(fan) = &app.snap.fan {
        line(
            f,
            region(content, 0, 1, 9, 1),
            format!("PWM {:>3.0}%", fan.pwm_pct),
        );
        let bar_w = content.width.saturating_sub(18);
        f.render_widget(
            GradientBar {
                ratio: fan.pwm_pct / 100.0,
            },
            region(content, 9, 1, bar_w, 1),
        );
        line(
            f,
            region(content, 10 + bar_w, 1, 8, 1),
            Line::styled(format!("{}/255", fan.pwm), theme::muted()),
        );
    } else {
        line(f, row(content, 1), Line::styled("PWM n/a", theme::muted()));
    }
    let stalled = app.snap.fan.as_ref().is_some_and(|v| v.stalled());
    let note = if app.bench {
        "anim paused (bench)"
    } else if stalled {
        "PWM > 0 but RPM = 0; check cooling"
    } else {
        "Control: auto (kernel/nvfancontrol)"
    };
    line(
        f,
        row(content, 2),
        Line::styled(
            note,
            Style::default().fg(if stalled && !app.bench {
                theme::CRITICAL
            } else {
                theme::MUTED
            }),
        ),
    );
    let ceiling = (app.hist.fan_rpm.max() / 1000.0).ceil().max(2.0) * 1000.0;
    chart::history(
        f,
        region(
            content,
            0,
            3,
            content.width,
            content.height.saturating_sub(3),
        ),
        &app.hist.fan_rpm,
        "RPM",
        ceiling,
        if stalled {
            theme::CRITICAL
        } else {
            theme::ACCENT
        },
        app.snap.fan.as_ref().and_then(|v| v.rpm).is_some(),
    );
}

// Verbatim 15×7 art from fan-frames.txt, including trailing spaces.
pub const FRAMES: [[&str; 7]; 10] = [
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉   ⣠⣤⣤⣄⠉⢦  ",
        " ⡜ ⣤⣄  ⣿⡿⠿⠟⠃ ⢣ ",
        " ⡇⠘⣿⣿⣷⣤●⣄⣀   ⢸ ",
        " ⢣ ⠈⠙⠋⠁ ⣿⣿⡇  ⡜ ",
        "  ⠳⣀   ⠸⡿⠿⠁⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉    ⣀⣤⣄⠉⢦  ",
        " ⡜ ⣾⣷⡀ ⢸⣿⣿⣿⡷ ⢣ ",
        " ⡇ ⢿⣿⣿⡤●⣀    ⢸ ",
        " ⢣  ⠈⠁ ⣠⣿⣿   ⡜ ",
        "  ⠳⣀  ⠰⣿⡿⠟ ⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉⣀    ⣀⡀⠉⢦  ",
        " ⡜ ⣾⣿⣇ ⢠⣿⣿⣿⣷ ⢣ ",
        " ⡇ ⠙⢿⡿⠦●⡁⠈⠉⠉ ⢸ ",
        " ⢣    ⢀⣼⣿⡆   ⡜ ",
        "  ⠳⣀  ⢿⣿⠿⠃ ⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉⣠⣤     ⠉⢦  ",
        " ⡜ ⢸⣿⣿⡀ ⣴⣿⣿⣦ ⢣ ",
        " ⡇ ⠈⠻⠿⠓●⠉⠙⠛⠿⠃⢸ ",
        " ⢣   ⢀⣠⣾⣷    ⡜ ",
        "  ⠳⣀ ⠿⢿⠿⠋  ⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉⣠⣶⡆    ⠉⢦  ",
        " ⡜  ⣿⣿⡇ ⣠⣶⣶⣄ ⢣ ",
        " ⡇  ⠈⠛⠓●⠉⠛⢿⣿⡇⢸ ",
        " ⢣  ⢀⣤⣴⣿⡆  ⠉ ⡜ ",
        "  ⠳⣀⠙⠿⠿⠟⠁  ⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉⢀⣶⣾⡆   ⠉⢦  ",
        " ⡜  ⢸⣿⣿ ⢀⣠⣄⡀ ⢣ ",
        " ⡇   ⠉⠙●⠛⢿⣿⣿⡄⢸ ",
        " ⢣ ⢠⣴⣶⣾⣿  ⠙⠛ ⡜ ",
        "  ⠳⣀⠙⠛⠛⠋   ⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉ ⣴⣾⣿⠆  ⠉⢦  ",
        " ⡜   ⣿⣿⠋ ⢀⡀  ⢣ ",
        " ⡇    ⠉●⠚⣿⣿⣷ ⢸ ",
        " ⢣ ⢾⣿⣿⣿⡇ ⠈⢿⡿ ⡜ ",
        "  ⠳⣀⠙⠛⠉    ⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉ ⢠⣶⣿⣷  ⠉⢦  ",
        " ⡜   ⠸⣿⡟⠁    ⢣ ",
        " ⡇ ⣀⣀⡀⢈●⠲⣾⣷⣄ ⢸ ",
        " ⢣ ⢿⣿⣿⣿⠃ ⢹⣿⡿ ⡜ ",
        "  ⠳⣀⠈⠉    ⠉⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉  ⣠⣶⣷⣶ ⠉⢦  ",
        " ⡜    ⢿⡿⠋⠁   ⢣ ",
        " ⡇⢠⣶⣤⣄⣀●⢤⣶⣦⡀ ⢸ ",
        " ⢣ ⠻⣿⣿⠟ ⠈⣿⣿⡇ ⡜ ",
        "  ⠳⣀     ⠛⠋⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
    [
        "   ⢀⡠⠔⠒⠒⠒⠢⢄⡀   ",
        "  ⡴⠉  ⢀⣴⣶⣶⣄⠉⢦  ",
        " ⡜ ⣀  ⠸⣿⠟⠛⠁  ⢣ ",
        " ⡇⢸⣿⣷⣤⣀●⢤⣤⡀  ⢸ ",
        " ⢣ ⠙⠿⠿⠋ ⢸⣿⣿  ⡜ ",
        "  ⠳⣀    ⠸⠿⠋⣀⠞  ",
        "   ⠈⠑⠢⠤⠤⠤⠔⠊⠁   ",
    ],
];
