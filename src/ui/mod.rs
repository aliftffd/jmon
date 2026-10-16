//! Read-only terminal presentation. All telemetry and state belong to `App`.

mod benchmark;
mod chart;
mod fan;
mod gauge;
mod memory;
mod overview;
mod power;
mod processes;
mod theme;
mod thermal;
mod throttle;
mod watched;
mod widgets;
mod workload;

use crate::{app::App, model::Tab};
use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use widgets::{line, region, row};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Block::default().style(theme::base()), area);
    let wide = area.width >= 110 && area.height >= 34;
    let header_h = (if wide { 4 } else { 3 }).min(area.height);
    let footer_h = (if wide { 4 } else { 3 }).min(area.height.saturating_sub(header_h));
    let gap = u16::from(wide);
    header(f, region(area, 0, 0, area.width, header_h), app, wide);
    footer(
        f,
        region(
            area,
            0,
            area.height.saturating_sub(footer_h),
            area.width,
            footer_h,
        ),
        app,
        wide,
    );
    let margin = u16::from(wide);
    let body = region(
        area,
        margin,
        header_h + gap,
        area.width.saturating_sub(margin * 2),
        area.height.saturating_sub(header_h + footer_h + gap),
    );
    if !app.has_snapshot {
        centered(f, body, "sampling…", theme::MUTED);
        return;
    }
    match app.tab {
        Tab::Overview => overview::draw(f, body, app, wide),
        Tab::Power => power::draw(f, body, app),
        Tab::Thermal => thermal::draw(f, body, app),
        Tab::Processes => processes::draw(f, body, app),
        Tab::Benchmark => benchmark::draw(f, body, app, wide),
    }
}

fn header(f: &mut Frame, area: Rect, app: &App, wide: bool) {
    f.render_widget(Block::default().style(theme::panel()), area);
    let pad = if wide { 2 } else { 0 };
    let inner = region(
        area,
        pad,
        u16::from(wide),
        area.width.saturating_sub(pad * 2),
        3,
    );
    let status_w = 20.min(inner.width);
    line(
        f,
        region(inner, 0, 0, inner.width.saturating_sub(status_w), 1),
        Line::from(vec![
            Span::styled("JETSON MONITOR", theme::strong(theme::ACCENT)),
            Span::styled(" / ", theme::muted()),
            Span::styled(
                if app.device.model.is_empty() {
                    "NVIDIA Jetson"
                } else {
                    &app.device.model
                },
                theme::strong(theme::TEXT),
            ),
        ]),
    );
    let status = if app.frozen { "FROZEN" } else { "LIVE" };
    f.render_widget(
        Paragraph::new(format!("{} | {status}", tab_name(app.tab)))
            .style(Style::default().fg(if app.frozen {
                theme::WARN
            } else {
                theme::MUTED
            }))
            .alignment(Alignment::Right),
        region(inner, inner.width.saturating_sub(status_w), 0, status_w, 1),
    );
    let mode = app.snap.power_mode.as_ref().map_or_else(
        || "n/a".to_string(),
        |m| format!("{} (mode {})", m.name, m.id),
    );
    let uptime = app.uptime_s();
    line(
        f,
        row(inner, 1),
        // lead: CLOCKS badge (jetson_clocks / custom lock / dynamic), added outside the UI split on request.
        Line::from(vec![
            ratatui::text::Span::styled(
                format!(
                    "{}  |  UP {:02}:{:02}:{:02}  |  nvpmodel {mode}  |  ",
                    if app.device.l4t.is_empty() {
                        "L4T n/a"
                    } else {
                        &app.device.l4t
                    },
                    uptime / 3600,
                    uptime / 60 % 60,
                    uptime % 60,
                ),
                theme::muted(),
            ),
            ratatui::text::Span::styled(
                app.snap.clocks.label(),
                match app.snap.clocks {
                    crate::model::ClockState::Dynamic => theme::muted(),
                    _ => theme::strong(theme::ACCENT_HI),
                },
            ),
        ]),
    );
    let tabs = [
        Tab::Overview,
        Tab::Power,
        Tab::Thermal,
        Tab::Processes,
        Tab::Benchmark,
    ];
    let mut spans = Vec::new();
    for (i, tab) in tabs.into_iter().enumerate() {
        let style = if tab == app.tab {
            theme::strong(theme::ACCENT_HI).bg(theme::BORDER)
        } else {
            theme::muted()
        };
        spans.push(Span::styled(
            format!("[{} {}]", i + 1, tab_name(tab)),
            style,
        ));
        spans.push(Span::raw(if wide { "  " } else { " " }));
    }
    line(f, row(inner, 2), Line::from(spans));
}

fn tab_name(tab: Tab) -> &'static str {
    match tab {
        Tab::Overview => "Overview",
        Tab::Power => "Power",
        Tab::Thermal => "Thermal",
        Tab::Processes => "Processes",
        Tab::Benchmark => "Benchmark",
    }
}

