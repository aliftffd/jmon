use super::{
    chart, fan,
    gauge::GradientBar,
    memory, power, theme, thermal, watched,
    widgets::{columns, line, panel, region, row},
    workload,
};
use crate::{app::App, model::Cpu};
use ratatui::{layout::Rect, text::Line, Frame};

pub fn draw(f: &mut Frame, area: Rect, app: &App, wide: bool) {
    if !wide {
        compact(f, area, app);
        return;
    }
    let mut y = 0;
    if app.snap.workload.is_some() {
        workload::strip(f, region(area, 0, y, area.width, 4), app);
        y += 4;
    }
    if app.snap.watched.is_some() {
        let height = watched::height(area.width);
        watched::draw(f, region(area, 0, y, area.width, height), app);
        y += height;
    }
    if y > 0 {
        monitoring(
            f,
            region(area, 0, y, area.width, area.height.saturating_sub(y)),
            app,
        );
        return;
    }
    let (left, right) = columns(area, 62, 2);
    let top_h = (area.height * 45 / 100).max(12).min(area.height);
    cpu(f, region(left, 0, 0, left.width, top_h), app, true);
    gpu(f, region(right, 0, 0, right.width, top_h), app, true);
    let bottom_y = top_h + 1;
    let bottom_h = area.height.saturating_sub(bottom_y);
    let mem_h = 9.min(bottom_h);
    memory::draw(f, region(left, 0, bottom_y, left.width, mem_h), app);
    power::overview(
        f,
        region(
            left,
            0,
            bottom_y + mem_h + 1,
            left.width,
            bottom_h.saturating_sub(mem_h + 1),
        ),
        app,
    );
    thermal::overview(f, region(right, 0, bottom_y, right.width, bottom_h), app);
}

fn monitoring(f: &mut Frame, area: Rect, app: &App) {
    let top_h = if area.height >= 24 { 10 } else { 8 };
    let (left, right) = columns(region(area, 0, 0, area.width, top_h), 62, 2);
    cpu(f, left, app, false);
    gpu(f, right, app, false);
    let rest = region(
        area,
        0,
        top_h,
        area.width,
        area.height.saturating_sub(top_h + 2),
    );
    let (left, right) = columns(rest, 62, 2);
    memory::draw(f, left, app);
    thermal::overview(f, right, app);
    let bottom = region(area, 0, top_h + rest.height, area.width, 2);
    line(f, row(bottom, 0), power::summary(app));
    power::gauge(f, row(bottom, 1), app);
}

fn core(f: &mut Frame, area: Rect, cpu: &Cpu) {
    line(
        f,
        region(area, 0, 0, 5, 1),
        Line::styled(format!("cpu{}", cpu.id), theme::muted()),
    );
    if !cpu.online {
        line(
            f,
            region(area, 5, 0, area.width.saturating_sub(5), 1),
            Line::styled("OFFLINE", theme::muted()),
        );
        return;
    }
    let bar_w = area
        .width
        .saturating_sub(23)
        .max(1)
        .min(area.width.saturating_sub(5));
    f.render_widget(
        GradientBar {
            ratio: cpu.util_pct / 100.0,
        },
        region(area, 5, 0, bar_w, 1),
    );
    line(
        f,
        region(area, bar_w + 6, 0, 17, 1),
        format!("{:>5.1}% {:>4} MHz", cpu.util_pct, cpu.cur_mhz),
    );
}

