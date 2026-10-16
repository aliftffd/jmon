//! App state (read by the UI) + the logic that turns snapshots into history,
//! benchmark checks, drift events and the fan animation frame.

use crate::model::*;
use std::time::Instant;

pub const FAN_FRAMES: usize = 10;
pub const HISTORY_LEN: usize = 120; // 60 s at 2 Hz

#[derive(Clone, Debug)]
pub struct Expected {
    pub power_mode_id: u32,
    pub power_mode_name: String,
    pub cpu_mhz: u32,
    pub gpu_mhz: u32,
}

impl Default for Expected {
    fn default() -> Self {
        Self {
            power_mode_id: 0,
            power_mode_name: "15W".into(),
            cpu_mhz: 1728,
            gpu_mhz: 612,
        }
    }
}

impl Expected {
    /// `key = value` lines; unknown keys and bad values are ignored (defaults stay).
    pub fn parse(text: &str) -> Self {
        let mut e = Self::default();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "power_mode_id" => e.power_mode_id = v.parse().unwrap_or(e.power_mode_id),
                "power_mode_name" => e.power_mode_name = v.to_string(),
                "cpu_mhz" => e.cpu_mhz = v.parse().unwrap_or(e.cpu_mhz),
                "gpu_mhz" => e.gpu_mhz = v.parse().unwrap_or(e.gpu_mhz),
                _ => {}
            }
        }
        e
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcSort {
    Mem,
    Cpu,
}

#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    pub model: String,
    pub l4t: String,
}

#[derive(Clone)]
pub struct App {
    pub snap: Snapshot,
    pub hist: History,
    pub checks: Vec<Check>,
    /// Newest first.
    pub events: Vec<Event>,
    pub expected: Expected,
    pub device: DeviceInfo,
    pub tab: Tab,
    /// Benchmark preset: 1 Hz sampling, no process scan, fan animation paused.
    pub bench: bool,
    /// Display frozen; sampling continues.
    pub frozen: bool,
    pub sample_ms: u64,
    pub data_age_ms: u64,
    pub started: Instant,
    /// Index into the UI's fan frames (0..FAN_FRAMES).
    pub fan_frame: usize,
    pub has_snapshot: bool,
    pub quit: bool,
    /// Process table order (key `s` toggles). Sensors deliver RSS order; CPU order is applied here.
    pub proc_sort: ProcSort,
    /// Clock drops while locked + OC rise, since start.
    pub throttle: Throttle,
    /// Memory used when the monitor started (baseline for "Δ since start").
    pub mem_used_start_kib: Option<u64>,
    fan_phase: f64,
    fan_tick: Instant,
    oc_baseline: Option<Vec<u64>>,
    watched_pid: Option<u32>,
    engine: String,
}

impl App {
    pub fn new(expected: Expected, device: DeviceInfo) -> Self {
        let now = Instant::now();
        Self {
            snap: Snapshot::default(),
            hist: History::new(HISTORY_LEN),
            checks: Vec::new(),
            events: Vec::new(),
            expected,
            device,
            tab: Tab::Overview,
            bench: false,
            frozen: false,
            sample_ms: 500,
            data_age_ms: 0,
            started: now,
            fan_frame: 0,
            has_snapshot: false,
            quit: false,
            proc_sort: ProcSort::Mem,
            throttle: Throttle::default(),
            mem_used_start_kib: None,
            fan_phase: 0.0,
            fan_tick: now,
            oc_baseline: None,
            watched_pid: None,
            engine: String::new(),
        }
    }

