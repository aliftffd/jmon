use super::{
    chart, fan,
    gauge::GradientBar,
    theme, throttle,
    widgets::{columns, line, oc_text, panel, region, row, temp_value},
};
use crate::{
    app::App,
    model::{CheckState, Temp},
};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    Frame,
};

fn warning(temp: &Temp) -> (&'static str, Color) {
    if temp.celsius >= 95.0 {
        ("[BAD]", theme::CRITICAL)
    } else if temp.celsius >= 85.0 {
        ("[WARN]", theme::WARN)
    } else {
        ("", theme::TEXT)
    }
}

pub fn zone(f: &mut Frame, area: Rect, temp: &Temp) {
    let (badge, color) = warning(temp);
    let trips = format!("{}/{}", temp_value(temp.trip_sw), temp_value(temp.trip_hw));
    line(
        f,
        region(area, 0, 0, 6, 1),
        Line::styled(temp.name.as_str(), theme::muted()),
    );
    line(
        f,
        region(area, 6, 0, 8, 1),
        Line::styled(
            format!("{:>5.1}°C", temp.celsius),
            Style::default().fg(color),
        ),
    );
    let suffix_w = if area.width >= 44 { 21 } else { 14 };
    let bar_w = area.width.saturating_sub(15 + suffix_w);
    f.render_widget(
        GradientBar {
            ratio: temp.celsius / 110.0,
        },
        region(area, 14, 0, bar_w, 1),
    );
    line(
        f,
        region(area, 15 + bar_w, 0, 14, 1),
        Line::styled(trips, theme::muted()),
    );
    if area.width >= 44 {
        line(
            f,
            region(area, 29 + bar_w, 0, 7, 1),
            Line::styled(badge, theme::strong(color)),
        );
    } else if !badge.is_empty() {
        // Keep an explicit alarm even when the narrow row cannot fit a badge column.
        line(
            f,
            region(area, 0, 0, 6, 1),
            Line::styled(badge, theme::strong(color)),
        );
    }
}

fn zones(f: &mut Frame, area: Rect, app: &App) -> u16 {
    line(
        f,
        row(area, 0),
        Line::styled("ZONE  NOW °C   0–110°C     SW/HW °C", theme::muted()),
    );
    if app.snap.temps.is_empty() {
        line(
            f,
            row(area, 1),
            Line::styled("Temperature sensors n/a", theme::muted()),
        );
        return 2.min(area.height);
    }
    let count = app
        .snap
        .temps
        .len()
        .min(area.height.saturating_sub(1) as usize);
    for (i, temp) in app.snap.temps.iter().take(count).enumerate() {
        zone(f, row(area, i as u16 + 1), temp);
    }
    count as u16 + 1
}

pub fn status(app: &App) -> Line<'static> {
    let check = app.checks.iter().find(|c| c.name == "Thermal");
    let state = check.map_or(CheckState::Unknown, |c| c.state);
    let observed = check.map_or("thermal check n/a", |c| c.observed.as_str());
    Line::styled(
        format!(
            "{} {observed} | {}",
            theme::check_label(state),
            oc_text(app)
        ),
        Style::default().fg(theme::check_color(state)),
    )
}

pub fn overview(f: &mut Frame, area: Rect, app: &App) {
    let inner = panel(f, area, "THERMAL / COOLING", theme::BORDER);
    let used = zones(
        f,
        region(inner, 0, 0, inner.width, inner.height.saturating_sub(5)),
        app,
    );
    line(f, row(inner, used), status(app));
    line(
        f,
        row(inner, used + 1),
        Line::styled("── FAN / AUTOMATIC ──", theme::muted()),
    );
    fan::draw(
        f,
        region(
            inner,
            0,
            used + 2,
            inner.width,
            inner.height.saturating_sub(used + 2),
        ),
        app,
    );
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let throttle_h = throttle::height(app).min(area.height);
    throttle::draw(f, region(area, 0, 0, area.width, throttle_h), app);
    let area = region(
        area,
        0,
        throttle_h,
        area.width,
        area.height.saturating_sub(throttle_h),
    );
    if area.width >= 110 && area.height >= 25 {
        let (left, right) = columns(area, 60, 2);
        let zone_h = ((app.snap.temps.len() as u16).saturating_add(6)).min(left.height);
        let inner = panel(
            f,
            region(left, 0, 0, left.width, zone_h),
            "THERMAL ZONES / TRIPS",
            theme::BORDER,
        );
        let used = zones(f, inner, app);
        line(f, row(inner, used + 1), status(app));
        line(
            f,
            row(inner, used + 2),
            Line::styled("Display alerts: [WARN] ≥85°C  [BAD] ≥95°C", theme::muted()),
        );
        tj_chart(
            f,
            region(
                left,
                0,
                zone_h + 1,
                left.width,
                left.height.saturating_sub(zone_h + 1),
            ),
            app,
        );
        let fan_inner = panel(f, right, "FAN / AUTOMATIC", theme::BORDER);
        fan::draw(f, fan_inner, app);
    } else {
        let zone_h =
            ((app.snap.temps.len() as u16).saturating_add(4)).min(area.height.saturating_sub(5));
        let inner = panel(
            f,
            region(area, 0, 0, area.width, zone_h),
            "THERMAL ZONES / TRIPS",
            theme::BORDER,
        );
        let used = zones(
            f,
            region(inner, 0, 0, inner.width, inner.height.saturating_sub(1)),
            app,
        );
        line(f, row(inner, used), status(app));
        let lower = region(
            area,
            0,
            zone_h,
            area.width,
            area.height.saturating_sub(zone_h),
        );
        let (left, right) = columns(lower, 52, 1);
        let fan_inner = panel(f, left, "FAN / AUTOMATIC", theme::BORDER);
        fan::draw(f, fan_inner, app);
        tj_chart(f, right, app);
    }
}

fn tj_chart(f: &mut Frame, area: Rect, app: &App) {
    let inner = panel(f, area, "TJ / TEMPERATURE", theme::BORDER);
    let available = app.snap.temps.iter().any(|t| t.name == "tj");
    let ceiling = (app.hist.tj_c.max() / 10.0).ceil().max(11.0) * 10.0;
    chart::history(
        f,
        inner,
        &app.hist.tj_c,
        "tj / °C",
        ceiling,
        theme::ACCENT,
        available,
    );
}