fn cpu(f: &mut Frame, area: Rect, app: &App, full: bool) {
    let title = if full {
        format!("CPU / {} CORES", app.snap.cpus.len())
    } else {
        format!(
            "CPU {:.1}% / {} CORES",
            app.snap.cpu_total_pct,
            app.snap.cpus.len()
        )
    };
    let inner = panel(f, area, title, theme::ACCENT);
    let side_chart = full && inner.width >= 74;
    let (table, plot) = if side_chart {
        (
            region(inner, 0, 0, 34, inner.height),
            region(
                inner,
                36,
                0,
                inner.width.saturating_sub(36),
                inner.height.saturating_sub(2),
            ),
        )
    } else {
        (inner, Rect::default())
    };
    let start = if full { 2 } else { 0 };
    if full {
        line(
            f,
            row(table, 0),
            Line::styled(
                format!(
                    "TOTAL {:.1}%  ONLINE {}/{}",
                    app.snap.cpu_total_pct,
                    app.snap.cpus.iter().filter(|c| c.online).count(),
                    app.snap.cpus.len()
                ),
                theme::strong(theme::TEXT),
            ),
        );
        line(
            f,
            row(table, 1),
            Line::styled("CORE  UTILIZATION        CLOCK", theme::muted()),
        );
    }
    let reserve = if full { 3 } else { 0 };
    let count = app
        .snap
        .cpus
        .len()
        .min(table.height.saturating_sub(start + reserve) as usize);
    for (i, value) in app.snap.cpus.iter().take(count).enumerate() {
        core(f, row(table, start + i as u16), value);
    }
    if app.snap.cpus.is_empty() {
        line(
            f,
            row(table, start),
            Line::styled("CPU readings n/a", theme::muted()),
        );
    }
    let y = start + count as u16;
    line(
        f,
        row(table, y),
        Line::styled(
            format!(
                "LOAD {:.2} / {:.2} / {:.2}",
                app.snap.load_avg[0], app.snap.load_avg[1], app.snap.load_avg[2]
            ),
            theme::muted(),
        ),
    );
    if full {
        let governor = app
            .snap
            .cpus
            .iter()
            .find(|c| c.online)
            .map_or("n/a", |c| c.governor.as_str());
        line(
            f,
            row(table, y + 1),
            Line::styled(format!("GOV {governor}"), theme::muted()),
        );
        chart::history(
            f,
            plot,
            &app.hist.cpu_total,
            "CPU TOTAL / %",
            100.0,
            theme::ACCENT,
            !app.snap.cpus.is_empty(),
        );
        if side_chart {
            line(
                f,
                region(
                    inner,
                    36,
                    inner.height.saturating_sub(2),
                    inner.width.saturating_sub(36),
                    1,
                ),
                Line::styled(
                    format!(
                        "avg {:.1}%  peak {:.1}%",
                        app.hist.cpu_total.avg(),
                        app.hist.cpu_total.max()
                    ),
                    theme::muted(),
                ),
            );
        }
        line(
            f,
            row(inner, inner.height.saturating_sub(1)),
            Line::styled(engines(app), theme::muted()),
        );
    }
}