    pub fn uptime_s(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Overall banner state: Bad if any check is Bad, Unknown if any Unknown, else Ok.
    pub fn conditions(&self) -> CheckState {
        if self.checks.iter().any(|c| c.state == CheckState::Bad) {
            CheckState::Bad
        } else if self.checks.is_empty()
            || self.checks.iter().any(|c| c.state == CheckState::Unknown)
        {
            CheckState::Unknown
        } else {
            CheckState::Ok
        }
    }

    pub fn ingest(&mut self, s: Snapshot) {
        self.hist.cpu_total.push(s.cpu_total_pct);
        self.hist
            .gpu_load
            .push(s.gpu.as_ref().map_or(f64::NAN, |g| g.load_pct));
        self.hist.vdd_in_w.push(
            s.rails
                .iter()
                .find(|r| r.name == "VDD_IN")
                .map_or(f64::NAN, |r| r.watts),
        );
        self.hist.fan_rpm.push(
            s.fan
                .as_ref()
                .and_then(|f| f.rpm)
                .map_or(f64::NAN, |r| r as f64),
        );
        let tj = s.temps.iter().find(|t| t.name == "tj").or_else(|| {
            s.temps
                .iter()
                .max_by(|a, b| a.celsius.total_cmp(&b.celsius))
        });
        self.hist.tj_c.push(tj.map_or(f64::NAN, |t| t.celsius));
        self.hist
            .mem_used_gib
            .push(s.mem.used_kib() as f64 / 1_048_576.0);
        self.hist.swap_out_pages_s.push(s.swap.out_pages_s);
        self.hist.watched_rss_gib.push(
            s.watched
                .as_ref()
                .and_then(|w| w.proc.as_ref())
                .map_or(f64::NAN, |p| p.rss_kib as f64 / 1_048_576.0),
        );
        self.hist.model_mem_gib.push(
            s.watched
                .as_ref()
                .and_then(|w| w.mem_est_kib)
                .map_or(f64::NAN, |k| k as f64 / 1_048_576.0),
        );
        self.hist
            .inf_per_s
            .push(s.workload.as_ref().map_or(f64::NAN, |w| w.inf_per_s));
        self.hist.p95_ms.push(
            s.workload
                .as_ref()
                .filter(|w| w.samples_in_window > 0)
                .map_or(f64::NAN, |w| w.p95_ms),
        );
        self.mem_used_start_kib.get_or_insert(s.mem.used_kib());
        if self.oc_baseline.is_none() {
            self.oc_baseline = Some(s.oc_events.clone());
        }
        let mut s = s;
        if self.proc_sort == ProcSort::Cpu {
            s.procs.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct));
        }
        self.track_throttle(&s);
        self.track_watched_and_workload(&s);
        let new_checks = self.evaluate(&s);
        self.log_transitions(&new_checks);
        self.checks = new_checks;
        self.snap = s;
        self.has_snapshot = true;
    }

    fn evaluate(&self, s: &Snapshot) -> Vec<Check> {
        let e = &self.expected;
        let mut out = Vec::new();

        let (obs, st) = match &s.power_mode {
            Some(m) => (
                format!("{} / mode {}", m.name, m.id),
                if m.id == e.power_mode_id {
                    CheckState::Ok
                } else {
                    CheckState::Bad
                },
            ),
            None => ("unreadable".into(), CheckState::Unknown),
        };
        out.push(Check {
            name: "Power profile",
            expected: format!("{} / mode {}", e.power_mode_name, e.power_mode_id),
            observed: obs,
            state: st,
        });

        // Locked = every online core's governor range pinned to the target and running at it.
        let online: Vec<&Cpu> = s.cpus.iter().filter(|c| c.online).collect();
        let (obs, st) = if online.is_empty() {
            ("no cpufreq".into(), CheckState::Unknown)
        } else {
            let lo = online.iter().map(|c| c.cur_mhz).min().unwrap_or(0);
            let hi = online.iter().map(|c| c.cur_mhz).max().unwrap_or(0);
            let locked = online
                .iter()
                .all(|c| c.cur_mhz == e.cpu_mhz && c.min_mhz == c.max_mhz);
            let range = if lo == hi {
                format!("{lo} MHz")
            } else {
                format!("{lo}-{hi} MHz")
            };
            (
                format!("{range} / {}", online[0].governor),
                if locked {
                    CheckState::Ok
                } else {
                    CheckState::Bad
                },
            )
        };
        out.push(Check {
            name: "CPU lock",
            expected: format!("{} MHz locked", e.cpu_mhz),
            observed: obs,
            state: st,
        });

        let (obs, st) = match &s.gpu {
            Some(g) => {
                let locked = g.cur_mhz == e.gpu_mhz && g.min_mhz == g.max_mhz;
                (
                    format!("{} MHz / {}", g.cur_mhz, g.governor),
                    if locked {
                        CheckState::Ok
                    } else {
                        CheckState::Bad
                    },
                )
            }
            None => ("no gpu devfreq".into(), CheckState::Unknown),
        };
        out.push(Check {
            name: "GPU lock",
            expected: format!("{} MHz locked", e.gpu_mhz),
            observed: obs,
            state: st,
        });

        out.push(Check {
            name: "jtop.service",
            expected: "stopped".into(),
            observed: if s.jtop_running {
                "running"
            } else {
                "not running"
            }
            .into(),
            state: if s.jtop_running {
                CheckState::Bad
            } else {
                CheckState::Ok
            },
        });
        out.push(Check {
            name: "tegrastats",
            expected: "not running".into(),
            observed: if s.tegrastats_running {
                "running"
            } else {
                "not running"
            }
            .into(),
            state: if s.tegrastats_running {
                CheckState::Bad
            } else {
                CheckState::Ok
            },
        });

        // Throttling = OC counters rose since start, or a zone crossed its SW trip.
        let base = self.oc_baseline.as_deref().unwrap_or(&s.oc_events);
        let oc_rise: u64 = s
            .oc_events
            .iter()
            .zip(base)
            .map(|(n, b)| n.saturating_sub(*b))
            .sum();
        let hot = s
            .temps
            .iter()
            .find(|t| t.trip_sw.is_some_and(|trip| t.celsius >= trip));
        let (obs, st) = match (oc_rise, hot) {
            (0, None) => ("no throttling".to_string(), CheckState::Ok),
            (n, None) => (format!("{n} OC events"), CheckState::Bad),
            (_, Some(t)) => (
                format!("{} at {:.1}°C ≥ trip", t.name, t.celsius),
                CheckState::Bad,
            ),
        };
        out.push(Check {
            name: "Thermal",
            expected: "no throttling".into(),
            observed: obs,
            state: st,
        });
        out
    }

    fn log_transitions(&mut self, new: &[Check]) {
        let at_s = self.uptime_s();
        for c in new {
            let prev = self
                .checks
                .iter()
                .find(|p| p.name == c.name)
                .map(|p| p.state);
            if prev == Some(c.state) {
                continue;
            }
            let (level, msg) = match (prev, c.state) {
                (_, CheckState::Bad) => (
                    Level::Bad,
                    format!("{}: {} (want {})", c.name, c.observed, c.expected),
                ),
                (Some(CheckState::Bad), CheckState::Ok) => {
                    (Level::Ok, format!("{} restored: {}", c.name, c.observed))
                }
                (_, CheckState::Unknown) => (Level::Warn, format!("{}: {}", c.name, c.observed)),
                _ => continue, // first sample already OK: nothing to report
            };
            self.events.insert(0, Event { at_s, level, msg });
        }
        self.events.truncate(200);
    }

    /// Count drops below the pinned clock, only while clocks are locked (dynamic = drops are normal).
    fn track_throttle(&mut self, s: &Snapshot) {
        let dt = self.sample_ms as f64 / 1000.0;
        let locked = s.clocks != ClockState::Dynamic;
        let cpu_below = locked && s.cpus.iter().any(|c| c.online && c.cur_mhz < c.min_mhz);
        let gpu_below = locked && s.gpu.as_ref().is_some_and(|g| g.cur_mhz < g.min_mhz);
        for (d, below) in [
            (&mut self.throttle.cpu, cpu_below),
            (&mut self.throttle.gpu, gpu_below),
        ] {
            if below {
                if !d.active {
                    d.drops += 1;
                    d.active = true;
                    d.longest_s = d.longest_s.max(0.0);
                }
                d.below_s += dt;
                d.longest_s = d.longest_s.max(d.current_s + dt);
                d.current_s += dt;
            } else {
                d.active = false;
                d.current_s = 0.0;
            }
        }
        let base = self.oc_baseline.as_deref().unwrap_or(&s.oc_events);
        self.throttle.oc_rise = s
            .oc_events
            .iter()
            .zip(base)
            .map(|(n, b)| n.saturating_sub(*b))
            .sum();
    }

    fn track_watched_and_workload(&mut self, s: &Snapshot) {
        if let Some(w) = &s.watched {
            let pid = w.proc.as_ref().map(|p| p.pid);
            match (self.watched_pid, pid) {
                (Some(old), None) => {
                    self.note(Level::Warn, format!("{} exited (pid {old})", w.target))
                }
                (None, Some(new)) => {
                    self.note(Level::Info, format!("{} running (pid {new})", w.target))
                }
                (Some(old), Some(new)) if old != new => self.note(
                    Level::Warn,
                    format!("{} restarted: pid {old} → {new}", w.target),
                ),
                _ => {}
            }
            self.watched_pid = pid;
        }
        if let Some(wl) = &s.workload {
            if !wl.engine.is_empty() && wl.engine != self.engine {
                let msg = format!("workload engine: {}", wl.engine);
                self.engine = wl.engine.clone();
                self.note(Level::Info, msg);
            }
        }
    }

    pub fn note(&mut self, level: Level, msg: impl Into<String>) {
        let at_s = self.uptime_s();
        self.events.insert(
            0,
            Event {
                at_s,
                level,
                msg: msg.into(),
            },
        );
    }

    /// Advance the fan animation (fan-frames.txt contract): fps = clamp(rpm/150, 0, 12),
    /// phase preserved across speed changes, frozen in bench mode / stall / unknown rpm.
    pub fn tick_fan(&mut self, now: Instant) {
        let dt = now.duration_since(self.fan_tick).as_secs_f64();
        self.fan_tick = now;
        let fps = match &self.snap.fan {
            Some(f) if !self.bench && !f.stalled() => {
                f.rpm.map_or(0.0, |r| (r as f64 / 150.0).clamp(0.0, 12.0))
            }
            _ => 0.0,
        };
        self.fan_phase = (self.fan_phase + dt * fps).rem_euclid(FAN_FRAMES as f64);
        self.fan_frame = self.fan_phase.floor() as usize % FAN_FRAMES;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(mode: u32, cpu: u32, gpu: u32, jtop: bool) -> Snapshot {
        Snapshot {
            cpus: (0..6)
                .map(|id| Cpu {
                    id,
                    online: true,
                    cur_mhz: cpu,
                    min_mhz: cpu,
                    max_mhz: cpu,
                    governor: "userspace".into(),
                    ..Default::default()
                })
                .collect(),
            gpu: Some(Gpu {
                cur_mhz: gpu,
                min_mhz: gpu,
                max_mhz: gpu,
                governor: "userspace".into(),
                ..Default::default()
            }),
            power_mode: Some(PowerMode {
                id: mode,
                name: if mode == 0 {
                    "15W".into()
                } else {
                    "MAXN_SUPER".into()
                },
            }),
            jtop_running: jtop,
            fan: Some(Fan {
                pwm: 66,
                pwm_pct: 25.9,
                rpm: Some(1360),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn conditions_and_drift_log() {
        let mut app = App::new(Expected::default(), DeviceInfo::default());
        app.ingest(snap(2, 1267, 306, true));
        assert_eq!(app.conditions(), CheckState::Bad);
        assert_eq!(
            app.checks
                .iter()
                .filter(|c| c.state == CheckState::Bad)
                .count(),
            4
        );
        assert_eq!(app.events.len(), 4);
        app.ingest(snap(0, 1728, 612, false));
        assert_eq!(app.conditions(), CheckState::Ok);
        assert!(app.events[0].level == Level::Ok && app.events[0].msg.contains("restored"));
    }

    #[test]
    fn missing_sensor_is_nan_not_zero() {
        let mut app = App::new(Expected::default(), DeviceInfo::default());
        let mut s = snap(0, 1728, 612, false);
        s.fan = None;
        app.ingest(s);
        assert!(app.hist.fan_rpm.data[0].is_nan());
        assert_eq!(app.hist.fan_rpm.max(), 0.0);
    }

    #[test]
    fn throttle_counts_only_when_locked() {
        let mut app = App::new(Expected::default(), DeviceInfo::default());
        let mut s = snap(0, 1728, 612, false);
        s.clocks = ClockState::Locked {
            cpu_mhz: 1728,
            gpu_mhz: 612,
        };
        app.ingest(s.clone());
        let mut drop = s.clone();
        drop.gpu.as_mut().unwrap().cur_mhz = 408; // pinned min stays 612
        app.ingest(drop.clone());
        app.ingest(drop);
        app.ingest(s.clone());
        let g = &app.throttle.gpu;
        assert_eq!(
            (g.drops, g.below_s, g.longest_s, g.active),
            (1, 1.0, 1.0, false)
        ); // 2 samples × 0.5 s
        assert_eq!(app.throttle.cpu.drops, 0);
        let mut dynamic = s; // under schedutil, drops are not counted
        dynamic.clocks = ClockState::Dynamic;
        dynamic.gpu.as_mut().unwrap().cur_mhz = 306;
        app.ingest(dynamic);
        assert_eq!(app.throttle.gpu.drops, 1);
    }

    #[test]
    fn watched_restart_logged() {
        let mut app = App::new(Expected::default(), DeviceInfo::default());
        let with = |pid: Option<u32>| {
            let mut s = snap(0, 1728, 612, false);
            s.watched = Some(Watched {
                target: "llm".into(),
                proc: pid.map(|pid| Proc {
                    pid,
                    ..Default::default()
                }),
                restarts: 0,
                mem_est_kib: None,
            });
            s
        };
        app.ingest(with(Some(10)));
        app.ingest(with(Some(11)));
        app.ingest(with(None));
        assert!(
            app.events[0].msg.contains("exited")
                && app.events[1].msg.contains("restarted: pid 10 → 11")
        );
    }

    #[test]
    fn expected_conf_parse() {
        let e = Expected::parse("# c\ncpu_mhz = 1500\ngpu_mhz=bad\npower_mode_name = 25W\n");
        assert_eq!(
            (e.cpu_mhz, e.gpu_mhz, e.power_mode_name.as_str()),
            (1500, 612, "25W")
        );
    }

    #[test]
    fn fan_phase() {
        let mut app = App::new(Expected::default(), DeviceInfo::default());
        app.ingest(snap(0, 1728, 612, false)); // 1360 rpm -> ~9.07 fps
        let t0 = app.fan_tick;
        app.tick_fan(t0 + std::time::Duration::from_millis(550));
        assert_eq!(app.fan_frame, 4); // 0.55 s * 9.07 = 4.99 -> frame 4
        app.bench = true;
        app.tick_fan(t0 + std::time::Duration::from_secs(5));
        assert_eq!(app.fan_frame, 4); // frozen in bench mode
    }
}
