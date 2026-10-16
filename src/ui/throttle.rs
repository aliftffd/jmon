use super::{
    theme,
    widgets::{line, rate, row},
};
use crate::{
    app::App,
    model::{ClockState, DropStats},
};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    Frame,
};

fn drops(name: &str, stats: &DropStats) -> Span<'static> {
    Span::styled(
        format!(
            "{name} {} / {:.1} s / longest {:.1} s",
            stats.drops, stats.below_s, stats.longest_s
        ),
        if stats.active {
            theme::strong(theme::WARN)
        } else {
            theme::muted()
        },
    )
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    line(
        f,
        row(area, 0),
        Line::from(vec![
            Span::styled("CLOCK DROPS (locked)  ", theme::strong(theme::MUTED)),
            Span::styled(
                format!("OC +{}", app.throttle.oc_rise),
                if app.throttle.oc_rise > 0 {
                    theme::strong(theme::WARN)
                } else {
                    theme::muted()
                },
            ),
            Span::styled(format!(" · sampled {} Hz", rate(app)), theme::muted()),
        ]),
    );
    line(
        f,
        row(area, 1),
        Line::from(vec![
            drops("CPU", &app.throttle.cpu),
            Span::raw(" · "),
            drops("GPU", &app.throttle.gpu),
        ]),
    );
    if app.snap.clocks == ClockState::Dynamic {
        line(
            f,
            row(area, 2),
            Line::styled("counting only while clocks are locked", theme::muted()),
        );
    }
}

pub fn height(app: &App) -> u16 {
    if app.snap.clocks == ClockState::Dynamic {
        3
    } else {
        2
    }
}
