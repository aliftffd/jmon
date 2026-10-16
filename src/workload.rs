//! Follows the model's --workload log: one `key=value` line per batch, e.g.
//!   ts=8251.7 lat_ms=1.19 batch=1 engine=v5_fp16 stage_pre=0.21 stage_inf=0.86 stage_post=0.12
//! Required: lat_ms. Optional: batch (default 1), engine, stage_* (ms), tok_ms (LLMs: generation ms per
//! token — compares fairly across short and long answers). Unknown keys ignored.
//! The writer must measure lat_ms AFTER a GPU synchronize and flush every line; the window uses
//! arrival time (ponytail: `ts` is accepted but unused — add it if writers buffer lines).

use crate::model::Workload;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Default rolling window; `--window SECONDS` overrides (LLMs with multi-second requests want 30–60 s).
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(10);
const MAX_KEEP: usize = 2000;

struct Entry {
    at: Instant,
    lat_ms: f64,
    tok_ms: Option<f64>,
    batch: u32,
    stages: Vec<(String, f64)>,
}

pub struct Follower {
    window: Duration,
    path: PathBuf,
    file: Option<File>,
    ino: u64,
    pos: u64,
    partial: String,
    win: VecDeque<Entry>,
    deadline_ms: Option<f64>,
    misses_total: u64,
    total: u64,
    bad: u64,
    engine: String,
    batch: u32,
    last_line: Option<Instant>,
    /// When the work behind the first line began (arrival − its own latency); throughput span starts here.
    work_start: Option<Instant>,
}

#[derive(Debug, PartialEq)]
pub struct Line {
    pub lat_ms: f64,
    pub tok_ms: Option<f64>,
    pub batch: u32,
    pub engine: Option<String>,
    pub stages: Vec<(String, f64)>,
}

pub fn parse_line(line: &str) -> Option<Line> {
    let mut l = Line {
        lat_ms: f64::NAN,
        tok_ms: None,
        batch: 1,
        engine: None,
        stages: Vec::new(),
    };
    for kv in line.split_whitespace() {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        match k {
            "lat_ms" => l.lat_ms = v.parse().ok()?,
            "tok_ms" => l.tok_ms = v.parse().ok().filter(|t: &f64| t.is_finite() && *t >= 0.0),
            "batch" => l.batch = v.parse().ok().filter(|b: &u32| *b > 0)?,
            "engine" => l.engine = Some(v.to_string()),
            _ if k.starts_with("stage_") => {
                if let Ok(ms) = v.parse::<f64>() {
                    l.stages.push((k["stage_".len()..].to_string(), ms));
                }
            }
            _ => {}
        }
    }
    (l.lat_ms.is_finite() && l.lat_ms >= 0.0).then_some(l)
}

/// Nearest-rank percentile of an ascending slice.
fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

impl Follower {
    pub fn new(path: PathBuf, deadline_ms: Option<f64>, window: Duration) -> Self {
        let mut f = Self {
            window,
            path,
            file: None,
            ino: 0,
            pos: 0,
            partial: String::new(),
            win: VecDeque::new(),
            deadline_ms,
            misses_total: 0,
            total: 0,
            bad: 0,
            engine: String::new(),
            batch: 0,
            last_line: None,
            work_start: None,
        };
        f.open(true);
        f
    }

    /// Start at the end on first open (old runs don't count); from 0 after rotation/creation.
    fn open(&mut self, at_end: bool) {
        self.file = File::open(&self.path).ok();
        if let Some(file) = self.file.as_mut() {
            let meta = file.metadata().ok();
            self.ino = meta.as_ref().map_or(0, |m| m.ino());
            self.pos = if at_end {
                meta.map_or(0, |m| m.len())
            } else {
                0
            };
            let _ = file.seek(SeekFrom::Start(self.pos));
        }
        self.partial.clear();
    }

    fn read_new(&mut self) -> String {
        // Rotated (new inode) or created later → reopen from the start.
        let on_disk = std::fs::metadata(&self.path).ok();
        match (&self.file, &on_disk) {
            (None, Some(_)) => self.open(false),
            (Some(_), Some(m)) if m.ino() != self.ino => self.open(false),
            (Some(_), Some(m)) if m.len() < self.pos => self.open(false), // truncated
            _ => {}
        }
        let mut buf = String::new();
        if let Some(file) = self.file.as_mut() {
            let mut bytes = Vec::new();
            if file.read_to_end(&mut bytes).is_ok() {
                self.pos += bytes.len() as u64;
                buf = String::from_utf8_lossy(&bytes).into_owned();
            }
        }
        buf
    }

