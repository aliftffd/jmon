use super::{
    chart, theme,
    widgets::{elide, line, panel, region, row},
};
use crate::{app::App, model::GpuUse};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    Frame,
};

pub fn gpu_badge(gpu: GpuUse) -> Span<'static> {
    match gpu {
        GpuUse::Yes => Span::styled("●", theme::strong(theme::ACCENT)),
        GpuUse::Unknown => Span::styled("?", theme::muted()),
        GpuUse::No => Span::raw(" "),
    }
}

pub fn height(width: u16) -> u16 {
    if width >= 110 {
        8
    } else {
        6
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let Some(watched) = &app.snap.watched else {
        return;
    };
    let inner = panel(f, area, "WATCHED MODEL", theme::ACCENT);
    let plot_w = if inner.width >= 106 && inner.height >= 6 {
        inner.width.saturating_sub(72)
    } else {
        0
    };
    let text = region(
        inner,
        0,
        0,
        inner.width.saturating_sub(plot_w),
        inner.height,
    );
    let identity = if let Some(proc) = &watched.proc {
        let pid = format!("  pid {}  ", proc.pid);
        let detail = format!(
            " GPU  CPU {:.0}%  restarts {}",
            proc.cpu_pct, watched.restarts
        );
        let fixed = 7 + Line::from(pid.as_str()).width() + Line::from(detail.as_str()).width();
        let target = elide(&watched.target, text.width.saturating_sub(fixed as u16));
        Line::from(vec![
            Span::styled(format!("MODEL {target}{pid}"), theme::strong(theme::TEXT)),
            gpu_badge(proc.gpu),
            Span::raw(detail),
        ])
    } else {
        let detail = format!("  not running  restarts {}", watched.restarts);
        let fixed = 6 + Line::from(detail.as_str()).width();
        let target = elide(&watched.target, text.width.saturating_sub(fixed as u16));
        Line::styled(
            format!("MODEL {target}{detail}"),
            theme::strong(theme::WARN),
        )
    };
    line(f, row(text, 0), identity);
    let estimate = watched.mem_est_kib.map_or_else(
        || "MODEL MEM n/a — start jmon before the model".into(),
        |kib| format!("MODEL MEM ≈ {:.2} GiB · estimate", kib as f64 / 1_048_576.0),
    );
    line(
        f,
        row(text, 1),
        Line::styled(estimate, theme::strong(theme::ACCENT_HI)),
    );
    line(
        f,
        row(text, 2),
        Line::styled("system Δ since pid appeared · estimate", theme::muted()),
    );
    let rss = watched.proc.as_ref().map_or_else(
        || "RSS (CPU-visible) n/a  SWAP n/a".into(),
        |p| {
            format!(
                "RSS (CPU-visible) {:.2} GiB  SWAP {:.2} GiB",
                p.rss_kib as f64 / 1_048_576.0,
                p.swap_kib as f64 / 1_048_576.0
            )
        },
    );
    line(f, row(text, 3), Line::styled(rss, theme::muted()));
    if plot_w > 0 {
        line(
            f,
            row(text, 4),
            Line::from(vec![
                Span::styled("MODEL ≈", theme::strong(theme::ACCENT_HI)),
                Span::styled(" / SYSTEM · GiB", theme::muted()),
            ]),
        );
        let plot = region(inner, inner.width - plot_w, 0, plot_w, inner.height);
        chart::histories(
            f,
            plot,
            &[
                (&app.hist.mem_used_gib, theme::MUTED),
                (&app.hist.model_mem_gib, theme::ACCENT_HI),
            ],
            "MEMORY / GiB",
            app.hist
                .mem_used_gib
                .max()
                .max(app.hist.model_mem_gib.max()),
            true,
        );
    }
}