fn footer(f: &mut Frame, area: Rect, app: &App, wide: bool) {
    let offset = u16::from(wide);
    if wide {
        line(
            f,
            row(area, 0),
            Line::styled("─".repeat(area.width as usize), theme::border()),
        );
    }
    let pad = if wide { 2 } else { 0 };
    let inner = region(area, pad, offset, area.width.saturating_sub(pad * 2), 3);
    let self_cost = if app.has_snapshot {
        format!(
            "SELF CPU {:.1}%  RSS {:.1} MiB",
            app.snap.self_cpu_pct, app.snap.self_rss_mib
        )
    } else {
        "SELF CPU n/a  RSS n/a".to_string()
    };
    line(
        f,
        row(inner, 0),
        Line::styled(
            format!(
                "{self_cost}  |  SAMPLE {} Hz  |  DATA AGE {} ms",
                widgets::rate(app),
                app.data_age_ms,
            ),
            theme::muted(),
        ),
    );
    let mut hints = vec![
        Span::styled("1-5", theme::strong(theme::ACCENT)),
        Span::raw(" tabs  "),
        Span::styled("b", theme::strong(theme::ACCENT)),
        Span::raw(" bench  "),
        Span::styled("Space", theme::strong(theme::ACCENT)),
        Span::raw(" freeze  "),
        Span::styled("+/-", theme::strong(theme::ACCENT)),
        Span::raw(" rate  "),
        Span::styled("q", theme::strong(theme::ACCENT)),
        Span::raw(" quit"),
    ];
    if app.tab == Tab::Processes {
        hints.extend([
            Span::raw("  "),
            Span::styled("s", theme::strong(theme::ACCENT)),
            Span::raw(" sort"),
        ]);
    }
    line(f, row(inner, 1), Line::from(hints));
    let state = if app.frozen {
        "FROZEN — sampling continues"
    } else if app.bench {
        "BENCH PRESET | anim paused (bench)"
    } else {
        "LIVE | sampling continues"
    };
    let color = if app.frozen {
        theme::WARN
    } else {
        theme::MUTED
    };
    if wide {
        let width = 38.min(inner.width);
        f.render_widget(
            Paragraph::new(state)
                .style(Style::default().fg(color))
                .alignment(Alignment::Right),
            region(inner, inner.width.saturating_sub(width), 0, width, 1),
        );
    } else {
        line(
            f,
            row(inner, 2),
            Line::styled(state, Style::default().fg(color)),
        );
    }
}

