//! Data contract between the logic half (sensors/app) and the UI half (ui/).
//! The UI only READS these types; it never touches sysfs.

use std::collections::VecDeque;

/// One full reading of the board. `None` = sensor absent or unreadable (e.g. root-only).
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub cpus: Vec<Cpu>,
    pub cpu_total_pct: f64,
    pub load_avg: [f64; 3],
    pub gpu: Option<Gpu>,
    /// Video/vision engine clocks, e.g. ("VIC", 435). Busy % is root-only and not provided.
    pub engines: Vec<(String, u32)>,
    pub mem: Mem,
    /// Memory-bus clock; always None without root (debugfs).
    pub emc_mhz: Option<u32>,
    /// INA3221 rails in hardware order: VDD_IN, VDD_CPU_GPU_CV, VDD_SOC. Never sum them.
    pub rails: Vec<Rail>,
    /// Only zones that report a temperature (cv0..cv2 are skipped on Orin Nano).
    pub temps: Vec<Temp>,
    pub fan: Option<Fan>,
    /// Whether CPU+GPU clocks are pinned (jetson_clocks or a custom lock). EMC is not checked (root-only).
    pub clocks: ClockState,
    /// soctherm over-current event counters (oc1..ocN). Rising = hardware throttled.
    pub oc_events: Vec<u64>,
    /// nvpmodel mode id from /var/lib/nvpmodel/status, name from /etc/nvpmodel.conf.
    pub power_mode: Option<PowerMode>,
    /// Swap traffic + per-tier usage (zram first, then the NVMe swapfile).
    pub swap: SwapStats,
    /// Process table, sorted by RSS (desc), top 64. Empty in bench mode (scan off).
    pub procs: Vec<Proc>,
    /// --watch NAME / --pid N target (read every sample, also in bench mode).
    pub watched: Option<Watched>,
    /// --workload FILE stats (filled by the sampler thread).
    pub workload: Option<Workload>,
    pub jtop_running: bool,
    pub tegrastats_running: bool,
    /// The monitor's own cost.
    pub self_cpu_pct: f64,
    pub self_rss_mib: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Cpu {
    pub id: usize,
    pub online: bool,
    pub util_pct: f64,
    pub cur_mhz: u32,
    pub min_mhz: u32,
    pub max_mhz: u32,
    /// Hardware ceiling (cpuinfo_max_freq) — what jetson_clocks pins to.
    pub hw_max_mhz: u32,
    pub governor: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClockState {
    /// Governors free to scale (normal).
    #[default]
    Dynamic,
    /// Every online core and the GPU pinned to their maximum = what `jetson_clocks` does.
    MaxLocked { cpu_mhz: u32, gpu_mhz: u32 },
    /// Pinned below max, e.g. the 1728/612 campaign lock.
    Locked { cpu_mhz: u32, gpu_mhz: u32 },
}

impl ClockState {
    /// Pinned = min == max for every online core and for the GPU.
    pub fn classify(cpus: &[Cpu], gpu: Option<&Gpu>) -> Self {
        let online: Vec<&Cpu> = cpus.iter().filter(|c| c.online).collect();
        let Some(g) = gpu else { return Self::Dynamic };
        if online.is_empty()
            || g.min_mhz != g.max_mhz
            || online.iter().any(|c| c.min_mhz != c.max_mhz)
        {
            return Self::Dynamic;
        }
        let cpu_mhz = online[0].min_mhz;
        let gpu_top = g.steps_mhz.last().copied().unwrap_or(g.max_mhz);
        if online.iter().all(|c| c.min_mhz == c.hw_max_mhz) && g.min_mhz == gpu_top {
            Self::MaxLocked {
                cpu_mhz,
                gpu_mhz: g.min_mhz,
            }
        } else {
            Self::Locked {
                cpu_mhz,
                gpu_mhz: g.min_mhz,
            }
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Dynamic => "CLOCKS dynamic".into(),
            Self::MaxLocked { cpu_mhz, gpu_mhz } => {
                format!("CLOCKS jetson_clocks {cpu_mhz}/{gpu_mhz}")
            }
            Self::Locked { cpu_mhz, gpu_mhz } => format!("CLOCKS locked {cpu_mhz}/{gpu_mhz}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Gpu {
    pub load_pct: f64,
    pub cur_mhz: u32,
    pub min_mhz: u32,
    pub max_mhz: u32,
    pub governor: String,
    /// Available steps in MHz, ascending (306..1020 on Orin Nano Super).
    pub steps_mhz: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Mem {
    pub total_kib: u64,
    pub avail_kib: u64,
    pub cached_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

impl Mem {
    pub fn used_kib(&self) -> u64 {
        self.total_kib.saturating_sub(self.avail_kib)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SwapStats {
    /// Pages/s swapped in/out (×4 KiB). Non-zero during a measured run = latency hit.
    pub in_pages_s: f64,
    pub out_pages_s: f64,
    /// Major page faults/s (disk reads); spikes mid-run are bad, at model load normal.
    pub majflt_s: f64,
    pub zram_used_kib: u64,
    pub zram_total_kib: u64,
    pub file_used_kib: u64,
    pub file_total_kib: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GpuUse {
    /// Has /dev/nvgpu/* or /dev/nvmap open.
    Yes,
    #[default]
    No,
    /// fds not readable (other user's/root process without sudo) or not checked (small RSS).
    Unknown,
}

/// One process. RSS = CPU-visible memory only. Whether GPU buffers show up in RSS depends on the
/// runtime: PyTorch cudaMalloc did (+1024 MiB → +1032 MiB RssAnon), llama.cpp `-ngl 99` did NOT
/// (1.33 GiB real cost, steady RSS 0.73 GiB, 0 MiB in nvmap mappings). Never label RSS as "incl. GPU".
/// Per-process GPU % does not exist on Orin.
#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub pid: u32,
    pub user: String,
    pub cpu_pct: f64,
    pub rss_kib: u64,
    pub swap_kib: u64,
    pub gpu: GpuUse,
    pub cmd: String,
}

/// The process chosen with --watch NAME / --pid N (the model under test).
#[derive(Clone, Debug, Default)]
pub struct Watched {
    /// What the user asked for, e.g. "llama-server" or "pid 1234".
    pub target: String,
    /// None = not running right now.
    pub proc: Option<Proc>,
    /// Times a new PID matched after the previous one exited.
    pub restarts: u32,
    /// Model memory estimate, all runtimes incl. GPU buffers: system MemAvailable in the sample
    /// before this PID appeared minus MemAvailable now. System-wide (anything else allocating at the
    /// same time is counted too). None = the PID already existed when jmon started → no baseline.
    pub mem_est_kib: Option<i64>,
}

/// Clock drops while clocks are locked, since monitor start (sampled — sub-sample drops can be missed).
#[derive(Clone, Debug, Default)]
pub struct Throttle {
    pub cpu: DropStats,
    pub gpu: DropStats,
    /// Sum of soctherm OC counter increases since start.
    pub oc_rise: u64,
}

#[derive(Clone, Debug, Default)]
pub struct DropStats {
    /// Number of separate episodes below the locked target.
    pub drops: u32,
    pub below_s: f64,
    pub longest_s: f64,
    /// Currently below target (episode in progress).
    pub active: bool,
    /// Length of the episode in progress.
    pub current_s: f64,
}

/// Model-side numbers from the --workload file (key=value lines), over a rolling window.
#[derive(Clone, Debug, Default)]
pub struct Workload {
    pub path: String,
    /// inferences/s = Σbatch / window span.
    pub inf_per_s: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub samples_in_window: usize,
    /// Window length in seconds (--window, default 10).
    pub window_s: f64,
    /// LLMs: per-token generation latency percentiles from `tok_ms=` (None if the writer doesn't send it).
    pub tok_ms_p50: Option<f64>,
    pub tok_ms_p95: Option<f64>,
    /// Latency > --deadline-ms: in window / since start. None = no deadline set.
    pub deadline_ms: Option<f64>,
    pub misses_window: u64,
    pub misses_total: u64,
    pub total_lines: u64,
    pub bad_lines: u64,
    /// Latest engine=… value.
    pub engine: String,
    pub batch: u32,
    /// Mean stage times over the window, if the writer reports stage_* keys.
    pub stages: Vec<(String, f64)>,
    /// inf/s ÷ VDD_IN W (board-level live estimate, includes idle draw). None if no rail/zero.
    pub inf_per_watt: Option<f64>,
    /// Seconds since the last line.
    pub idle_s: f64,
    /// Median gap between line arrivals in the window (0 until two lines). LLM requests are seconds apart.
    pub typical_gap_s: f64,
}

impl Workload {
    /// No line for longer than 3× the usual gap (at least 3 s) — so a 4 s-per-request LLM isn't "stalled".
    pub fn stalled(&self) -> bool {
        self.total_lines > 0 && self.idle_s > (3.0 * self.typical_gap_s).max(3.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Rail {
    pub name: String,
    pub volts: f64,
    pub amps: f64,
    pub watts: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Temp {
    pub name: String,
    pub celsius: f64,
    /// Lowest "real" trip (e.g. 99 °C SW) and the highest (104.5 °C HW), if exposed.
    pub trip_sw: Option<f64>,
    pub trip_hw: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Fan {
    /// Raw duty 0..=255.
    pub pwm: u8,
    pub pwm_pct: f64,
    /// Tachometer; None if no tach sensor. Some(0) with pwm > 0 = stalled.
    pub rpm: Option<u32>,
}

impl Fan {
    pub fn stalled(&self) -> bool {
        self.pwm > 0 && self.rpm == Some(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerMode {
    pub id: u32,
    pub name: String,
}

/// Fixed-length history for charts/sparklines (oldest first).
#[derive(Clone, Debug)]
pub struct Series {
    pub cap: usize,
    pub data: VecDeque<f64>,
}

impl Series {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            data: VecDeque::with_capacity(cap),
        }
    }
    pub fn push(&mut self, v: f64) {
        if self.data.len() == self.cap {
            self.data.pop_front();
        }
        self.data.push_back(v);
    }
    /// Ignores missing samples (NaN = sensor absent at that tick).
    pub fn max(&self) -> f64 {
        self.data
            .iter()
            .cloned()
            .filter(|v| v.is_finite())
            .fold(0.0, f64::max)
    }
    pub fn avg(&self) -> f64 {
        let (sum, n) = self
            .data
            .iter()
            .filter(|v| v.is_finite())
            .fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    }
}

/// Last ~60 s of the headline signals. A missing reading is stored as NaN, never 0.
#[derive(Clone, Debug)]
pub struct History {
    pub cpu_total: Series,
    pub gpu_load: Series,
    pub vdd_in_w: Series,
    pub fan_rpm: Series,
    pub tj_c: Series,
    pub mem_used_gib: Series,
    pub swap_out_pages_s: Series,
    /// Watched process RSS in GiB (NaN when not running / no --watch).
    pub watched_rss_gib: Series,
    /// Watched model memory estimate (Watched::mem_est_kib) in GiB, NaN when unknown.
    pub model_mem_gib: Series,
    pub inf_per_s: Series,
    pub p95_ms: Series,
}

impl History {
    pub fn new(cap: usize) -> Self {
        Self {
            cpu_total: Series::new(cap),
            gpu_load: Series::new(cap),
            vdd_in_w: Series::new(cap),
            fan_rpm: Series::new(cap),
            tj_c: Series::new(cap),
            mem_used_gib: Series::new(cap),
            swap_out_pages_s: Series::new(cap),
            watched_rss_gib: Series::new(cap),
            model_mem_gib: Series::new(cap),
            inf_per_s: Series::new(cap),
            p95_ms: Series::new(cap),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Ok,
    Bad,
    Unknown,
}

/// One row of the BENCHMARK checklist.
#[derive(Clone, Debug)]
pub struct Check {
    pub name: &'static str,
    pub expected: String,
    pub observed: String,
    pub state: CheckState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Bad,
}

/// Drift log entry; `at` = seconds since the monitor started.
#[derive(Clone, Debug)]
pub struct Event {
    pub at_s: u64,
    pub level: Level,
    pub msg: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Power,
    Thermal,
    Processes,
    Benchmark,
}
