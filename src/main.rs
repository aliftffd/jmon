//! jmon — NVIDIA Jetson Orin Nano hardware monitor.
//! Logic: model.rs (contract), sensors.rs (sysfs), app.rs (state/checks). UI: ui/ (ratatui).

mod app;
mod model;
mod sensors;
mod ui;
mod workload;

use app::{App, DeviceInfo, Expected};
use crossterm::event::{self, Event as TermEvent, KeyCode, KeyEventKind};
use model::{Level, Tab};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "jmon [--root DIR] [--expected FILE] [--bench] [--watch NAME | --pid N] [--workload FILE [--deadline-ms MS]]
  --root DIR         read sysfs/procfs under DIR (captured fixture) instead of /
  --expected FILE    benchmark targets (default: first of ./expected.conf, ~/.config/jmon/expected.conf,
                     /etc/jmon/expected.conf, else built-in 15W/1728/612)
  --bench            start in benchmark preset (1 Hz, no process table, fan animation paused)
  --watch NAME       follow the process whose command line contains NAME (re-found after restarts)
  --pid N            follow process N
  --workload FILE    follow the model's key=value log (lat_ms required; see src/workload.rs)
  --deadline-ms MS   count workload latencies above MS as deadline misses
  --window SECONDS   workload rolling window (default 10; 30-60 for LLMs)";

fn main() -> std::io::Result<()> {
    let mut root = PathBuf::from("/");
    let mut expected_path: Option<PathBuf> = None;
    let mut bench = false;
    let mut watch = None;
    let mut workload_path: Option<PathBuf> = None;
    let mut deadline_ms: Option<f64> = None;
    let mut window = workload::DEFAULT_WINDOW;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = args.next().map(PathBuf::from).unwrap_or(root),
            "--expected" => expected_path = args.next().map(PathBuf::from),
            "--bench" => bench = true,
            "--watch" => watch = args.next().map(sensors::WatchTarget::Name),
            "--pid" => {
                watch = args
                    .next()
                    .and_then(|p| p.parse().ok())
                    .map(sensors::WatchTarget::Pid)
            }
            "--workload" => workload_path = args.next().map(PathBuf::from),
            "--deadline-ms" => deadline_ms = args.next().and_then(|d| d.parse().ok()),
            "--window" => {
                window = args
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|s| *s >= 1.0)
                    .map(Duration::from_secs_f64)
                    .unwrap_or(window)
            }
            _ => {
                println!("{USAGE}");
                return Ok(());
            }
        }
    }

    let expected = expected_path
        .or_else(|| config_candidates().into_iter().find(|p| p.is_file()))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| Expected::parse(&t))
        .unwrap_or_default();
    let mut sensors = sensors::Sensors::discover(&root);
    if let Some(w) = watch {
        sensors.set_watch(w);
    }
    let mut follower = workload_path.map(|p| workload::Follower::new(p, deadline_ms, window));
    let device = DeviceInfo {
        model: sensors.device_model(),
        l4t: sensors.l4t_release(),
    };
    let mut app = App::new(expected, device);
    app.bench = bench;
    app.note(
        Level::Info,
        if bench {
            "Bench preset selected"
        } else {
            "Monitor started"
        },
    );

    // Sampler thread: owns the sensors, period shared via atomic so +/- and bench mode retune it live.
    let period_ms = Arc::new(AtomicU64::new(if bench { 1000 } else { 500 }));
    let bench_flag = Arc::new(AtomicBool::new(bench));
    let (tx, rx) = mpsc::channel();
    {
        let period_ms = period_ms.clone();
        let bench_flag = bench_flag.clone();
        std::thread::spawn(move || loop {
            let t = Instant::now();
            let p = period_ms.load(Ordering::Relaxed);
            sensors.set_scan_every((5000 / p.max(1)) as u32); // jtop/tegrastats check ≈ every 5 s
            sensors.proc_table = !bench_flag.load(Ordering::Relaxed);
            let mut snap = sensors.sample();
            if let Some(f) = follower.as_mut() {
                snap.workload = Some(
                    f.poll(
                        snap.rails
                            .iter()
                            .find(|r| r.name == "VDD_IN")
                            .map(|r| r.watts),
                    ),
                );
            }
            if tx.send((snap, Instant::now())).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(p).saturating_sub(t.elapsed()));
        });
    }

    let mut terminal = ratatui::init();
    let mut last_sample = Instant::now();
    let mut frozen_view: Option<App> = None;
    let result = loop {
        while let Ok((snap, at)) = rx.try_recv() {
            app.ingest(snap);
            last_sample = at;
        }
        let now = Instant::now();
        app.sample_ms = period_ms.load(Ordering::Relaxed);
        app.data_age_ms = now.duration_since(last_sample).as_millis() as u64;
        app.tick_fan(now);
        // Freeze = keep drawing a copy taken at freeze time; `app` keeps ingesting + checking.
        if !app.frozen {
            frozen_view = None;
        } else if frozen_view.is_none() {
            frozen_view = Some(app.clone());
        }
        let view = frozen_view.as_ref().unwrap_or(&app);
        if let Err(e) = terminal.draw(|f| ui::draw(f, view)) {
            break Err(e);
        }
        // ~10 fps while the fan animates, otherwise just wake for new samples.
        let wait = if app.bench {
            Duration::from_millis(app.sample_ms)
        } else {
            Duration::from_millis(100)
        };
        match event::poll(wait) {
            Ok(true) => {
                if let Ok(TermEvent::Key(k)) = event::read() {
                    if k.kind == KeyEventKind::Press {
                        handle_key(&mut app, k.code, &period_ms);
                        bench_flag.store(app.bench, Ordering::Relaxed);
                        if let Some(v) = frozen_view.as_mut() {
                            v.tab = app.tab;
                        }
                    }
                }
            }
            Ok(false) => {}
            Err(e) => break Err(e),
        }
        if app.quit {
            break Ok(());
        }
    };
    ratatui::restore();
    result
}