fn centered(f: &mut Frame, area: Rect, text: &str, color: ratatui::style::Color) {
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(color))
            .alignment(Alignment::Center),
        row(area, area.height / 2),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{DeviceInfo, Expected, ProcSort},
        model::{
            Check, CheckState, ClockState, Cpu, DropStats, Fan, Gpu, GpuUse, Mem, PowerMode, Proc,
            Rail, Series, Snapshot, SwapStats, Temp, Throttle, Watched, Workload,
        },
        sensors::Sensors,
    };
    use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, widgets::Widget, Terminal};
    use std::{collections::HashSet, path::Path};

    const TABS: [Tab; 5] = [
        Tab::Overview,
        Tab::Power,
        Tab::Thermal,
        Tab::Processes,
        Tab::Benchmark,
    ];
    const SIZES: [(u16, u16); 3] = [(160, 45), (120, 40), (80, 24)];

    fn fixture_app() -> App {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture");
        let mut sensors = Sensors::discover(&root);
        let device = DeviceInfo {
            model: sensors.device_model(),
            l4t: sensors.l4t_release(),
        };
        let mut app = App::new(Expected::default(), device);
        app.ingest(sensors.sample());
        app.ingest(sensors.sample());
        app
    }

    fn render(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|f| draw(f, app))
            .expect("render without panic or backend errors");
        terminal.backend().buffer().clone()
    }

    fn populated_app() -> App {
        const GIB: u64 = 1_048_576;
        let procs = vec![
            Proc {
                pid: 1234,
                user: "jetson".into(),
                cpu_pct: 98.0,
                rss_kib: GIB * 3 / 4,
                swap_kib: 0,
                gpu: GpuUse::Yes,
                cmd: "llama-server --model qwen.gguf --n-gpu-layers 99".into(),
            },
            Proc {
                pid: 2345,
                user: "jetson".into(),
                cpu_pct: 12.5,
                rss_kib: GIB / 4,
                swap_kib: 1024,
                gpu: GpuUse::No,
                cmd: "cpu-worker --threads 2".into(),
            },
            Proc {
                pid: 3456,
                user: "root".into(),
                cpu_pct: 0.5,
                rss_kib: GIB / 8,
                swap_kib: 0,
                gpu: GpuUse::Unknown,
                cmd: "root-worker --private".into(),
            },
        ];
        let workload = Workload {
            path: "/tmp/model-workload.log".into(),
            inf_per_s: 31.5,
            p50_ms: 4000.0,
            p95_ms: 8000.0,
            p99_ms: 9500.0,
            samples_in_window: 12,
            window_s: 60.0,
            tok_ms_p50: Some(31.7),
            tok_ms_p95: Some(35.2),
            deadline_ms: Some(7000.0),
            misses_window: 2,
            misses_total: 7,
            total_lines: 120,
            bad_lines: 1,
            engine: "llama.cpp".into(),
            batch: 128,
            stages: vec![
                ("pre".into(), 2.0),
                ("inf".into(), 31.0),
                ("post".into(), 1.0),
            ],
            inf_per_watt: Some(5.625),
            idle_s: 0.5,
            typical_gap_s: 0.5,
        };
        let snap = Snapshot {
            cpus: (0..6)
                .map(|id| Cpu {
                    id,
                    online: true,
                    util_pct: 20.0 + id as f64,
                    cur_mhz: 1728,
                    min_mhz: 1728,
                    max_mhz: 1728,
                    governor: "userspace".into(),
                    ..Default::default()
                })
                .collect(),
            cpu_total_pct: 22.5,
            load_avg: [1.2, 1.0, 0.8],
            gpu: Some(Gpu {
                load_pct: 65.0,
                cur_mhz: 408,
                min_mhz: 612,
                max_mhz: 612,
                governor: "userspace".into(),
                steps_mhz: vec![306, 408, 612, 1020],
            }),
            mem: Mem {
                total_kib: 8 * GIB,
                avail_kib: GIB * 11 / 2,
                cached_kib: GIB,
                swap_total_kib: 12 * GIB,
                swap_free_kib: 12 * GIB - GIB * 3 / 8,
            },
            swap: SwapStats {
                in_pages_s: 256.0,
                out_pages_s: 512.0,
                majflt_s: 3.0,
                zram_used_kib: GIB / 4,
                zram_total_kib: 4 * GIB,
                file_used_kib: GIB / 8,
                file_total_kib: 8 * GIB,
            },
            rails: [("VDD_IN", 5.6), ("VDD_CPU_GPU_CV", 3.1), ("VDD_SOC", 1.0)]
                .into_iter()
                .map(|(name, watts)| Rail {
                    name: name.into(),
                    watts,
                    volts: 5.0,
                    amps: watts / 5.0,
                })
                .collect(),
            temps: ["cpu", "gpu", "soc", "tj"]
                .into_iter()
                .map(|name| Temp {
                    name: name.into(),
                    celsius: 55.0,
                    trip_sw: Some(99.0),
                    trip_hw: Some(104.5),
                })
                .collect(),
            fan: Some(Fan {
                pwm: 66,
                pwm_pct: 25.9,
                rpm: Some(1360),
            }),
            clocks: ClockState::Locked {
                cpu_mhz: 1728,
                gpu_mhz: 612,
            },
            oc_events: vec![0, 2, 0],
            power_mode: Some(PowerMode {
                id: 0,
                name: "15W".into(),
            }),
            watched: Some(Watched {
                target: "llama-server".into(),
                proc: Some(procs[0].clone()),
                restarts: 2,
                mem_est_kib: Some((GIB * 3 / 2) as i64),
            }),
            procs,
            workload: Some(workload),
            self_cpu_pct: 1.0,
            self_rss_mib: 1.9,
            ..Default::default()
        };
        let mut app = App::new(
            Expected::default(),
            DeviceInfo {
                model: "Jetson Orin Nano".into(),
                l4t: "L4T R36.5.2".into(),
            },
        );
        for i in 0..24 {
            let mut sample = snap.clone();
            if i == 10 {
                sample.watched.as_mut().unwrap().mem_est_kib = None;
            }
            app.ingest(sample);
        }
        app.mem_used_start_kib = Some(GIB);
        app.throttle = Throttle {
            cpu: DropStats::default(),
            gpu: DropStats {
                drops: 3,
                below_s: 1.5,
                longest_s: 1.0,
                active: true,
                current_s: 0.5,
            },
            oc_rise: 2,
        };
        app
    }

    fn text_cell<'a>(buffer: &'a Buffer, needle: &str) -> &'a ratatui::buffer::Cell {
        &buffer[text_position(buffer, needle)]
    }

    fn text_position(buffer: &Buffer, needle: &str) -> (u16, u16) {
        for y in buffer.area.y..buffer.area.bottom() {
            let text = (buffer.area.x..buffer.area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if let Some(index) = text.find(needle) {
                let x = Line::from(&text[..index]).width() as u16;
                return (buffer.area.x + x, y);
            }
        }
        panic!("missing {needle}\n{}", screen(buffer));
    }

    fn screen(buffer: &Buffer) -> String {
        (buffer.area.y..buffer.area.bottom())
            .map(|y| {
                (buffer.area.x..buffer.area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn memory_swap_tiers_have_gradient_bars_at_required_sizes() {
        let app = populated_app();
        for (width, height) in SIZES {
            let buffer = render(&app, width, height);
            let (ram_x, ram_y) = text_position(&buffer, "USED 2.50/8.00 GiB");
            let ram_width = (ram_x..buffer.area.right())
                .take_while(|&x| "░▏▎▍▌▋▊▉█".contains(buffer[(x, ram_y + 1)].symbol()))
                .count();
            assert_eq!(ram_width, 30);
            for (label, usage, prefix, color) in [
                ("zram", "0.25/4.00 GiB", "█▉", theme::MUTED),
                ("NVMe swp", "0.12/8.00 GiB", "▍", theme::CRITICAL),
            ] {
                let (x, y) = text_position(&buffer, label);
                let (usage_x, usage_y) = text_position(&buffer, usage);
                assert_eq!(usage_y, y);
                let bar: Vec<_> = (x..usage_x)
                    .map(|x| &buffer[(x, y)])
                    .skip_while(|cell| !"░▏▎▍▌▋▊▉█".contains(cell.symbol()))
                    .take_while(|cell| "░▏▎▍▌▋▊▉█".contains(cell.symbol()))
                    .collect();
                assert_eq!(bar.len(), ram_width, "{width}×{height} {label}");
                assert_eq!(
                    bar.iter().map(|cell| cell.symbol()).collect::<String>(),
                    format!("{prefix}{}", "░".repeat(ram_width - prefix.chars().count()))
                );
                assert_eq!(bar[0].fg, theme::gradient_color(0.0));
                assert_eq!(buffer[(x, y)].fg, color);
                assert_eq!(text_cell(&buffer, usage).fg, theme::MUTED);
            }
        }
    }

    #[test]
    fn memory_swap_traffic_uses_history_and_preserves_live_badge() {
        let mut app = populated_app();
        app.snap.swap.out_pages_s = 0.0;
        for value in &mut app.hist.swap_out_pages_s.data {
            *value = 0.0;
        }
        for (width, height) in [(160, 45), (120, 40)] {
            let buffer = render(&app, width, height);
            let text = screen(&buffer);
            assert_eq!(text.matches("swap traffic: none").count(), 1);
            assert_eq!(text_cell(&buffer, "swap traffic: none").fg, theme::MUTED);
            assert!(!text.contains("SWAP OUT / pages/s"));
            assert!(!text.contains("SWAPPING"));
            assert!(text.contains("swap in 1.00 / out 0.00 MiB/s · majflt 3.0/s"));
        }
        app.snap.swap.out_pages_s = 512.0;
        let buffer = render(&app, 160, 45);
        assert!(screen(&buffer).contains("swap traffic: none"));
        assert_eq!(text_cell(&buffer, "SWAPPING").fg, theme::WARN);

        app.snap.swap.out_pages_s = 0.0;
        app.hist.swap_out_pages_s.push(16.0);
        let buffer = render(&app, 160, 45);
        let text = screen(&buffer);
        let (x, y) = text_position(&buffer, "SWAP OUT / pages/s");
        assert!((x + 21..x + 50)
            .any(|x| buffer[(x, y)].fg == theme::WARN && buffer[(x, y)].symbol() != " "));
        assert!(!text.contains("swap traffic: none"));
        assert!(!text.contains("SWAPPING"));
    }

    #[test]
    fn memory_swap_idle_bars_and_absent_tiers() {
        let mut app = populated_app();
        app.snap.swap.zram_used_kib = 0;
        app.snap.swap.file_used_kib = 0;
        for (zram, file) in [(true, true), (false, true), (true, false), (false, false)] {
            app.snap.swap.zram_total_kib = if zram { 4 * 1_048_576 } else { 0 };
            app.snap.swap.file_total_kib = if file { 8 * 1_048_576 } else { 0 };
            let mut terminal = Terminal::new(TestBackend::new(100, 9)).unwrap();
            terminal.draw(|f| memory::draw(f, f.area(), &app)).unwrap();
            let buffer = terminal.backend().buffer();
            let text = screen(buffer);
            let (ram_x, ram_y) = text_position(buffer, "USED 2.50/8.00 GiB");
            let mut next_y = ram_y + 2;
            for (label, present, usage) in [
                ("zram", zram, "0.00/4.00 GiB · compressed in RAM"),
                ("NVMe swp", file, "0.00/8.00 GiB · slow"),
            ] {
                assert_eq!(text.contains(label), present);
                if present {
                    assert_eq!(text_position(buffer, label), (ram_x, next_y));
                    assert_eq!(text_position(buffer, usage).1, next_y);
                    assert_eq!(text_cell(buffer, label).fg, theme::MUTED);
                    assert!(text.contains(&format!("{} {usage}", "░".repeat(30))));
                    next_y += 1;
                }
            }
            assert_eq!(text_position(buffer, "swap in").1, next_y);
        }
    }

    #[test]
    fn memory_swap_bars_take_priority_when_height_is_limited() {
        let mut app = populated_app();
        for active_history in [true, false] {
            if !active_history {
                app.hist.swap_out_pages_s.data.clear();
            }
            for height in 4..=9 {
                let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
                terminal.draw(|f| memory::draw(f, f.area(), &app)).unwrap();
                let text = screen(terminal.backend().buffer());
                assert!(text.contains("USED 2.50/8.00 GiB"));
                assert!(text.contains("Δ since start +1.50 GiB"));
                assert!(text.contains("avail 5.50 · cache 1.00 GiB"));
                for (needle, min_height) in [
                    ("zram", 5),
                    ("NVMe swp", 6),
                    ("swap in 1.00 / out 2.00 MiB/s · majflt 3.0/s", 7),
                    (
                        if active_history {
                            "SWAP OUT / pages/s"
                        } else {
                            "swap traffic: none"
                        },
                        8,
                    ),
                    ("Used = total − available", 9),
                ] {
                    assert_eq!(
                        text.contains(needle),
                        height >= min_height,
                        "height {height}, {needle}\n{text}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_tab_renders_fixture_at_required_sizes() {
        let mut app = fixture_app();
        assert_eq!(app.hist.cpu_total.data.len(), 2);
        for (width, height) in SIZES {
            for tab in TABS {
                app.tab = tab;
                let buffer = render(&app, width, height);
                let text = screen(&buffer);
                assert!(
                    text.contains("JETSON MONITOR"),
                    "{width}×{height} {tab:?}\n{text}"
                );
                assert!(text.contains("1-5 tabs  b bench  Space freeze  +/- rate  q quit"));
                assert!(text.contains("DATA AGE"));
                let required = match tab {
                    Tab::Overview => "VDD_CPU_GPU_CV",
                    Tab::Power => "live estimate — not campaign data",
                    Tab::Thermal => "THERMAL ZONES",
                    Tab::Processes => "PROCESSES · sort MEM (RSS)",
                    Tab::Benchmark => "CONDITIONS NOT OK",
                };
                assert!(
                    text.contains(required),
                    "missing {required}, {width}×{height} {tab:?}\n{text}"
                );
                assert_eq!(buffer.area, Rect::new(0, 0, width, height));
                assert!(
                    buffer
                        .content
                        .iter()
                        .all(|c| matches!(c.fg, Color::Rgb(..)) && matches!(c.bg, Color::Rgb(..))),
                    "{width}×{height} {tab:?}: {:?}",
                    buffer
                        .content
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| !matches!(c.fg, Color::Rgb(..))
                            || !matches!(c.bg, Color::Rgb(..)))
                        .take(5)
                        .collect::<Vec<_>>()
                );
                if std::env::var("JMON_UI_PREVIEW").ok().as_deref()
                    == Some(format!("{width}x{height}").as_str())
                    && matches!(tab, Tab::Overview | Tab::Benchmark)
                {
                    println!("\n{tab:?} {width}×{height}\n{text}\n");
                }
            }
        }
    }

    #[test]
    fn every_tab_renders_populated_model_runs_at_required_sizes() {
        let base = populated_app();
        for watched_state in 0..4 {
            for tokens in [false, true] {
                for bench in [false, true] {
                    let mut app = base.clone();
                    match watched_state {
                        0 => app.snap.watched = None,
                        1 => app.snap.watched.as_mut().unwrap().mem_est_kib = None,
                        2 => {}
                        _ => {
                            let w = app.snap.watched.as_mut().unwrap();
                            w.proc = None;
                            w.mem_est_kib = None;
                        }
                    }
                    if !tokens {
                        let w = app.snap.workload.as_mut().unwrap();
                        w.tok_ms_p50 = None;
                        w.tok_ms_p95 = None;
                    }
                    app.bench = bench;
                    if bench {
                        app.snap.procs.clear();
                    }
                    for (width, height) in SIZES {
                        for tab in TABS {
                            app.tab = tab;
                            let buffer = render(&app, width, height);
                            let text = screen(&buffer);
                            let require = |needle: &str| {
                                assert!(text.contains(needle), "missing {needle}, {width}×{height} {tab:?}, watch {watched_state}, tokens {tokens}, bench {bench}\n{text}")
                            };
                            require("JETSON MONITOR");
                            require("DATA AGE");
                            assert_eq!(buffer.area, Rect::new(0, 0, width, height));
                            assert!(buffer.content.iter().all(|c| matches!(c.fg, Color::Rgb(..))
                                && matches!(c.bg, Color::Rgb(..))));
                            assert_eq!(text.contains("s sort"), tab == Tab::Processes);
                            assert!(!text.contains("GPU%") && !text.contains("joules"));
                            if matches!(tab, Tab::Overview | Tab::Processes) {
                                assert_eq!(text.contains("WATCHED MODEL"), watched_state != 0);
                                if watched_state > 0 {
                                    require("RSS (CPU-visible)");
                                    require("system Δ since pid appeared · estimate");
                                    require(if watched_state == 2 {
                                        "MODEL MEM ≈ 1.50 GiB"
                                    } else {
                                        "n/a — start jmon before the model"
                                    });
                                    if watched_state == 3 {
                                        require("not running");
                                    }
                                }
                            }
                            if matches!(tab, Tab::Overview | Tab::Power) {
                                require("WORKLOAD · last 60 s");
                                require(if tokens { "31.5 tok/s" } else { "31.5 inf/s" });
                                require(if tokens {
                                    "31.7 ms/token p50"
                                } else {
                                    "8000.0 ms p95"
                                });
                                require("VDD_IN");
                            }
                            match tab {
                                Tab::Overview => {
                                    for s in [
                                        "Δ since start +1.50 GiB",
                                        "SWAPPING",
                                        "zram",
                                        "0.25/4.00 GiB",
                                        "NVMe swp",
                                        "0.12/8.00 GiB",
                                        "in 1.00 / out 2.00 MiB/s",
                                        "majflt 3.0/s",
                                        "VDD_CPU_GPU_CV",
                                    ] {
                                        require(s);
                                    }
                                    assert_eq!(text_cell(&buffer, "SWAPPING").fg, theme::WARN);
                                    assert_eq!(text_cell(&buffer, "NVMe swp").fg, theme::CRITICAL);
                                }
                                Tab::Power => {
                                    for s in [
                                        "inf/s/W — board-level live estimate (includes idle)",
                                        "misses 2 window / 7 total",
                                        "STAGES mean ms:",
                                        "lines 120 · bad 1",
                                        "batch 128",
                                        "12 samples",
                                    ] {
                                        require(s);
                                    }
                                    if tokens {
                                        require("request p50 4000.0 / p95 8000.0 / p99 9500.0 ms");
                                    }
                                    if width >= 120 {
                                        require("THROUGHPUT /");
                                        require("REQUEST p95 / ms");
                                    }
                                }
                                Tab::Thermal | Tab::Benchmark => {
                                    for s in [
                                        "CLOCK DROPS (locked)",
                                        "GPU 3 / 1.5 s / longest 1.0 s",
                                        "OC +2",
                                        "sampled 2 Hz",
                                    ] {
                                        require(s);
                                    }
                                    assert_eq!(text_cell(&buffer, "GPU 3 /").fg, theme::WARN);
                                    if tab == Tab::Benchmark {
                                        for check in &app.checks {
                                            require(check.name);
                                        }
                                        require("VDD_IN");
                                    }
                                }
                                Tab::Processes => {
                                    require(
                                        "MEM = CPU-visible RSS; GPU buffers may not be included",
                                    );
                                    require("per-process GPU % is not available on Orin");
                                    if bench {
                                        require("process table off in bench mode (key b)");
                                    } else {
                                        for s in [
                                            "PID",
                                            "USER",
                                            "GPU",
                                            "CPU%",
                                            "MEM (RSS)",
                                            "SWAP",
                                            "COMMAND",
                                            "1234",
                                            "2345",
                                            "3456",
                                            "cpu-worker",
                                            "root-worker",
                                        ] {
                                            require(s);
                                        }
                                        if watched_state == 1 || watched_state == 2 {
                                            assert_eq!(
                                                text_cell(&buffer, "llama-server -").bg,
                                                theme::BORDER
                                            );
                                        }
                                    }
                                }
                            }
                            if watched_state == 2
                                && tokens
                                && !bench
                                && std::env::var("JMON_UI_PREVIEW").ok().as_deref()
                                    == Some(format!("{width}x{height}").as_str())
                            {
                                println!("\n{tab:?} {width}×{height}\n{text}\n");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn gradient_colors_match_the_three_stops() {
        assert_eq!(theme::gradient_color(0.0), Color::Rgb(118, 185, 0));
        assert_eq!(theme::gradient_color(0.6), Color::Rgb(245, 158, 11));
        assert_eq!(theme::gradient_color(1.0), Color::Rgb(239, 68, 68));
    }

    #[test]
    fn process_sort_gpu_markers_and_watched_highlight_are_explicit() {
        let mut app = populated_app();
        app.tab = Tab::Processes;
        app.proc_sort = ProcSort::Cpu;
        app.snap.procs[1].cpu_pct = 120.0;
        app.ingest(app.snap.clone());
        for (width, height) in SIZES {
            let buffer = render(&app, width, height);
            let text = screen(&buffer);
            assert!(text.contains("sort CPU%"));
            let (gpu_x, header_y) = text_position(&buffer, "GPU CPU%");
            let (_, no_y) = text_position(&buffer, "cpu-worker");
            let (_, yes_y) = text_position(&buffer, "llama-server -");
            let (_, unknown_y) = text_position(&buffer, "root-worker");
            assert_eq!(no_y, header_y + 1);
            assert_eq!(yes_y, no_y + 1);
            assert_eq!(unknown_y, yes_y + 1);
            assert_eq!(buffer[(gpu_x, no_y)].symbol(), " ");
            assert_eq!(buffer[(gpu_x, yes_y)].symbol(), "●");
            assert_eq!(buffer[(gpu_x, yes_y)].fg, theme::ACCENT);
            assert_eq!(buffer[(gpu_x, unknown_y)].symbol(), "?");
            assert_eq!(buffer[(gpu_x, unknown_y)].fg, theme::MUTED);
            assert_eq!(text_cell(&buffer, "llama-server -").bg, theme::BORDER);
            assert_eq!(text_cell(&buffer, "PROCESSES ·").fg, theme::MUTED);
            let (rss_x, _) = text_position(&buffer, "MEM (RSS)             ");
            assert_eq!(buffer[(rss_x + 10, yes_y)].symbol(), "█");
            assert_eq!(buffer[(rss_x + 10, yes_y)].fg, theme::ACCENT);
        }
    }

    #[test]
    fn workload_waiting_stalled_and_optional_fields_render_at_required_sizes() {
        for waiting in [true, false] {
            let mut app = populated_app();
            let workload = app.snap.workload.as_mut().unwrap();
            workload.deadline_ms = None;
            workload.inf_per_watt = None;
            workload.stages.clear();
            workload.idle_s = 8.5;
            if waiting {
                workload.total_lines = 0;
                workload.samples_in_window = 0;
                workload.tok_ms_p50 = None;
                workload.tok_ms_p95 = None;
            }
            for (width, height) in SIZES {
                for tab in [Tab::Overview, Tab::Power] {
                    app.tab = tab;
                    let buffer = render(&app, width, height);
                    let text = screen(&buffer);
                    let message = if waiting {
                        "waiting for first line in /tmp/model-workload.log"
                    } else {
                        "WORKLOAD STALLED (no line for 8.5 s)"
                    };
                    assert!(text.contains(message), "{width}×{height} {tab:?}\n{text}");
                    assert_eq!(
                        text_cell(&buffer, message).fg,
                        if waiting { theme::MUTED } else { theme::WARN }
                    );
                    assert!(!text.contains("deadline") && !text.contains("misses"));
                    assert!(!text.contains("STAGES mean"));
                    if waiting {
                        assert!(text.contains("n/a inf/s") && text.contains("n/a ms p95"));
                    }
                    if tab == Tab::Power {
                        assert!(text
                            .contains("n/a inf/s/W — board-level live estimate (includes idle)"));
                    }
                }
            }
        }
    }

    #[test]
    fn signed_model_estimates_and_dynamic_clock_note_stay_visible() {
        let mut app = populated_app();
        app.snap.watched.as_mut().unwrap().mem_est_kib = Some(-524_288);
        app.hist.model_mem_gib.push(f64::NAN);
        app.hist.model_mem_gib.push(-0.5);
        app.snap.clocks = ClockState::Dynamic;
        for (width, height) in SIZES {
            for tab in [Tab::Overview, Tab::Processes] {
                app.tab = tab;
                let buffer = render(&app, width, height);
                let text = screen(&buffer);
                assert!(text.contains("MODEL MEM ≈ -0.50 GiB · estimate"));
                assert!(text.contains("RSS (CPU-visible) 0.75 GiB"));
                assert_eq!(text_cell(&buffer, "MODEL MEM").fg, theme::ACCENT_HI);
                assert_eq!(text_cell(&buffer, "RSS (CPU-visible)").fg, theme::MUTED);
                if width >= 120 {
                    assert!(text.contains("MODEL ≈ / SYSTEM · GiB"));
                }
            }
            for tab in [Tab::Thermal, Tab::Benchmark] {
                app.tab = tab;
                let buffer = render(&app, width, height);
                assert!(screen(&buffer).contains("counting only while clocks are locked"));
                assert_eq!(text_cell(&buffer, "counting only").fg, theme::MUTED);
                if tab == Tab::Benchmark {
                    for check in &app.checks {
                        assert!(screen(&buffer).contains(check.name));
                    }
                }
            }
        }
    }

    #[test]
    fn model_memory_chart_preserves_gaps_and_negative_estimates() {
        let mut model = Series::new(6);
        let mut system = Series::new(6);
        for value in [-0.5, -0.5, f64::NAN, f64::NAN, 1.5, 1.5] {
            model.push(value);
            system.push(2.5);
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|f| {
                chart::histories(
                    f,
                    f.area(),
                    &[(&system, theme::MUTED), (&model, theme::ACCENT_HI)],
                    "MEMORY / GiB",
                    2.5,
                    true,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(screen(buffer).contains("-0.5"));
        assert!(screen(buffer).contains("2.5"));
        let columns: Vec<usize> = buffer
            .content
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.fg == theme::ACCENT_HI)
            .map(|(i, _)| i % 80)
            .collect();
        assert!(columns.iter().any(|x| *x < 25));
        assert!(columns.iter().any(|x| *x > 60));
        assert!(
            columns.iter().all(|x| !(30..55).contains(x)),
            "missing samples must not be connected"
        );
    }

    #[test]
    fn gradient_uses_track_position_and_eighth_cells() {
        let area = Rect::new(2, 1, 21, 1);
        let mut low = Buffer::empty(Rect::new(0, 0, 25, 3));
        let mut high = low.clone();
        gauge::GradientBar { ratio: 0.3 }.render(area, &mut low);
        gauge::GradientBar { ratio: 1.0 }.render(area, &mut high);
        for i in 0..6 {
            assert_eq!(low[(2 + i, 1)].fg, high[(2 + i, 1)].fg);
            assert_eq!(
                low[(2 + i, 1)].fg,
                theme::gradient_color(f64::from(i) / 20.0)
            );
        }
        assert_eq!(low[(8, 1)].symbol(), "▎");
        assert_eq!(low[(9, 1)].symbol(), "░");
        assert_eq!(low[(9, 1)].fg, theme::BORDER);
        assert_eq!(high[(14, 1)].fg, theme::WARN);
        assert_eq!(high[(22, 1)].fg, theme::CRITICAL);
        assert_eq!(low[(1, 1)].symbol(), " ");
        for ratio in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 2.0] {
            gauge::GradientBar { ratio }.render(Rect::new(24, 2, 20, 1), &mut low);
            gauge::GradientBar { ratio }.render(Rect::new(0, 0, 0, 0), &mut low);
        }
    }

    #[test]
    fn fan_art_preserves_frames_and_alarm_priority() {
        assert_eq!(fan::FRAMES.len(), crate::app::FAN_FRAMES);
        assert_eq!(fan::FRAMES.iter().collect::<HashSet<_>>().len(), 10);
        for frame in fan::FRAMES {
            assert!(frame.iter().all(|line| line.chars().count() == 15));
            assert_eq!(frame[3].chars().nth(7), Some('●'));
        }
        assert!(fan::ROLE_MASK.iter().all(|row| row.len() == 15));
        let mut app = fixture_app();
        app.snap.fan = Some(Fan {
            pwm: 66,
            pwm_pct: 25.9,
            rpm: Some(0),
        });
        app.fan_frame = 9;
        app.bench = true;
        let mut buffer = Buffer::empty(Rect::new(0, 0, 15, 7));
        fan::FanArt { app: &app }.render(buffer.area, &mut buffer);
        assert_eq!(buffer[(7, 3)].fg, theme::CRITICAL);
        assert_eq!(buffer[(3, 0)].fg, theme::BORDER);
        assert_eq!(screen(&buffer), fan::FRAMES[0].join("\n"));
        for (width, height) in SIZES {
            for tab in [Tab::Overview, Tab::Thermal, Tab::Benchmark] {
                app.tab = tab;
                let text = screen(&render(&app, width, height));
                assert!(
                    text.contains("FAN STALLED"),
                    "{width}×{height} {tab:?}\n{text}"
                );
                assert!(text.contains("anim paused (bench)"));
            }
        }
        app.snap.fan.as_mut().unwrap().rpm = None;
        let text = screen(&render(&app, 80, 24));
        assert!(text.contains("rpm n/a"));
        assert!(!text.contains("FAN STALLED"));
    }

    #[test]
    fn frozen_sampling_and_condition_states_are_explicit() {
        let mut app = fixture_app();
        app.frozen = true;
        app.sample_ms = 4000;
        for (width, height) in SIZES {
            let text = screen(&render(&app, width, height));
            assert!(text.contains("FROZEN — sampling continues"));
            assert!(text.contains("SAMPLE 0.25 Hz"));
        }
        app.tab = Tab::Benchmark;
        for (state, label) in [
            (CheckState::Ok, "OK"),
            (CheckState::Bad, "NOT OK"),
            (CheckState::Unknown, "UNKNOWN"),
        ] {
            app.checks = vec![Check {
                name: "Example",
                expected: "target".into(),
                observed: "reading".into(),
                state,
            }];
            let text = screen(&render(&app, 160, 45));
            assert!(text.contains(&format!("CONDITIONS {label}")));
        }
        app.events.insert(
            0,
            crate::model::Event {
                at_s: 125,
                level: crate::model::Level::Warn,
                msg: "Timing marker".into(),
            },
        );
        assert!(screen(&render(&app, 160, 45)).contains("T+02:05"));
        app.has_snapshot = false;
        for tab in TABS {
            app.tab = tab;
            let text = screen(&render(&app, 80, 24));
            assert!(text.contains("sampling…"));
            assert!(!text.contains("CONDITIONS"));
        }
    }

    #[test]
    fn missing_sensors_and_tiny_resize_never_panic() {
        let mut app = App::new(Expected::default(), DeviceInfo::default());
        app.ingest(Snapshot::default());
        app.sample_ms = 0;
        app.fan_frame = usize::MAX;
        for (width, height) in SIZES
            .into_iter()
            .chain([(1, 1), (12, 4), (40, 12), (80, 10)])
        {
            for tab in TABS {
                app.tab = tab;
                render(&app, width, height);
            }
        }
        let mut populated = populated_app();
        populated.snap.procs[0].cmd = "推論 --model 長いモデル名 ".repeat(50);
        populated.snap.watched.as_mut().unwrap().target = "推論モデル".repeat(40);
        populated.snap.workload.as_mut().unwrap().engine = "engine".repeat(40);
        populated.snap.workload.as_mut().unwrap().stages =
            vec![("zero".into(), 0.0), ("missing".into(), f64::NAN)];
        for (width, height) in SIZES
            .into_iter()
            .chain([(1, 1), (12, 4), (40, 12), (80, 10)])
        {
            for tab in TABS {
                populated.tab = tab;
                let buffer = render(&populated, width, height);
                if width >= 80 && height >= 24 {
                    let text = screen(&buffer);
                    if matches!(tab, Tab::Overview | Tab::Processes) {
                        assert!(text.contains("pid 1234") && text.contains("restarts 2"));
                    }
                    if tab == Tab::Power {
                        assert!(text.contains("batch 128") && text.contains("12 samples"));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod lead_fan_layout {
    // lead: the fan art must stay visible in a ~116-col window (side column < 52 cols wide).
    use super::*;
    use crate::{
        app::{DeviceInfo, Expected},
        sensors::Sensors,
    };
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn fan_art_in_narrow_window() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture");
        let mut s = Sensors::discover(&root);
        let mut app = App::new(Expected::default(), DeviceInfo::default());
        app.ingest(s.sample());
        let mut t = Terminal::new(TestBackend::new(116, 50)).unwrap();
        t.draw(|f| draw(f, &app)).unwrap();
        let buf = t.backend().buffer().clone();
        let rows: Vec<String> = (0..50)
            .map(|y| (0..116).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect();
        let title = rows
            .iter()
            .position(|r| r.contains("FAN / AUTOMATIC"))
            .expect("fan panel");
        let art = fan::FRAMES[0][0].trim(); // shroud top row, same in every frame
        eprintln!("{}", rows[title..(title + 10).min(50)].join("\n"));
        assert!(
            rows[title + 1..(title + 9).min(50)]
                .iter()
                .any(|r| r.contains(art)),
            "fan art missing below title"
        );
    }
}
