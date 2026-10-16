use super::{
    fan, power, theme, throttle,
    widgets::{columns, line, oc_text, panel, rate, region, row},
};
use crate::{
    app::App,
    model::{CheckState, Level},
};
use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

pub fn draw(f: &mut Frame, area: Rect, app: &App, wide: bool) {
    let throttle_h = throttle::height(app).min(area.height);
    throttle::draw(f, region(area, 0, 0, area.width, throttle_h), app);
    let area = region(
        area,
        0,
        throttle_h,
        area.width,
        area.height.saturating_sub(throttle_h),
    );
    if wide {
        let banner_h = if area.height >= 35 { 7 } else { 6 };
        let middle_h = if area.height >= 35 { 15 } else { 12 };
        banner(f, region(area, 0, 0, area.width, banner_h), app);
        let (left, right) = columns(region(area, 0, banner_h + 1, area.width, middle_h), 62, 2);
        checklist(f, left, app, true);
        profile(f, right, app);
        let lower_y = banner_h + middle_h + 2;
        let (left, right) = columns(
            region(
                area,
                0,
                lower_y,
                area.width,
                area.height.saturating_sub(lower_y),
            ),
            62,
            2,
        );
        power::live(f, left, app);
        events(f, right, app);
    } else {
        banner(f, region(area, 0, 0, area.width, 3), app);
        let checklist_h = (app.checks.len() as u16)
            .saturating_add(3)
            .min(area.height.saturating_sub(6));
        checklist(f, region(area, 0, 3, area.width, checklist_h), app, false);
        let y = 3 + checklist_h;
        line(f, row(area, y), fan::summary(app));
        line(f, row(area, y + 1), power::summary(app));
        events(
            f,
            region(
                area,
                0,
                y + 2,
                area.width,
                area.height.saturating_sub(y + 2),
            ),
            app,
        );
    }
}

fn banner(f: &mut Frame, area: Rect, app: &App) {
    let state = app.conditions();
    let color = theme::check_color(state);
    let label = match state {
        CheckState::Ok => "OK",
        CheckState::Bad => "NOT OK",
        CheckState::Unknown => "UNKNOWN",
    };
    let inner = panel(f, area, "BENCHMARK CONDITIONS", color);
    let failed = app
        .checks
        .iter()
        .filter(|c| c.state == CheckState::Bad)
        .count();
    let passed = app
        .checks
        .iter()
        .filter(|c| c.state == CheckState::Ok)
        .count();
    let unknown = app
        .checks
        .iter()
        .filter(|c| c.state == CheckState::Unknown)
        .count();
    line(
        f,
        row(inner, 0),
        Line::styled(
            format!(
                "CONDITIONS {label}  |  {passed}/{} OK  |  {failed} FAIL  |  {unknown} UNKNOWN",
                app.checks.len()
            ),
            theme::strong(color),
        ),
    );
    if inner.height <= 1 {
        return;
    }
    let (left, right) = columns(
        region(inner, 0, 1, inner.width, inner.height.saturating_sub(2)),
        50,
        3,
    );
    let problems = app
        .checks
        .iter()
        .filter(|c| c.state != CheckState::Ok)
        .collect::<Vec<_>>();
    for (i, check) in problems.iter().take(4).enumerate() {
        let target = if i % 2 == 0 { left } else { right };
        line(
            f,
            row(target, i as u16 / 2),
            Line::styled(
                format!("{}: {}", check.name, check.observed),
                Style::default().fg(theme::WARN),
            ),
        );
    }
    if problems.is_empty() {
        line(
            f,
            row(left, 0),
            Line::styled(
                if app.checks.is_empty() {
                    "Awaiting condition checks"
                } else {
                    "All observed conditions match the target."
                },
                theme::muted(),
            ),
        );
    }
    if inner.height >= 5 {
        let resolved = app.events.iter().filter(|e| e.level == Level::Ok).count();
        line(f, row(inner, inner.height - 2), Line::styled(format!("DRIFT {failed} active / {resolved} cleared events  |  Observing live device conditions"), theme::muted()));
    }
    line(
        f,
        row(inner, inner.height.saturating_sub(1)),
        Line::styled(
            format!(
                "TARGET {} / mode {}  |  CPU {} MHz locked  |  GPU {} MHz locked",
                app.expected.power_mode_name,
                app.expected.power_mode_id,
                app.expected.cpu_mhz,
                app.expected.gpu_mhz,
            ),
            theme::muted(),
        ),
    );
}