fn gpu(f: &mut Frame, area: Rect, app: &App, full: bool) {
    let title = app.snap.gpu.as_ref().map_or_else(
        || "GPU / n/a".to_string(),
        |g| format!("GPU / {:.1}% BUSY", g.load_pct),
    );
    let inner = panel(f, area, title, theme::BORDER);
    let Some(gpu) = app.snap.gpu.as_ref() else {
        line(
            f,
            row(inner, 0),
            Line::styled("GPU sensor n/a", theme::muted()),
        );
        return;
    };
    line(
        f,
        row(inner, 0),
        Line::styled(
            format!("{} MHz / {}", gpu.cur_mhz, gpu.governor),
            theme::strong(theme::TEXT),
        ),
    );
    f.render_widget(
        GradientBar {
            ratio: gpu.load_pct / 100.0,
        },
        row(inner, 1),
    );
    let range = match (gpu.steps_mhz.first(), gpu.steps_mhz.last()) {
        (Some(lo), Some(hi)) => format!("{lo}…{hi} MHz / {} steps", gpu.steps_mhz.len()),
        _ => "Supported steps n/a".to_string(),
    };
    line(f, row(inner, 2), Line::styled(range, theme::muted()));
    if full {
        line(
            f,
            row(inner, 3),
            Line::styled(
                format!("Limits {}…{} MHz", gpu.min_mhz, gpu.max_mhz),
                theme::muted(),
            ),
        );
        chart::history(
            f,
            region(inner, 0, 4, inner.width, inner.height.saturating_sub(5)),
            &app.hist.gpu_load,
            "GPU / %",
            100.0,
            theme::ACCENT,
            true,
        );
        line(
            f,
            row(inner, inner.height.saturating_sub(1)),
            Line::styled(
                format!(
                    "avg {:.1}%  peak {:.1}%",
                    app.hist.gpu_load.avg(),
                    app.hist.gpu_load.max()
                ),
                theme::muted(),
            ),
        );
    } else {
        line(
            f,
            row(inner, 3),
            Line::styled(
                format!(
                    "avg {:.1}%  peak {:.1}%",
                    app.hist.gpu_load.avg(),
                    app.hist.gpu_load.max()
                ),
                theme::muted(),
            ),
        );
        line(
            f,
            row(inner, 4),
            Line::styled(
                format!(
                    "LOAD {:.2}/{:.2}/{:.2}",
                    app.snap.load_avg[0], app.snap.load_avg[1], app.snap.load_avg[2]
                ),
                theme::muted(),
            ),
        );
        line(f, row(inner, 5), Line::styled(emc(app), theme::muted()));
    }
}

fn emc(app: &App) -> String {
    app.snap
        .emc_mhz
        .map_or_else(|| "EMC n/a (root)".into(), |v| format!("EMC {v} MHz"))
}

fn engines(app: &App) -> String {
    let mut items: Vec<(String, u32, usize)> = Vec::new();
    for (name, mhz) in &app.snap.engines {
        if let Some(entry) = items.iter_mut().find(|(n, v, _)| n == name && v == mhz) {
            entry.2 += 1;
        } else {
            items.push((name.clone(), *mhz, 1));
        }
    }
    let clocks = items
        .iter()
        .map(|(name, mhz, count)| {
            if *count > 1 {
                format!("{name}×{count} {mhz}")
            } else {
                format!("{name} {mhz}")
            }
        })
        .collect::<Vec<_>>()
        .join(" / ");
    if clocks.is_empty() {
        "ENGINE CLOCKS n/a | busy: root only".into()
    } else {
        format!("ENGINES {clocks} MHz | busy: root only")
    }
}

fn compact(f: &mut Frame, area: Rect, app: &App) {
    let mut y = 0;
    if app.snap.workload.is_some() {
        workload::strip(f, region(area, 0, y, area.width, 4), app);
        y += 4;
    }
    if app.snap.watched.is_some() {
        let height = watched::height(area.width);
        watched::draw(f, region(area, 0, y, area.width, height), app);
        y += height;
    }
    if y > 0 {
        let gpu = app.snap.gpu.as_ref().map_or_else(
            || "GPU n/a".into(),
            |g| format!("GPU {:.1}% / {} MHz", g.load_pct, g.cur_mhz),
        );
        line(
            f,
            row(area, y),
            format!("CPU {:.1}%  |  {gpu}", app.snap.cpu_total_pct),
        );
        y += 1;
    } else {
        let (left, right) = columns(region(area, 0, 0, area.width, 8), 61, 1);
        cpu(f, left, app, false);
        gpu(f, right, app, false);
        y = 8;
    }
    memory::compact(f, region(area, 0, y, area.width, 5), app);
    y += 5;
    line(f, row(area, y), fan::summary(app));
    line(f, row(area, y + 1), power::summary(app));
    y += 2;
    let (left, right) = columns(
        region(area, 0, y, area.width, area.height.saturating_sub(y)),
        50,
        2,
    );
    for (i, temp) in app.snap.temps.iter().take(6).enumerate() {
        thermal::zone(
            f,
            row(if i % 2 == 0 { left } else { right }, (i / 2) as u16),
            temp,
        );
    }
}
