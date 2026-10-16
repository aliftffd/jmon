use super::{
    chart,
    gauge::GradientBar,
    theme,
    widgets::{columns, line, panel, region, row},
    workload,
};
use crate::{app::App, model::Rail};
use ratatui::{
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Cell, Row, Table},
    Frame,
};

pub fn input(app: &App) -> Option<&Rail> {
    app.snap.rails.iter().find(|r| r.name == "VDD_IN")
}

pub fn summary(app: &App) -> Line<'static> {
    if app.snap.rails.is_empty() {
        return Line::styled("VDD_IN n/a · Rail sensors n/a", theme::muted());
    }
    let mut spans = Vec::new();
    for (i, rail) in app.snap.rails.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", theme::muted()));
        }
        spans.push(Span::styled(
            format!("{} {:.2} W", rail.name, rail.watts),
            if rail.name == "VDD_IN" {
                theme::strong(theme::ACCENT_HI)
            } else {
                theme::muted()
            },
        ));
    }
    Line::from(spans)
}

pub fn ceiling(app: &App) -> f64 {
    (app.hist
        .vdd_in_w
        .max()
        .max(input(app).map_or(0.0, |r| r.watts))
        / 5.0)
        .ceil()
        .max(3.0)
        * 5.0
}

fn budget(app: &App) -> Option<f64> {
    // Only an explicitly reported wattage is a budget; MAXN is not 15 W.
    app.snap
        .power_mode
        .as_ref()?
        .name
        .strip_suffix('W')?
        .parse::<f64>()
        .ok()
        .filter(|w| w.is_finite() && *w > 0.0)
}

pub fn gauge(f: &mut Frame, area: Rect, app: &App) {
    let Some(rail) = input(app) else {
        line(f, row(area, 0), Line::styled("VDD_IN n/a", theme::muted()));
        return;
    };
    let scale = budget(app).unwrap_or_else(|| ceiling(app));
    f.render_widget(
        GradientBar {
            ratio: rail.watts / scale,
        },
        row(area, 0),
    );
    let label = if budget(app).is_some() {
        format!("{scale:.0} W mode budget")
    } else {
        format!("0–{scale:.0} W display scale")
    };
    line(f, row(area, 1), Line::styled(label, theme::muted()));
}

pub fn rails(f: &mut Frame, area: Rect, app: &App) {
    if app.snap.rails.is_empty() {
        line(
            f,
            row(area, 0),
            Line::styled("Rail sensors n/a", theme::muted()),
        );
        return;
    }
    let electrical = area.width >= 39;
    let rows = app.snap.rails.iter().map(|rail| {
        let style = if rail.name == "VDD_IN" {
            theme::strong(theme::ACCENT_HI)
        } else {
            theme::muted()
        };
        let mut cells = vec![
            Cell::from(rail.name.as_str()),
            Cell::from(format!("{:.2}", rail.watts)),
        ];
        if electrical {
            cells.extend([
                Cell::from(format!("{:.2}", rail.volts)),
                Cell::from(format!("{:.3}", rail.amps)),
            ]);
        }
        Row::new(cells).style(style)
    });
    let (labels, widths) = if electrical {
        (
            vec!["RAIL", "W", "V", "A"],
            vec![
                Constraint::Min(14),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(7),
            ],
        )
    } else {
        (
            vec!["RAIL", "W"],
            vec![Constraint::Min(14), Constraint::Length(7)],
        )
    };
    f.render_widget(
        Table::new(rows, widths)
            .header(Row::new(labels).style(theme::muted()))
            .column_spacing(1),
        area,
    );
}