    /// Called once per sample. `vdd_in_w` for the perf/W estimate.
    pub fn poll(&mut self, vdd_in_w: Option<f64>) -> Workload {
        let now = Instant::now();
        let new = self.read_new();
        self.partial.push_str(&new);
        while let Some(nl) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=nl).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            self.total += 1;
            match parse_line(line) {
                Some(l) => {
                    if self.deadline_ms.is_some_and(|d| l.lat_ms > d) {
                        self.misses_total += 1;
                    }
                    if let Some(e) = l.engine {
                        self.engine = e;
                    }
                    self.batch = l.batch;
                    self.win.push_back(Entry {
                        at: now,
                        lat_ms: l.lat_ms,
                        tok_ms: l.tok_ms,
                        batch: l.batch,
                        stages: l.stages,
                    });
                    self.last_line = Some(now);
                    self.work_start.get_or_insert_with(|| {
                        now.checked_sub(Duration::from_secs_f64(l.lat_ms / 1000.0))
                            .unwrap_or(now)
                    });
                }
                None => self.bad += 1,
            }
        }
        while self
            .win
            .front()
            .is_some_and(|e| now.duration_since(e.at) > self.window)
            || self.win.len() > MAX_KEEP
        {
            self.win.pop_front();
        }
        self.stats(now, vdd_in_w)
    }

    fn stats(&self, now: Instant, vdd_in_w: Option<f64>) -> Workload {
        let mut lat: Vec<f64> = self.win.iter().map(|e| e.lat_ms).collect();
        lat.sort_by(f64::total_cmp);
        // Throughput over the time actually covered: from when the first line's work began (not jmon's start —
        // a model can take seconds to load), capped at the window.
        let span = self
            .work_start
            .map_or(0.0, |s| {
                now.duration_since(s).min(self.window).as_secs_f64()
            })
            .max(1e-3);
        let mut gaps: Vec<f64> = self
            .win
            .iter()
            .zip(self.win.iter().skip(1))
            .map(|(a, b)| b.at.duration_since(a.at).as_secs_f64())
            .collect();
        gaps.sort_by(f64::total_cmp);
        let mut tok: Vec<f64> = self.win.iter().filter_map(|e| e.tok_ms).collect();
        tok.sort_by(f64::total_cmp);
        let inf_per_s = self.win.iter().map(|e| e.batch as f64).sum::<f64>() / span;
        let mut stages: Vec<(String, f64, u32)> = Vec::new();
        for e in &self.win {
            for (k, v) in &e.stages {
                match stages.iter_mut().find(|s| &s.0 == k) {
                    Some(s) => (s.1, s.2) = (s.1 + v, s.2 + 1),
                    None => stages.push((k.clone(), *v, 1)),
                }
            }
        }
        Workload {
            path: self.path.display().to_string(),
            inf_per_s,
            p50_ms: pct(&lat, 50.0),
            p95_ms: pct(&lat, 95.0),
            p99_ms: pct(&lat, 99.0),
            samples_in_window: lat.len(),
            window_s: self.window.as_secs_f64(),
            tok_ms_p50: (!tok.is_empty()).then(|| pct(&tok, 50.0)),
            tok_ms_p95: (!tok.is_empty()).then(|| pct(&tok, 95.0)),
            deadline_ms: self.deadline_ms,
            misses_window: self
                .deadline_ms
                .map_or(0, |d| lat.iter().filter(|l| **l > d).count() as u64),
            misses_total: self.misses_total,
            total_lines: self.total,
            bad_lines: self.bad,
            engine: self.engine.clone(),
            batch: self.batch,
            stages: stages
                .into_iter()
                .map(|(k, sum, n)| (k, sum / n as f64))
                .collect(),
            inf_per_watt: vdd_in_w
                .filter(|w| *w > 0.0 && inf_per_s > 0.0)
                .map(|w| inf_per_s / w),
            idle_s: self
                .last_line
                .map_or(0.0, |t| now.duration_since(t).as_secs_f64()),
            typical_gap_s: if gaps.is_empty() {
                0.0
            } else {
                gaps[gaps.len() / 2]
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse() {
        let l = parse_line(
            "ts=1.0 lat_ms=1.19 batch=8 engine=v5_fp16 stage_pre=0.2 stage_inf=0.9 extra=x",
        )
        .unwrap();
        assert_eq!(
            (l.lat_ms, l.batch, l.engine.as_deref()),
            (1.19, 8, Some("v5_fp16"))
        );
        assert_eq!(l.stages, vec![("pre".into(), 0.2), ("inf".into(), 0.9)]);
        assert!(parse_line("batch=1").is_none()); // lat_ms required
        assert!(parse_line("lat_ms=abc").is_none());
        assert!(parse_line("lat_ms=1 batch=0").is_none());
        assert_eq!(
            parse_line("lat_ms=4000 tok_ms=31.7").unwrap().tok_ms,
            Some(31.7)
        );
        assert_eq!(parse_line("lat_ms=4000 tok_ms=x").unwrap().tok_ms, None); // bad optional field ≠ bad line
    }

    #[test]
    fn llm_cadence_throughput_and_stall() {
        let dir = std::env::temp_dir().join(format!("jmon-wl2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("w.log");
        let mut f = Follower::new(p.clone(), None, Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(300)); // "model loading": must not dilute tok/s
        std::fs::write(&p, "lat_ms=200 batch=10\n").unwrap();
        let w = f.poll(None);
        // 10 tokens over ~0.2 s of work, not over 0.5 s since jmon started
        assert!(
            w.inf_per_s > 30.0,
            "tok/s {} diluted by load time",
            w.inf_per_s
        );
        let mut s = Workload {
            total_lines: 5,
            typical_gap_s: 4.2,
            idle_s: 3.5,
            ..Default::default()
        };
        assert!(!s.stalled()); // 3.5 s gap is normal for 4.2 s LLM requests
        s.idle_s = 13.0;
        assert!(s.stalled());
        s.typical_gap_s = 0.01;
        s.idle_s = 2.0;
        assert!(!s.stalled()); // floor of 3 s
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn percentiles() {
        let v: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(
            (pct(&v, 50.0), pct(&v, 95.0), pct(&v, 99.0)),
            (50.0, 95.0, 99.0)
        );
        assert_eq!(pct(&[], 50.0), 0.0);
    }

    #[test]
    fn follow_append_truncate_rotate() {
        let dir = std::env::temp_dir().join(format!("jmon-wl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("w.log");
        std::fs::write(&p, "lat_ms=99 batch=1\n").unwrap(); // pre-existing run: must be ignored
        let mut f = Follower::new(p.clone(), Some(5.0), DEFAULT_WINDOW);
        let mut file = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        for i in 0..10 {
            writeln!(
                file,
                "lat_ms={} batch=2 engine=v0_fp32 tok_ms={}",
                1 + i,
                30 + i
            )
            .unwrap();
        }
        write!(file, "lat_ms=7 bat").unwrap(); // partial line waits
        let w = f.poll(Some(5.0));
        assert_eq!(
            (w.total_lines, w.samples_in_window, w.engine.as_str()),
            (10, 10, "v0_fp32")
        );
        assert_eq!((w.p50_ms, w.p99_ms), (5.0, 10.0));
        assert_eq!(
            (w.tok_ms_p50, w.tok_ms_p95, w.window_s),
            (Some(34.0), Some(39.0), 10.0)
        );
        assert_eq!((w.misses_window, w.misses_total), (5, 5)); // 6..10 > 5 ms
        assert!(w.inf_per_s > 0.0 && w.inf_per_watt.is_some());
        writeln!(file, "ch=1").unwrap(); // completes "lat_ms=7 batch=1"
        writeln!(file, "garbage").unwrap();
        let w = f.poll(None);
        assert_eq!((w.total_lines, w.bad_lines, w.batch), (12, 1, 1));
        std::fs::write(&p, "lat_ms=3\n").unwrap(); // truncated + rewritten → re-read from 0
        let w = f.poll(None);
        assert_eq!(w.total_lines, 13);
        std::fs::remove_file(&p).unwrap();
        std::fs::write(&p, "lat_ms=4\nlat_ms=4\n").unwrap(); // rotated (new inode)
        let w = f.poll(None);
        assert_eq!(w.total_lines, 15);
        std::fs::remove_dir_all(&dir).ok();
    }
}