/// Config search order when --expected isn't given (installed via cargo/deb, not run from the repo).
fn config_candidates() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("expected.conf")];
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(dir) = xdg {
        v.push(dir.join("jmon/expected.conf"));
    }
    v.push(PathBuf::from("/etc/jmon/expected.conf"));
    v
}

fn handle_key(app: &mut App, key: KeyCode, period_ms: &AtomicU64) {
    match key {
        KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
        KeyCode::Char('1') => app.tab = Tab::Overview,
        KeyCode::Char('2') => app.tab = Tab::Power,
        KeyCode::Char('3') => app.tab = Tab::Thermal,
        KeyCode::Char('4') => app.tab = Tab::Processes,
        KeyCode::Char('5') => app.tab = Tab::Benchmark,
        KeyCode::Char(' ') => app.frozen = !app.frozen,
        KeyCode::Char('s') => {
            app.proc_sort = if app.proc_sort == app::ProcSort::Mem {
                app::ProcSort::Cpu
            } else {
                app::ProcSort::Mem
            };
            if app.proc_sort == app::ProcSort::Cpu {
                app.snap
                    .procs
                    .sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
            } else {
                app.snap.procs.sort_by_key(|p| std::cmp::Reverse(p.rss_kib));
            }
        }
        KeyCode::Char('b') => {
            app.bench = !app.bench;
            period_ms.store(if app.bench { 1000 } else { 500 }, Ordering::Relaxed);
            if app.bench {
                app.tab = Tab::Benchmark;
            }
            app.note(
                Level::Info,
                if app.bench {
                    "Bench preset selected"
                } else {
                    "Bench preset off"
                },
            );
        }
        KeyCode::Char('+') => {
            let p = period_ms.load(Ordering::Relaxed);
            period_ms.store((p / 2).max(250), Ordering::Relaxed);
        }
        KeyCode::Char('-') => {
            let p = period_ms.load(Ordering::Relaxed);
            period_ms.store((p * 2).min(4000), Ordering::Relaxed);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn config_search_order() {
        let c = super::config_candidates();
        assert_eq!(
            c.first().unwrap(),
            &std::path::PathBuf::from("expected.conf")
        );
        assert!(c
            .iter()
            .any(|p| p.ends_with("jmon/expected.conf") && !p.starts_with("/etc")));
        assert_eq!(
            c.last().unwrap(),
            &std::path::PathBuf::from("/etc/jmon/expected.conf")
        );
    }
}
