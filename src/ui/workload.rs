use super::{
    chart, theme,
    widgets::{columns, elide, line, panel, region, row},
};
use crate::{app::App, model::Workload};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    Frame,
};

fn number(value: f64, available: bool) -> String {
    if available && value.is_finite() {
        format!("{value:.1}")
    } else {
        "n/a".into()
    }
}

fn headline(w: &Workload) -> Line<'static> {
    let unit = if w.tok_ms_p50.is_some() {
        "tok/s"
    } else {
        "inf/s"
    };
    let latency = if let Some(p50) = w.tok_ms_p50 {
        format!(
            "{} ms/token p50  {} p95",
            number(p50, w.samples_in_window > 0),
            number(w.tok_ms_p95.unwrap_or(f64::NAN), w.samples_in_window > 0)
        )
    } else {
        format!("{} ms p95", number(w.p95_ms, w.samples_in_window > 0))
    };
    Line::from(vec![
        Span::styled(
            format!("{} {unit}", number(w.inf_per_s, w.total_lines > 0)),
            theme::strong(theme::ACCENT_HI),
        ),
        Span::raw("  |  "),
        Span::styled(latency, theme::strong(theme::TEXT)),
    ])
}

fn secondary(w: &Workload) -> String {
    let valid = w.samples_in_window > 0;
    if w.tok_ms_p50.is_some() {
        format!(
            "request p50 {} / p95 {} / p99 {} ms",
            number(w.p50_ms, valid),
            number(w.p95_ms, valid),
            number(w.p99_ms, valid)
        )
    } else {
        format!(
            "p50 {} / p99 {} ms",
            number(w.p50_ms, valid),
            number(w.p99_ms, valid)
        )
    }
}

fn warning(w: &Workload) -> Option<Line<'static>> {
    if w.total_lines == 0 {
        Some(Line::styled(
            format!("waiting for first line in {}", w.path),
            theme::muted(),
        ))
    } else if w.stalled() {
        Some(Line::styled(
            format!("WORKLOAD STALLED (no line for {:.1} s)", w.idle_s),
            theme::strong(theme::WARN),
        ))
    } else {
        None
    }
}

fn title(w: &Workload) -> String {
    format!("WORKLOAD · last {} s", w.window_s)
}

pub fn strip(f: &mut Frame, area: Rect, app: &App) {
    let Some(w) = &app.snap.workload else { return };
    let inner = panel(f, area, title(w), theme::ACCENT);
    line(f, row(inner, 0), headline(w));
    let detail = warning(w).unwrap_or_else(|| {
        Line::styled(
            format!(
                "{} · {} samples · {}",
                secondary(w),
                w.samples_in_window,
                w.engine
            ),
            theme::muted(),
        )
    });
    line(f, row(inner, 1), detail);
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let Some(w) = &app.snap.workload else { return };
    let inner = panel(f, area, title(w), theme::ACCENT);
    line(f, row(inner, 0), headline(w));
    line(f, row(inner, 1), Line::styled(secondary(w), theme::muted()));
    let details = format!(" · batch {} · {} samples", w.batch, w.samples_in_window);
    let engine = elide(
        if w.engine.is_empty() {
            "n/a"
        } else {
            &w.engine
        },
        inner
            .width
            .saturating_sub((7 + Line::from(details.as_str()).width()) as u16),
    );
    line(
        f,
        row(inner, 2),
        Line::styled(format!("engine {engine}{details}"), theme::muted()),
    );
    line(
        f,
        row(inner, 3),
        Line::styled(
            format!(
                "lines {} · bad {} · idle {:.1} s",
                w.total_lines, w.bad_lines, w.idle_s
            ),
            theme::muted(),
        ),
    );
    let mut y = 4;
    if let Some(warning) = warning(w) {
        line(f, row(inner, y), warning);
        y += 1;
    }
    if let Some(deadline) = w.deadline_ms {
        line(
            f,
            row(inner, y),
            Line::styled(
                format!(
                    "deadline {deadline:.1} ms · misses {} window / {} total",
                    w.misses_window, w.misses_total
                ),
                if w.misses_window > 0 {
                    theme::strong(theme::WARN)
                } else {
                    theme::muted()
                },
            ),
        );
        y += 1;
    }
    // inf_per_watt is computed from board input power, including idle draw.
    // Preserve the contract label, and clarify the numerator for token writers.
    let estimate = number(w.inf_per_watt.unwrap_or(f64::NAN), w.total_lines > 0);
    let efficiency = "inf/s/W — board-level live estimate (includes idle)";
    let inline = format!("{estimate} {efficiency}");
    if Line::from(inline.as_str()).width() <= inner.width as usize {
        line(f, row(inner, y), Line::styled(inline, theme::muted()));
        y += 1;
    } else {
        line(
            f,
            row(inner, y),
            Line::styled(
                format!("{estimate} inf/s/W — board-level live estimate"),
                theme::muted(),
            ),
        );
        line(
            f,
            row(inner, y + 1),
            Line::styled("(includes idle)", theme::muted()),
        );
        y += 2;
    }
    if w.tok_ms_p50.is_some() {
        line(
            f,
            row(inner, y),
            Line::styled("LLM efficiency numerator = tok/s", theme::muted()),
        );
        y += 1;
    }
    if !w.stages.is_empty() {
        stages(f, region(inner, 0, y, inner.width, 2), &w.stages);
        y += 2;
    }
    let (throughput, latency) = columns(
        region(inner, 0, y, inner.width, inner.height.saturating_sub(y)),
        50,
        2,
    );
    chart::history(
        f,
        throughput,
        &app.hist.inf_per_s,
        if w.tok_ms_p50.is_some() {
            "THROUGHPUT / tok/s"
        } else {
            "THROUGHPUT / inf/s"
        },
        app.hist.inf_per_s.max(),
        theme::ACCENT_HI,
        w.total_lines > 0,
    );
    chart::history(
        f,
        latency,
        &app.hist.p95_ms,
        "REQUEST p95 / ms",
        app.hist.p95_ms.max(),
        theme::WARN,
        w.total_lines > 0,
    );
}

fn stage_color(index: usize) -> Color {
    [theme::ACCENT, theme::ACCENT_HI, theme::MUTED, theme::WARN][index % 4]
}

fn stages(f: &mut Frame, area: Rect, stages: &[(String, f64)]) {
    let total: f64 = stages
        .iter()
        .map(|(_, ms)| if ms.is_finite() { ms.max(0.0) } else { 0.0 })
        .sum();
    let mut x = 0;
    let mut sum = 0.0;
    let mut legend = vec![Span::styled("STAGES mean ms: ", theme::muted())];
    for (i, (name, ms)) in stages.iter().enumerate() {
        let color = stage_color(i);
        sum += if ms.is_finite() { ms.max(0.0) } else { 0.0 };
        let end = if total > 0.0 {
            (sum / total * f64::from(area.width)).round() as u16
        } else {
            0
        };
        let width = end.saturating_sub(x);
        line(
            f,
            region(area, x, 0, width, 1),
            Line::styled("█".repeat(width as usize), Style::default().fg(color)),
        );
        x = end;
        legend.push(Span::styled(
            format!("{name} {}  ", number(*ms, true)),
            Style::default().fg(color),
        ));
    }
    line(f, row(area, 1), Line::from(legend));
}
