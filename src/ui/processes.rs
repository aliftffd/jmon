use super::{
    gauge::GradientBar,
    theme, watched,
    widgets::{line, panel, region, row},
};
use crate::{
    app::{App, ProcSort},
    model::Proc,
};
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Block, Paragraph, Wrap},
    Frame,
};

const FOOTNOTE: &str = "GPU = has the GPU device open · per-process GPU % is not available on Orin · MEM = CPU-visible RSS; GPU buffers may not be included";

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let watched_h = if app.snap.watched.is_some() {
        watched::height(area.width).min(area.height)
    } else {
        0
    };
    watched::draw(f, region(area, 0, 0, area.width, watched_h), app);
    let table = region(
        area,
        0,
        watched_h,
        area.width,
        area.height.saturating_sub(watched_h),
    );
    let sort = match app.proc_sort {
        ProcSort::Mem => "MEM (RSS)",
        ProcSort::Cpu => "CPU%",
    };
    let inner = panel(
        f,
        table,
        format!("PROCESSES · sort {sort} ↓ · s toggles"),
        theme::BORDER,
    );
    let note_lines = if Line::from(FOOTNOTE).width() <= inner.width as usize {
        vec![Line::from(FOOTNOTE)]
    } else {
        vec![
            Line::from(
                "GPU = has the GPU device open · per-process GPU % is not available on Orin",
            ),
            Line::from("MEM = CPU-visible RSS; GPU buffers may not be included"),
        ]
    };
    let note_h = (note_lines.len() as u16).min(inner.height);
    let note = Paragraph::new(note_lines)
        .style(theme::muted())
        .wrap(Wrap { trim: true });
    let rows_h = inner.height.saturating_sub(note_h);
    f.render_widget(note, region(inner, 0, rows_h, inner.width, note_h));
    let table = region(inner, 0, 0, inner.width, rows_h);
    if app.snap.procs.is_empty() {
        line(
            f,
            row(table, 0),
            Line::styled(
                if app.bench {
                    "process table off in bench mode (key b)"
                } else {
                    "No process readings yet"
                },
                theme::muted(),
            ),
        );
        return;
    }
    let widths = if inner.width >= 110 {
        [7, 12, 3, 7, 28, 10]
    } else {
        [7, 9, 3, 7, 21, 9]
    };
    let labels = ["PID", "USER", "GPU", "CPU%", "MEM (RSS)", "SWAP", "COMMAND"];
    let mut x = 0;
    for (i, label) in labels.iter().enumerate() {
        let width = widths
            .get(i)
            .copied()
            .unwrap_or(inner.width.saturating_sub(x));
        line(
            f,
            region(table, x, 0, width, 1),
            Line::styled(*label, theme::muted()),
        );
        x += width + 1;
    }
    let watched_pid = app
        .snap
        .watched
        .as_ref()
        .and_then(|w| w.proc.as_ref())
        .map(|p| p.pid);
    for (i, proc) in app
        .snap
        .procs
        .iter()
        .take(table.height.saturating_sub(1) as usize)
        .enumerate()
    {
        let area = row(table, i as u16 + 1);
        if watched_pid == Some(proc.pid) {
            f.render_widget(
                Block::default().style(theme::strong(theme::ACCENT_HI).bg(theme::BORDER)),
                area,
            );
        }
        process(f, area, proc, widths, app.snap.mem.total_kib);
    }
}

fn process(f: &mut Frame, area: Rect, proc: &Proc, widths: [u16; 6], total_kib: u64) {
    let mut x = 0;
    let values = [
        proc.pid.to_string(),
        proc.user.clone(),
        String::new(),
        format!("{:.1}", proc.cpu_pct),
        String::new(),
        if proc.swap_kib == 0 {
            "0".into()
        } else {
            format!("{:.2} GiB", proc.swap_kib as f64 / 1_048_576.0)
        },
    ];
    for (i, width) in widths.into_iter().enumerate() {
        let cell = region(area, x, 0, width, 1);
        match i {
            2 => line(f, cell, Line::from(watched::gpu_badge(proc.gpu))),
            4 => {
                line(
                    f,
                    region(cell, 0, 0, 10, 1),
                    format!("{:.2} GiB", proc.rss_kib as f64 / 1_048_576.0),
                );
                f.render_widget(
                    GradientBar {
                        ratio: proc.rss_kib as f64 / total_kib.max(1) as f64,
                    },
                    region(cell, 10, 0, width.saturating_sub(10), 1),
                );
            }
            _ => line(f, cell, values[i].as_str()),
        }
        x += width + 1;
    }
    // Paragraph clips commands by terminal cell width, including Unicode commands.
    line(
        f,
        region(area, x, 0, area.width.saturating_sub(x), 1),
        proc.cmd.as_str(),
    );
}
