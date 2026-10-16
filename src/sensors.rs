//! Sensor discovery (once, by name) and sampling (every tick, small file reads only).
//! All board paths go through `root` so a captured fixture can stand in for the device.

use crate::model::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Only processes at least this big get their fds checked for GPU device nodes
/// (a CUDA context alone is ~300 MiB). ponytail: smaller GPU users show GpuUse::No; lower if needed.
const GPU_CHECK_MIN_KIB: u64 = 64 * 1024;
const PROC_TABLE_LEN: usize = 64;

#[derive(Clone, Debug)]
pub enum WatchTarget {
    Name(String),
    Pid(u32),
}

fn read(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| {
        s.trim_matches(|c: char| c.is_whitespace() || c == '\0')
            .to_string()
    })
}
fn num<T: std::str::FromStr>(p: &Path) -> Option<T> {
    read(p)?.parse().ok()
}

struct DevfreqDev {
    dir: PathBuf,
    name: String,
}

struct RailPaths {
    name: String,
    mv: PathBuf,
    ma: PathBuf,
}

struct Zone {
    name: String,
    temp: PathBuf,
    trip_sw: Option<f64>,
    trip_hw: Option<f64>,
}

pub struct Sensors {
    root: PathBuf,
    cpus: Vec<(usize, PathBuf)>,
    gpu: Option<(DevfreqDev, Option<PathBuf>, Vec<u32>)>,
    engines: Vec<DevfreqDev>,
    rails: Vec<RailPaths>,
    zones: Vec<Zone>,
    fan_pwm: Option<PathBuf>,
    fan_rpm: Option<PathBuf>,
    oc: Vec<PathBuf>,
    prev_stat: Vec<(u64, u64)>, // per line of /proc/stat "cpu*": (busy, total)
    prev_self: Option<(u64, Instant)>,
    mode_names: Vec<(u32, String)>,
    scan_every: u32,
    scans: u32,
    procs: (bool, bool),
    /// Process table on/off (off in bench mode; jtop/tegrastats detection keeps running).
    pub proc_table: bool,
    users: HashMap<u32, String>,
    proc_ticks: HashMap<u32, u64>,
    proc_scan_at: Option<Instant>,
    last_table: Vec<Proc>,
    prev_vm: Option<([u64; 3], Instant)>,
    watch: Option<WatchTarget>,
    watched: Option<Watched>,
    watch_ticks: Option<(u32, u64, Instant)>,
    /// MemAvailable of the previous sample (None before the first sample).
    prev_avail_kib: Option<u64>,
    /// (pid, MemAvailable baseline) for the model-memory estimate.
    watch_base: Option<(u32, Option<u64>)>,
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

impl Sensors {
    pub fn discover(root: &Path) -> Self {
        let r = |p: &str| root.join(p.trim_start_matches('/'));

        let mut cpus: Vec<(usize, PathBuf)> = sorted_dirs(&r("/sys/devices/system/cpu"))
            .into_iter()
            .filter_map(|p| {
                let id = p.file_name()?.to_str()?.strip_prefix("cpu")?.parse().ok()?;
                p.join("cpufreq").exists().then_some((id, p))
            })
            .collect();
        cpus.sort_by_key(|c| c.0);

        let mut gpu = None;
        let mut engines = Vec::new();
        for dir in sorted_dirs(&r("/sys/class/devfreq")) {
            let name = read(&dir.join("device/of_node/name")).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let dev = DevfreqDev {
                dir: dir.clone(),
                name: name.clone(),
            };
            if name == "gpu" {
                // load lives on the platform device, e.g. /sys/devices/platform/bus@0/17000000.gpu/load
                let node = dir.file_name().map(|n| n.to_owned()).unwrap_or_default();
                let load = [
                    r("/sys/devices/platform/bus@0").join(&node).join("load"),
                    r("/sys/devices/platform").join(&node).join("load"),
                ]
                .into_iter()
                .find(|p| p.exists());
                let mut steps: Vec<u32> = read(&dir.join("available_frequencies"))
                    .unwrap_or_default()
                    .split_whitespace()
                    .filter_map(|s| s.parse::<u64>().ok().map(|hz| (hz / 1_000_000) as u32))
                    .collect();
                steps.sort();
                gpu = Some((dev, load, steps));
            } else {
                engines.push(dev);
            }
        }

        let mut rails = Vec::new();
        let (mut fan_pwm, mut fan_rpm, mut oc) = (None, None, Vec::new());
        for h in sorted_dirs(&r("/sys/class/hwmon")) {
            match read(&h.join("name")).as_deref() {
                Some("ina3221") => {
                    for i in 1..=4 {
                        if let Some(label) = read(&h.join(format!("in{i}_label"))) {
                            rails.push(RailPaths {
                                name: label,
                                mv: h.join(format!("in{i}_input")),
                                ma: h.join(format!("curr{i}_input")),
                            });
                        }
                    }
                }
                Some("pwmfan") => fan_pwm = Some(h.join("pwm1")).filter(|p| p.exists()),
                Some("pwm_tach") => fan_rpm = Some(h.join("rpm")).filter(|p| p.exists()),
                Some("soctherm_oc") => {
                    oc = (1..=8)
                        .map(|i| h.join(format!("oc{i}_event_cnt")))
                        .filter(|p| p.exists())
                        .collect();
                }
                _ => {}
            }
        }

        let zones = sorted_dirs(&r("/sys/class/thermal"))
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("thermal_zone"))
            })
            .filter_map(|z| {
                let ty = read(&z.join("type"))?;
                num::<i64>(&z.join("temp"))?; // skip zones that report nothing (cv0..cv2)
                let trips: Vec<f64> = (0..8)
                    .filter_map(|i| num::<i64>(&z.join(format!("trip_point_{i}_temp"))))
                    .map(|m| m as f64 / 1000.0)
                    .collect();
                // SW trip = lowest trip ≥ 90 °C (lower ones are fan/active points), HW = highest.
                let trip_sw = trips
                    .iter()
                    .cloned()
                    .filter(|t| *t >= 90.0)
                    .reduce(f64::min);
                let trip_hw = trips.iter().cloned().reduce(f64::max);
                Some(Zone {
                    name: ty.trim_end_matches("-thermal").to_string(),
                    temp: z.join("temp"),
                    trip_sw,
                    trip_hw,
                })
            })
            .collect();

        Self {
            root: root.to_path_buf(),
            cpus,
            gpu,
            engines,
            rails,
            zones,
            fan_pwm,
            fan_rpm,
            oc,
            prev_stat: Vec::new(),
            prev_self: None,
            mode_names: parse_mode_names(&read(&r("/etc/nvpmodel.conf")).unwrap_or_default()),
            scan_every: 10,
            scans: 0,
            procs: (false, false),
            proc_table: true,
            users: parse_passwd(&read(&r("/etc/passwd")).unwrap_or_default()),
            proc_ticks: HashMap::new(),
            proc_scan_at: None,
            last_table: Vec::new(),
            prev_vm: None,
            watch: None,
            watched: None,
            watch_ticks: None,
            prev_avail_kib: None,
            watch_base: None,
        }
    }

    pub fn set_watch(&mut self, w: WatchTarget) {
        let target = match &w {
            WatchTarget::Name(n) => n.clone(),
            WatchTarget::Pid(p) => format!("pid {p}"),
        };
        self.watched = Some(Watched {
            target,
            proc: None,
            restarts: 0,
            mem_est_kib: None,
        });
        self.watch = Some(w);
    }

    /// Process scan cadence in samples (every 10 samples = 5 s at 2 Hz).
    pub fn set_scan_every(&mut self, n: u32) {
        self.scan_every = n.max(1);
    }

    pub fn device_model(&self) -> String {
        read(&self.root.join("proc/device-tree/model")).unwrap_or_else(|| "NVIDIA Jetson".into())
    }

    pub fn l4t_release(&self) -> String {
        // "# R36 (release), REVISION: 5.2, ..." -> "R36.5.2"
        let s = read(&self.root.join("etc/nv_tegra_release")).unwrap_or_default();
        let major = s.split_whitespace().nth(1).unwrap_or("");
        let rev = s
            .split("REVISION:")
            .nth(1)
            .and_then(|r| r.split(',').next())
            .map(str::trim)
            .unwrap_or("");
        if major.is_empty() {
            "L4T ?".into()
        } else {
            format!("L4T {major}.{rev}")
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let r = |p: &str| self.root.join(p.trim_start_matches('/'));
        let mut s = Snapshot::default();

        // CPU utilisation from /proc/stat deltas. Line 0 = total, then cpuN.
        let stat = read(&r("/proc/stat")).unwrap_or_default();
        let cur: Vec<(u64, u64)> = stat
            .lines()
            .filter(|l| l.starts_with("cpu"))
            .map(|l| {
                let v: Vec<u64> = l
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|x| x.parse().ok())
                    .collect();
                let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
                let total: u64 = v.iter().take(8).sum();
                (total - idle, total)
            })
            .collect();
        let pct = |i: usize| -> f64 {
            match (cur.get(i), self.prev_stat.get(i)) {
                (Some(&(b, t)), Some(&(pb, pt))) if t > pt => {
                    (b.saturating_sub(pb)) as f64 * 100.0 / (t - pt) as f64
                }
                _ => 0.0,
            }
        };
        s.cpu_total_pct = pct(0);
        for (n, (id, dir)) in self.cpus.iter().enumerate() {
            let f = dir.join("cpufreq");
            s.cpus.push(Cpu {
                id: *id,
                online: read(&dir.join("online")).is_none_or(|v| v == "1"),
                util_pct: pct(n + 1),
                cur_mhz: num::<u32>(&f.join("scaling_cur_freq")).unwrap_or(0) / 1000,
                min_mhz: num::<u32>(&f.join("scaling_min_freq")).unwrap_or(0) / 1000,
                max_mhz: num::<u32>(&f.join("scaling_max_freq")).unwrap_or(0) / 1000,
                hw_max_mhz: num::<u32>(&f.join("cpuinfo_max_freq")).unwrap_or(0) / 1000,
                governor: read(&f.join("scaling_governor")).unwrap_or_default(),
            });
        }
        self.prev_stat = cur;

        if let Some(l) = read(&r("/proc/loadavg")) {
            for (i, v) in l.split_whitespace().take(3).enumerate() {
                s.load_avg[i] = v.parse().unwrap_or(0.0);
            }
        }

        let mhz = |p: PathBuf| num::<u64>(&p).map_or(0, |hz| (hz / 1_000_000) as u32);
        if let Some((dev, load, steps)) = &self.gpu {
            s.gpu = Some(Gpu {
                load_pct: load
                    .as_ref()
                    .and_then(|p| num::<f64>(p))
                    .map_or(0.0, |permille| permille / 10.0),
                cur_mhz: mhz(dev.dir.join("cur_freq")),
                min_mhz: mhz(dev.dir.join("min_freq")),
                max_mhz: mhz(dev.dir.join("max_freq")),
                governor: read(&dev.dir.join("governor")).unwrap_or_default(),
                steps_mhz: steps.clone(),
            });
        }
        s.clocks = ClockState::classify(&s.cpus, s.gpu.as_ref());
        s.engines = self
            .engines
            .iter()
            .map(|e| (e.name.to_uppercase(), mhz(e.dir.join("cur_freq"))))
            .collect();

        let mem = read(&r("/proc/meminfo")).unwrap_or_default();
        let kib = |key: &str| {
            mem.lines()
                .find(|l| l.split(':').next() == Some(key))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        s.mem = Mem {
            total_kib: kib("MemTotal"),
            avail_kib: kib("MemAvailable"),
            cached_kib: kib("Cached"),
            swap_total_kib: kib("SwapTotal"),
            swap_free_kib: kib("SwapFree"),
        };

        s.rails = self
            .rails
            .iter()
            .filter_map(|rp| {
                let (v, a) = (num::<f64>(&rp.mv)? / 1000.0, num::<f64>(&rp.ma)? / 1000.0);
                Some(Rail {
                    name: rp.name.clone(),
                    volts: v,
                    amps: a,
                    watts: v * a,
                })
            })
            .collect();

        s.temps = self
            .zones
            .iter()
            .filter_map(|z| {
                Some(Temp {
                    name: z.name.clone(),
                    celsius: num::<f64>(&z.temp)? / 1000.0,
                    trip_sw: z.trip_sw,
                    trip_hw: z.trip_hw,
                })
            })
            .collect();

        if let Some(pwm) = self.fan_pwm.as_ref().and_then(|p| num::<u32>(p)) {
            let pwm = pwm.min(255) as u8;
            s.fan = Some(Fan {
                pwm,
                pwm_pct: pwm as f64 * 100.0 / 255.0,
                rpm: self.fan_rpm.as_ref().and_then(|p| num(p)),
            });
        }
        s.oc_events = self.oc.iter().map(|p| num(p).unwrap_or(0)).collect();

        s.power_mode = read(&r("/var/lib/nvpmodel/status")).and_then(|st| {
            let id: u32 = st.strip_prefix("pmode:")?.trim().parse().ok()?;
            let name = self
                .mode_names
                .iter()
                .find(|m| m.0 == id)
                .map_or_else(|| format!("mode {id}"), |m| m.1.clone());
            Some(PowerMode { id, name })
        });

        s.swap = self.swap_stats();

        // One /proc walk serves jtop/tegrastats detection (every `scan_every` samples, ≈5 s),
        // the process table (≈2 s, off in bench mode) and finding a --watch NAME target.
        let now = Instant::now();
        let table_due = self.proc_table
            && self
                .proc_scan_at
                .is_none_or(|t| now.duration_since(t).as_secs_f64() >= 2.0);
        let watch_lost = matches!(self.watch, Some(WatchTarget::Name(_)))
            && self.watched.as_ref().is_some_and(|w| w.proc.is_none());
        if self.scans.is_multiple_of(self.scan_every) || table_due || watch_lost {
            self.scan(table_due, now);
        }
        self.scans = self.scans.wrapping_add(1);
        (s.jtop_running, s.tegrastats_running) = self.procs;
        s.procs = if self.proc_table {
            self.last_table.clone()
        } else {
            Vec::new()
        };
        self.update_watched(now, s.mem.avail_kib);
        self.prev_avail_kib = Some(s.mem.avail_kib);
        s.watched = self.watched.clone();

        // Own overhead always from the real /proc/self, not the fixture.
        if let Some(st) = read(Path::new("/proc/self/stat")) {
            let f: Vec<&str> = st
                .rsplit(')')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let ticks: u64 = f.get(11).and_then(|v| v.parse().ok()).unwrap_or(0)
                + f.get(12).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let now = Instant::now();
            if let Some((pt, pi)) = self.prev_self {
                let secs = now.duration_since(pi).as_secs_f64();
                if secs > 0.0 {
                    s.self_cpu_pct = (ticks.saturating_sub(pt)) as f64 / 100.0 / secs * 100.0;
                    // USER_HZ = 100
                }
            }
            self.prev_self = Some((ticks, now));
        }
        if let Some(statm) = read(Path::new("/proc/self/statm")) {
            let pages: f64 = statm
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            s.self_rss_mib = pages * 4096.0 / 1_048_576.0;
        }
        s
    }
}