pub fn overview(f: &mut Frame, area: Rect, app: &App) {
    let inner = panel(f, area, "POWER / RAILS", theme::BORDER);
    let (left, right) = if inner.width >= 75 {
        columns(inner, 47, 2)
    } else {
        (inner, Rect::default())
    };
    let count = (app.snap.rails.len() as u16)
        .saturating_add(1)
        .min(left.height);
    rails(f, region(left, 0, 0, left.width, count), app);
    gauge(f, region(left, 0, count, left.width, 2), app);
    line(
        f,
        row(inner, inner.height.saturating_sub(1)),
        Line::styled("Separate rail readings; not additive.", theme::muted()),
    );
    chart::history(
        f,
        region(right, 0, 0, right.width, right.height.saturating_sub(1)),
        &app.hist.vdd_in_w,
        "VDD_IN / W",
        ceiling(app),
        theme::ACCENT_HI,
        input(app).is_some(),
    );
}

pub fn live(f: &mut Frame, area: Rect, app: &App) {
    let inner = panel(f, area, "VDD_IN / LIVE POWER", theme::BORDER);
    line(
        f,
        row(inner, 0),
        Line::styled("live estimate — not campaign data", theme::muted()),
    );
    let reading = input(app).map_or_else(
        || "VDD_IN n/a".to_string(),
        |r| {
            format!(
                "{:.2} W   {:.2} V × {:.3} A   |   {} Hz",
                r.watts,
                r.volts,
                r.amps,
                super::widgets::rate(app)
            )
        },
    );
    line(
        f,
        row(inner, 1),
        Line::styled(reading, theme::strong(theme::ACCENT_HI)),
    );
    line(
        f,
        row(inner, 2),
        Line::styled(
            format!("0–{:.0} W display scale", ceiling(app)),
            theme::muted(),
        ),
    );
    chart::history(
        f,
        region(inner, 0, 3, inner.width, inner.height.saturating_sub(3)),
        &app.hist.vdd_in_w,
        "VDD_IN / W",
        ceiling(app),
        theme::ACCENT_HI,
        input(app).is_some(),
    );
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    if app.snap.workload.is_some() {
        if area.width >= 110 {
            let (power, work) = columns(area, 40, 2);
            power_panels(f, power, app);
            workload::draw(f, work, app);
        } else {
            let inner = panel(
                f,
                region(area, 0, 0, area.width, 4),
                "VDD_IN / LIVE POWER",
                theme::BORDER,
            );
            line(f, row(inner, 0), summary(app));
            line(
                f,
                row(inner, 1),
                Line::styled(
                    "live estimate — not campaign data · separate rails, not additive",
                    theme::muted(),
                ),
            );
            workload::draw(
                f,
                region(area, 0, 4, area.width, area.height.saturating_sub(4)),
                app,
            );
        }
    } else {
        power_panels(f, area, app);
    }
}

fn power_panels(f: &mut Frame, area: Rect, app: &App) {
    let top_h = 9.min(area.height);
    let inner = panel(
        f,
        region(area, 0, 0, area.width, top_h),
        "POWER / SEPARATE RAILS",
        theme::BORDER,
    );
    let (table, gauge_area) = if inner.width >= 80 {
        columns(inner, 62, 3)
    } else {
        (inner, Rect::default())
    };
    rails(
        f,
        region(
            table,
            0,
            0,
            table.width,
            table
                .height
                .saturating_sub(if gauge_area.is_empty() { 3 } else { 1 }),
        ),
        app,
    );
    if gauge_area.is_empty() {
        gauge(f, region(table, 0, 4, table.width, 2), app);
    } else {
        gauge(f, region(gauge_area, 0, 1, gauge_area.width, 2), app);
    }
    line(
        f,
        row(gauge_area, 3),
        Line::styled("VDD_IN input power", theme::strong(theme::ACCENT_HI)),
    );
    line(
        f,
        row(inner, inner.height.saturating_sub(1)),
        Line::styled("Separate rail readings; not additive.", theme::muted()),
    );
    live(
        f,
        region(
            area,
            0,
            top_h,
            area.width,
            area.height.saturating_sub(top_h),
        ),
        app,
    );
}