fn checklist(f: &mut Frame, area: Rect, app: &App, detail: bool) {
    let passed = app
        .checks
        .iter()
        .filter(|c| c.state == CheckState::Ok)
        .count();
    let inner = panel(
        f,
        area,
        format!("CONDITION CHECKLIST / {passed}/{} OK", app.checks.len()),
        theme::BORDER,
    );
    let extra = if detail && inner.height >= 10 { 3 } else { 0 };
    let name_w = if inner.width >= 85 { 20 } else { 13 };
    let expected_w = if inner.width >= 85 { 25 } else { 18 };
    let rows = app.checks.iter().map(|c| {
        Row::new(vec![
            Cell::from(c.name),
            Cell::from(c.expected.as_str()).style(theme::muted()),
            Cell::from(c.observed.as_str()),
            Cell::from(theme::check_label(c.state))
                .style(theme::strong(theme::check_color(c.state))),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(name_w),
            Constraint::Length(expected_w),
            Constraint::Min(10),
            Constraint::Length(6),
        ],
    )
    .column_spacing(1)
    .header(Row::new(["CHECK", "EXPECTED", "OBSERVED", "STATE"]).style(theme::muted()));
    f.render_widget(
        table,
        region(inner, 0, 0, inner.width, inner.height.saturating_sub(extra)),
    );
    if app.checks.is_empty() {
        line(
            f,
            row(inner, 1),
            Line::styled("No condition checks yet", theme::muted()),
        );
    }
    if extra > 0 {
        let y = inner.height - extra;
        line(
            f,
            row(inner, y),
            Line::styled("Per-core actual / MHz", theme::muted()),
        );
        let cores = app
            .snap
            .cpus
            .iter()
            .map(|c| {
                if c.online {
                    format!("{}:{}", c.id, c.cur_mhz)
                } else {
                    format!("{}:off", c.id)
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        line(f, row(inner, y + 1), cores);
        line(
            f,
            row(inner, y + 2),
            Line::styled(oc_text(app), theme::muted()),
        );
    }
}

fn profile(f: &mut Frame, area: Rect, app: &App) {
    let inner = panel(f, area, "MONITOR PROFILE", theme::BORDER);
    let data = [
        ("REFRESH", format!("{} Hz", rate(app))),
        (
            "PROCESS SCAN",
            if app.bench {
                "DISABLED"
            } else {
                "normal cadence"
            }
            .to_string(),
        ),
        (
            "PRESET",
            if app.bench { "BENCH" } else { "NORMAL" }.to_string(),
        ),
        ("CLOCK CHECK", "every sample".to_string()),
        ("DRIFT CHECK", "every sample".to_string()),
        ("DATA AGE", format!("{} ms", app.data_age_ms)),
        (
            "DISPLAY",
            if app.frozen { "FROZEN" } else { "LIVE" }.to_string(),
        ),
    ];
    for (i, (name, value)) in data.iter().enumerate() {
        line(
            f,
            row(inner, i as u16),
            Line::from(vec![
                Span::styled(format!("{name:<17}"), theme::muted()),
                Span::raw(value.as_str()),
            ]),
        );
    }
    fan::draw(f, region(inner, 0, 7, inner.width, 3), app);
    line(
        f,
        row(inner, 10),
        Line::styled("Space freezes display; sampling continues.", theme::muted()),
    );
    line(
        f,
        row(inner, 11),
        Line::styled("Conditions and drift checks remain active.", theme::muted()),
    );
}

fn events(f: &mut Frame, area: Rect, app: &App) {
    let inner = if area.height >= 3 {
        panel(f, area, "DRIFT EVENT LOG", theme::BORDER)
    } else {
        area
    };
    let header = inner.height >= 6;
    if header {
        line(
            f,
            row(inner, 0),
            Line::styled("TIME / EVENT — newest first", theme::muted()),
        );
    }
    if app.events.is_empty() {
        line(
            f,
            row(inner, u16::from(header)),
            Line::styled("No drift events recorded", theme::muted()),
        );
        return;
    }
    let lines = app
        .events
        .iter()
        .take(inner.height as usize)
        .map(|event| {
            let (label, color) = match event.level {
                Level::Info => ("[INFO]", theme::MUTED),
                Level::Ok => ("[CLEAR]", theme::ACCENT),
                Level::Warn => ("[WARN]", theme::WARN),
                Level::Bad => ("[BAD]", theme::CRITICAL),
            };
            Line::from(vec![
                Span::styled(
                    format!("T+{:02}:{:02} ", event.at_s / 60, event.at_s % 60),
                    theme::muted(),
                ),
                Span::styled(format!("{label} "), theme::strong(color)),
                Span::styled(event.msg.as_str(), Style::default().fg(color)),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        region(
            inner,
            0,
            u16::from(header),
            inner.width,
            inner.height.saturating_sub(u16::from(header)),
        ),
    );
}
