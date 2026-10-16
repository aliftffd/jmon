use super::{
    gauge::GradientBar,
    theme,
    widgets::{line, panel, region, row},
};
use crate::app::App;
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Sparkline,
    Frame,
};

fn gib(kib: u64) -> f64 {
    kib as f64 / 1_048_576.0
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let mem = &app.snap.mem;
    let inner = panel(
        f,
        area,
        format!(
            "Shared CPU/GPU RAM · SWAP {:.2}/{:.2} GiB",
            gib(mem.swap_total_kib.saturating_sub(mem.swap_free_kib)),
            gib(mem.swap_total_kib)
        ),
        theme::BORDER,
    );
    let next_row = compact(f, inner, app);
    if inner.height > next_row {
        if app.hist.swap_out_pages_s.data.iter().any(|&v| v > 0.0) {
            let plot = region(inner, 21, next_row, inner.width.saturating_sub(21), 1);
            line(
                f,
                region(inner, 0, next_row, 21, 1),
                Line::styled("SWAP OUT / pages/s", theme::muted()),
            );
            let data: Vec<u64> = app
                .hist
                .swap_out_pages_s
                .data
                .iter()
                .rev()
                .take(plot.width as usize)
                .rev()
                .map(|v| {
                    if v.is_finite() {
                        (v.max(0.0) * 1000.0) as u64
                    } else {
                        0
                    }
                })
                .collect();
            f.render_widget(
                Sparkline::default()
                    .data(&data)
                    .style(theme::strong(theme::WARN)),
                plot,
            );
        } else {
            line(
                f,
                row(inner, next_row),
                Line::styled("swap traffic: none", theme::muted()),
            );
        }
    }
    line(
        f,
        row(inner, next_row + 1),
        Line::styled(
            "Used = total − available (includes reclaimable cache)",
            theme::muted(),
        ),
    );
    if inner.height > next_row + 2 {
        let emc = app
            .snap
            .emc_mhz
            .map_or_else(|| "EMC n/a (root)".into(), |mhz| format!("EMC {mhz} MHz"));
        line(
            f,
            row(inner, next_row + 2),
            Line::styled(emc, theme::muted()),
        );
    }
}

/// Prioritize RAM and present swap tiers, then rates; return the next row.
pub fn compact(f: &mut Frame, area: Rect, app: &App) -> u16 {
    let mem = &app.snap.mem;
    let swap = &app.snap.swap;
    let used = if mem.total_kib > 0 {
        format!(
            "USED {:.2}/{:.2} GiB",
            gib(mem.used_kib()),
            gib(mem.total_kib)
        )
    } else {
        "RAM n/a".into()
    };
    let delta = app.mem_used_start_kib.map_or_else(
        || "n/a".into(),
        |start| {
            format!(
                "{:+.2} GiB",
                (mem.used_kib() as f64 - start as f64) / 1_048_576.0
            )
        },
    );
    let mut spans = vec![
        Span::styled(used, theme::strong(theme::TEXT)),
        Span::styled(format!("  Δ since start {delta}"), theme::muted()),
    ];
    if swap.out_pages_s > 0.0 {
        spans.push(Span::styled("  SWAPPING", theme::strong(theme::WARN)));
    }
    line(f, row(area, 0), Line::from(spans));
    let bar_w = area.width.saturating_sub(36).min(30);
    f.render_widget(
        GradientBar {
            ratio: mem.used_kib() as f64 / mem.total_kib.max(1) as f64,
        },
        region(area, 0, 1, bar_w, 1),
    );
    line(
        f,
        region(area, bar_w + 1, 1, area.width.saturating_sub(bar_w + 1), 1),
        Line::styled(
            format!(
                "avail {:.2} · cache {:.2} GiB",
                gib(mem.avail_kib),
                gib(mem.cached_kib)
            ),
            theme::muted(),
        ),
    );
    let mut y = 2;
    for (label, used, total, description, style) in [
        (
            "zram",
            swap.zram_used_kib,
            swap.zram_total_kib,
            "compressed in RAM",
            theme::muted(),
        ),
        (
            "NVMe swp",
            swap.file_used_kib,
            swap.file_total_kib,
            "slow",
            if swap.file_used_kib > 0 {
                theme::strong(theme::CRITICAL)
            } else {
                theme::muted()
            },
        ),
    ] {
        if total == 0 {
            continue;
        }
        line(f, region(area, 0, y, 8, 1), Line::styled(label, style));
        f.render_widget(
            GradientBar {
                ratio: used as f64 / total as f64,
            },
            region(area, 9, y, bar_w, 1),
        );
        let text_x = 10 + bar_w;
        line(
            f,
            region(area, text_x, y, area.width.saturating_sub(text_x), 1),
            Line::styled(
                format!("{:.2}/{:.2} GiB · {description}", gib(used), gib(total)),
                theme::muted(),
            ),
        );
        y += 1;
    }
    line(
        f,
        row(area, y),
        Line::styled(
            format!(
                "swap in {:.2} / out {:.2} MiB/s · majflt {:.1}/s",
                swap.in_pages_s / 256.0,
                swap.out_pages_s / 256.0,
                swap.majflt_s
            ),
            theme::muted(),
        ),
    );
    y + 1
}