/// `< POWER_MODEL ID=0 NAME=15W >` lines from /etc/nvpmodel.conf.
fn parse_mode_names(conf: &str) -> Vec<(u32, String)> {
    conf.lines()
        .filter(|l| l.contains("POWER_MODEL"))
        .filter_map(|l| {
            let id = l
                .split("ID=")
                .nth(1)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            let name = l
                .split("NAME=")
                .nth(1)?
                .split_whitespace()
                .next()?
                .to_string();
            Some((id, name))
        })
        .collect()
}

fn parse_passwd(text: &str) -> HashMap<u32, String> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?.to_string();
            let uid = f.nth(1)?.parse().ok()?;
            Some((uid, name))
        })
        .collect()
}

fn status_kib(status: &str, key: &str) -> u64 {
    status
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// utime + stime (clock ticks) from /proc/<pid>/stat; comm may contain spaces → split after ')'.
fn proc_ticks(stat: &str) -> u64 {
    let f: Vec<&str> = stat
        .rsplit(')')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    f.get(11).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
        + f.get(12).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
}

/// GPU device nodes open? Unknown if the fd dir isn't readable (other user's process without root).
fn gpu_use(pid_dir: &Path) -> GpuUse {
    let Ok(fds) = fs::read_dir(pid_dir.join("fd")) else {
        return GpuUse::Unknown;
    };
    for fd in fds.flatten() {
        if let Ok(target) = fs::read_link(fd.path()) {
            let t = target.to_string_lossy();
            if t.starts_with("/dev/nvgpu") || t == "/dev/nvmap" {
                return GpuUse::Yes;
            }
        }
    }
    GpuUse::No
}

fn cmdline(pid_dir: &Path) -> String {
    let raw = fs::read(pid_dir.join("cmdline")).unwrap_or_default();
    let mut cmd: String = String::from_utf8_lossy(&raw)
        .split('\0')
        .filter(|a| !a.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    cmd.truncate(200);
    cmd
}

impl Sensors {
    fn scan(&mut self, table: bool, now: Instant) {
        let proc_dir = self.root.join("proc");
        let secs = self
            .proc_scan_at
            .map(|t| now.duration_since(t).as_secs_f64())
            .filter(|s| *s > 0.0);
        let (mut jtop, mut tegra) = (false, false);
        let mut rows = Vec::new();
        let mut ticks = HashMap::new();
        let want_name = match (&self.watch, &self.watched) {
            (Some(WatchTarget::Name(n)), Some(w)) if w.proc.is_none() => Some(n.clone()),
            _ => None,
        };
        let mut found: Option<u32> = None;
        for e in fs::read_dir(&proc_dir).into_iter().flatten().flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let dir = e.path();
            let cmd = cmdline(&dir);
            let mut args = cmd.split(' ');
            let base = |s: &str| s.rsplit('/').next().unwrap_or("").to_string();
            let (a0, a1) = (
                base(args.next().unwrap_or("")),
                base(args.next().unwrap_or("")),
            );
            jtop |= a0 == "jtop" || a1 == "jtop";
            tegra |= a0 == "tegrastats";
            if let Some(n) = &want_name {
                if found.is_none() && pid != std::process::id() && cmd.contains(n.as_str()) {
                    found = Some(pid);
                }
            }
            if !table {
                continue;
            }
            let Some(status) = read(&dir.join("status")) else {
                continue;
            };
            let t = read(&dir.join("stat")).map_or(0, |st| proc_ticks(&st));
            ticks.insert(pid, t);
            let cpu_pct = match (secs, self.proc_ticks.get(&pid)) {
                (Some(dt), Some(prev)) => t.saturating_sub(*prev) as f64 / 100.0 / dt * 100.0, // USER_HZ = 100
                _ => 0.0,
            };
            let rss_kib = status_kib(&status, "VmRSS:");
            let uid: u32 = status
                .lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let gpu = if status_kib(&status, "RssAnon:") >= GPU_CHECK_MIN_KIB {
                gpu_use(&dir)
            } else {
                GpuUse::No
            };
            let cmd = if cmd.is_empty() {
                format!("[{}]", read(&dir.join("comm")).unwrap_or_default())
            } else {
                cmd
            };
            rows.push(Proc {
                pid,
                user: self
                    .users
                    .get(&uid)
                    .cloned()
                    .unwrap_or_else(|| uid.to_string()),
                cpu_pct,
                rss_kib,
                swap_kib: status_kib(&status, "VmSwap:"),
                gpu,
                cmd,
            });
        }
        self.procs = (jtop, tegra);
        if table {
            rows.sort_by_key(|p| std::cmp::Reverse(p.rss_kib));
            rows.truncate(PROC_TABLE_LEN);
            self.last_table = rows;
            self.proc_ticks = ticks;
            self.proc_scan_at = Some(now);
        }
        if let (Some(pid), Some(w)) = (found, self.watched.as_mut()) {
            if self.watch_ticks.is_some() {
                w.restarts += 1; // a previous instance existed and exited
            }
            self.watch_ticks = Some((pid, 0, now));
        }
    }

    /// Read the watched PID every sample (cheap: stat + status + fds only for this one process).
    fn update_watched(&mut self, now: Instant, avail_kib: u64) {
        let pid = match (&self.watch, self.watch_ticks) {
            (Some(WatchTarget::Pid(p)), _) => *p,
            (Some(WatchTarget::Name(_)), Some((p, _, _))) => p,
            _ => {
                if let Some(w) = self.watched.as_mut() {
                    w.proc = None;
                }
                return;
            }
        };
        let dir = self.root.join("proc").join(pid.to_string());
        let (Some(status), Some(w)) = (read(&dir.join("status")), self.watched.as_mut()) else {
            if let Some(w) = self.watched.as_mut() {
                w.proc = None;
            }
            if let Some(w) = self.watched.as_mut() {
                w.mem_est_kib = None;
            }
            return; // exited; --watch NAME re-finds a new PID on the next scan
        };
        let t = read(&dir.join("stat")).map_or(0, |st| proc_ticks(&st));
        let cpu_pct = match self.watch_ticks {
            Some((p, prev, at)) if p == pid && prev > 0 => {
                let dt = now.duration_since(at).as_secs_f64();
                if dt > 0.0 {
                    t.saturating_sub(prev) as f64 / 100.0 / dt * 100.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        self.watch_ticks = Some((pid, t, now));
        let uid: u32 = status
            .lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        w.proc = Some(Proc {
            pid,
            user: self
                .users
                .get(&uid)
                .cloned()
                .unwrap_or_else(|| uid.to_string()),
            cpu_pct,
            rss_kib: status_kib(&status, "VmRSS:"),
            swap_kib: status_kib(&status, "VmSwap:"),
            gpu: gpu_use(&dir),
            cmd: cmdline(&dir),
        });
        // Baseline = MemAvailable of the sample BEFORE this pid was first seen (none on jmon's first sample).
        if self.watch_base.map(|b| b.0) != Some(pid) {
            self.watch_base = Some((pid, self.prev_avail_kib));
        }
        w.mem_est_kib = self
            .watch_base
            .and_then(|b| b.1)
            .map(|base| base as i64 - avail_kib as i64);
    }

    fn swap_stats(&mut self) -> SwapStats {
        let mut st = SwapStats::default();
        let vm = read(&self.root.join("proc/vmstat")).unwrap_or_default();
        let get = |k: &str| {
            vm.lines()
                .find(|l| l.split(' ').next() == Some(k))
                .and_then(|l| l.split(' ').nth(1))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
        };
        let cur = [get("pswpin"), get("pswpout"), get("pgmajfault")];
        let now = Instant::now();
        if let Some((prev, at)) = self.prev_vm {
            let dt = now.duration_since(at).as_secs_f64();
            if dt > 0.0 {
                let rate = |i: usize| cur[i].saturating_sub(prev[i]) as f64 / dt;
                (st.in_pages_s, st.out_pages_s, st.majflt_s) = (rate(0), rate(1), rate(2));
            }
        }
        self.prev_vm = Some((cur, now));
        // /proc/swaps: Filename Type Size Used Priority (KiB)
        for l in read(&self.root.join("proc/swaps"))
            .unwrap_or_default()
            .lines()
            .skip(1)
        {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (size, used) = (
                f.get(2).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0),
                f.get(3).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0),
            );
            if f.first().is_some_and(|n| n.contains("zram")) {
                st.zram_total_kib += size;
                st.zram_used_kib += used;
            } else {
                st.file_total_kib += size;
                st.file_used_kib += used;
            }
        }
        st
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fixture_like_the_board() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture");
        let mut s = Sensors::discover(&root);
        let snap = s.sample();
        assert_eq!(snap.cpus.len(), 6);
        // Expected watts straight from the fixture's raw mV/mA files (hwmon1 = ina3221 ch1).
        // Expected values come straight from the fixture's raw files (they change per capture).
        let raw = |f: &str| {
            fs::read_to_string(root.join("sys/class/hwmon").join(f))
                .unwrap()
                .trim()
                .parse::<f64>()
                .unwrap()
        };
        let vin = snap
            .rails
            .iter()
            .find(|r| r.name == "VDD_IN")
            .expect("VDD_IN");
        assert!(
            (vin.watts - raw("hwmon1/in1_input") * raw("hwmon1/curr1_input") / 1e6).abs() < 1e-9,
            "VDD_IN {} W",
            vin.watts
        );
        assert_eq!(snap.rails.len(), 3);
        let fan = snap.fan.expect("fan");
        assert_eq!(
            (fan.pwm as f64, fan.rpm.map(|r| r as f64)),
            (raw("hwmon0/pwm1"), Some(raw("hwmon2/rpm")))
        );
        let gpu = snap.gpu.expect("gpu");
        assert_eq!(
            (gpu.min_mhz, gpu.max_mhz, gpu.steps_mhz.len()),
            (306, 1020, 8)
        );
        assert!(
            snap.temps.iter().all(|t| !t.name.starts_with("cv")),
            "cv zones must be skipped"
        );
        assert!(snap.temps.iter().any(|t| t.name == "tj"));
        let cpu = snap.temps.iter().find(|t| t.name == "cpu").unwrap();
        assert_eq!((cpu.trip_sw, cpu.trip_hw), (Some(99.0), Some(104.5)));
        assert_eq!(snap.oc_events, vec![0, 0, 0]);
        assert_eq!(
            snap.power_mode,
            Some(PowerMode {
                id: 2,
                name: "MAXN_SUPER".into()
            })
        );
        assert_eq!(s.l4t_release(), "L4T R36.5.2");
        assert!(snap.engines.iter().any(|(n, _)| n == "NVDEC"));
        assert_eq!(snap.cpus[0].hw_max_mhz, 1728);
        assert_eq!(snap.clocks, ClockState::Dynamic); // fixture captured in MAXN_SUPER, schedutil
    }

    #[test]
    fn swap_tiers_from_fixture() {
        let mut s = Sensors::discover(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture"));
        let st = s.sample().swap;
        assert_eq!(
            (st.zram_total_kib, st.file_total_kib),
            (6 * 649156, 8388604)
        ); // 6 zram + NVMe swapfile
    }

    /// Real /proc on whatever machine runs the tests: table sorted + capped, --pid self readable.
    #[test]
    fn process_scan_finds_self() {
        let mut s = Sensors::discover(Path::new("/"));
        s.set_watch(WatchTarget::Pid(std::process::id()));
        let snap = s.sample();
        assert!(!snap.procs.is_empty() && snap.procs.len() <= PROC_TABLE_LEN);
        assert!(
            snap.procs.windows(2).all(|w| w[0].rss_kib >= w[1].rss_kib),
            "sorted by RSS desc"
        );
        let me = snap
            .watched
            .expect("watched")
            .proc
            .expect("own pid readable");
        assert!(me.pid == std::process::id() && me.rss_kib > 0 && !me.cmd.is_empty());
    }

    /// Model memory = MemAvailable(sample before the PID appeared) − MemAvailable(now), even when the
    /// process RSS is small (llama.cpp keeps its GPU weights outside RSS).
    #[test]
    fn model_memory_estimate() {
        let root = std::env::temp_dir().join(format!("jmon-mem-{}", std::process::id()));
        let meminfo = |avail: u64| {
            std::fs::write(
                root.join("proc/meminfo"),
                format!("MemTotal: 7789884 kB\nMemAvailable: {avail} kB\n"),
            )
            .unwrap()
        };
        std::fs::create_dir_all(root.join("proc")).unwrap();
        meminfo(6_000_000);
        let mut s = Sensors::discover(&root);
        s.set_watch(WatchTarget::Name("llama-server".into()));
        assert!(s.sample().watched.unwrap().proc.is_none());
        // model starts: 1.33 GiB gone system-wide, but RSS only 0.7 GiB
        let pd = root.join("proc/4242");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(
            pd.join("cmdline"),
            "/home/u/llama.cpp/build/bin/llama-server\0-m\0q.gguf\0",
        )
        .unwrap();
        std::fs::write(
            pd.join("status"),
            "Uid:\t1000\nVmRSS:\t746496 kB\nVmSwap:\t0 kB\n",
        )
        .unwrap();
        std::fs::write(
            pd.join("stat"),
            "4242 (llama-server) S 1 0 0 0 0 0 0 0 0 0 10 5 0",
        )
        .unwrap();
        meminfo(4_640_000);
        let w = s.sample().watched.unwrap();
        assert_eq!(w.proc.as_ref().map(|p| p.pid), Some(4242));
        assert_eq!(w.mem_est_kib, Some(1_360_000));
        meminfo(4_600_000); // KV cache grows
        assert_eq!(s.sample().watched.unwrap().mem_est_kib, Some(1_400_000));
        // jmon started after the model: no baseline → None
        let mut late = Sensors::discover(&root);
        late.set_watch(WatchTarget::Name("llama-server".into()));
        assert_eq!(late.sample().watched.unwrap().mem_est_kib, None);
        std::fs::remove_dir_all(&root).ok();
    }

    /// Run on the Jetson: `cargo test -- --ignored live_board` (reads the real /sys, read-only).
    #[test]
    #[ignore]
    fn live_board() {
        let mut s = Sensors::discover(Path::new("/"));
        if let Ok(name) = std::env::var("JMON_WATCH") {
            s.set_watch(WatchTarget::Name(name));
        }
        s.sample();
        std::thread::sleep(std::time::Duration::from_millis(2100)); // let the process table get CPU deltas
        let mut snap = s.sample();
        // JMON_SECS=N: keep sampling 1/s and print the watched process (start the model AFTER the test).
        for _ in 0..std::env::var("JMON_SECS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0)
        {
            std::thread::sleep(std::time::Duration::from_secs(1));
            snap = s.sample();
            if let Some(w) = &snap.watched {
                eprintln!(
                    "  t gpu {:5.1}% @{:>4} MHz  vin {:5.2} W  tj {:4.1}C  fan {:?}  | pid {:?} cpu {:?}% rss {:?} KiB  model mem est {:?} KiB",
                    snap.gpu.as_ref().map_or(0.0, |g| g.load_pct),
                    snap.gpu.as_ref().map_or(0, |g| g.cur_mhz),
                    snap.rails.first().map_or(0.0, |r| r.watts),
                    snap.temps.iter().find(|t| t.name == "tj").map_or(0.0, |t| t.celsius),
                    snap.fan.as_ref().and_then(|f| f.rpm),
                    w.proc.as_ref().map(|p| p.pid),
                    w.proc.as_ref().map(|p| p.cpu_pct.round()),
                    w.proc.as_ref().map(|p| p.rss_kib),
                    w.mem_est_kib
                );
            }
        }
        assert_eq!(snap.cpus.len(), 6);
        assert!(snap.cpu_total_pct > 0.0 && snap.cpu_total_pct <= 100.0);
        assert_eq!(snap.rails.len(), 3);
        assert!(
            snap.rails[0].watts > 2.0 && snap.rails[0].watts < 30.0,
            "VDD_IN {}",
            snap.rails[0].watts
        );
        assert!(snap.fan.as_ref().is_some_and(|f| f.rpm.is_some()));
        assert!(snap.gpu.is_some() && snap.power_mode.is_some() && snap.temps.len() >= 6);
        for p in snap.procs.iter().take(5) {
            eprintln!(
                "  proc {:>7} {:<12} {:>5.1}% {:>8} KiB gpu {:?}  {:.60}",
                p.pid, p.user, p.cpu_pct, p.rss_kib, p.gpu, p.cmd
            );
        }
        if let Some(w) = &snap.watched {
            eprintln!(
                "  watched {} -> {:?} model mem est {:?} KiB",
                w.target,
                w.proc.as_ref().map(|p| (p.pid, p.rss_kib, p.gpu)),
                w.mem_est_kib
            );
        }
        eprintln!(
            "  swap in/out {:.0}/{:.0} pages/s majflt {:.0}/s zram {}/{} KiB file {}/{} KiB",
            snap.swap.in_pages_s,
            snap.swap.out_pages_s,
            snap.swap.majflt_s,
            snap.swap.zram_used_kib,
            snap.swap.zram_total_kib,
            snap.swap.file_used_kib,
            snap.swap.file_total_kib
        );
        eprintln!("live: cpu {:.1}% vin {:.2} W fan {:?} mode {:?} jtop {} tegrastats {} self {:.2}% {:.1} MiB",
            snap.cpu_total_pct, snap.rails[0].watts, snap.fan.as_ref().and_then(|f| f.rpm), snap.power_mode, snap.jtop_running, snap.tegrastats_running, snap.self_cpu_pct, snap.self_rss_mib);
    }

    #[test]
    fn clock_states() {
        let cpus = |min, max| {
            (0..6)
                .map(|id| Cpu {
                    id,
                    online: true,
                    min_mhz: min,
                    max_mhz: max,
                    hw_max_mhz: 1728,
                    ..Default::default()
                })
                .collect::<Vec<_>>()
        };
        let gpu = |min, max| Gpu {
            min_mhz: min,
            max_mhz: max,
            steps_mhz: vec![306, 612, 1020],
            ..Default::default()
        };
        assert_eq!(
            ClockState::classify(&cpus(729, 1728), Some(&gpu(306, 1020))),
            ClockState::Dynamic
        );
        assert_eq!(
            ClockState::classify(&cpus(1728, 1728), Some(&gpu(1020, 1020))),
            ClockState::MaxLocked {
                cpu_mhz: 1728,
                gpu_mhz: 1020
            }
        );
        assert_eq!(
            ClockState::classify(&cpus(1728, 1728), Some(&gpu(612, 612))),
            ClockState::Locked {
                cpu_mhz: 1728,
                gpu_mhz: 612
            }
        );
        assert_eq!(
            ClockState::classify(&cpus(1728, 1728), Some(&gpu(306, 1020))),
            ClockState::Dynamic
        ); // GPU still free
    }

    #[test]
    fn mode_names() {
        let m = parse_mode_names(
            "< POWER_MODEL ID=0 NAME=15W >\nfoo\n< POWER_MODEL ID=2 NAME=MAXN_SUPER >",
        );
        assert_eq!(
            m,
            vec![(0, "15W".to_string()), (2, "MAXN_SUPER".to_string())]
        );
    }
}
