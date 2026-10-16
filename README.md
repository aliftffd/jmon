# jmon

jmon is a btop/nvtop-style terminal hardware monitor for NVIDIA Jetson, developed and tested on the Orin Nano. It is written in Rust with ratatui and shows CPU, GPU, memory, power, cooling, processes, and model workload statistics in one terminal. Monitoring is read-only, needs no root privileges, and uses about 2 MB of RAM (PSS). Benchmark checks help you see whether the board still matches your chosen test conditions; jmon never changes hardware settings.

Not affiliated with or endorsed by NVIDIA.

## Screenshots

Placeholders; the images will be added later.

![Overview tab](https://raw.githubusercontent.com/aliftffd/jmon/main/docs/overview.png)

![Processes tab](https://raw.githubusercontent.com/aliftffd/jmon/main/docs/processes.png)

![Power tab](https://raw.githubusercontent.com/aliftffd/jmon/main/docs/power.png)

## Features

### 1 — Overview

- CPU utilization and clock speed per core, online/offline state, total CPU load, and load averages.
- GPU load, current clock, frequency range, and governor; discovered video/vision engine clocks.
- Shared CPU/GPU RAM, available memory, change since monitor start, and separate zram and NVMe swap tiers. Swap-in/out rates and major page faults help reveal memory pressure.
- Power, temperatures, and fan RPM/PWM, with an animated fan when there is room.
- A watched model panel with PID, CPU use, CPU-visible RSS, swap, restarts, and a system-memory-based model estimate.
- A workload summary with throughput, request latency, and, when supplied, tokens/s and ms/token.

### 2 — Power

- INA3221 rail readings in watts, volts, and amps: `VDD_IN`, `VDD_CPU_GPU_CV`, and `VDD_SOC`.
- Board input-power history from `VDD_IN`. **The rails are never summed:** these are separate, overlapping measurements, not additive parts of a total.
- Full workload details beside power: throughput, request p50/p95/p99, token-latency p50/p95, deadline misses, stage timings, and log status.
- Throughput per watt using live `VDD_IN`, including idle draw. This is a board-level live estimate, not an energy measurement for a campaign.

### 3 — Thermal

- Thermal zones that report valid temperatures, with exposed software/hardware trip points and temperature history.
- Fan RPM, PWM duty as a percentage and raw 0–255 value, animated fan art, and a stall alert when PWM is nonzero but RPM is zero.
- CPU/GPU clock-drop episodes while clocks are locked: count, total time below the lock, longest episode, and increases in hardware over-current (OC) event counters.

### 4 — Processes

- PID, user, GPU flag, CPU%, CPU-visible RSS, swap, and command line; switch between memory and CPU sorting.
- Up to 64 processes selected by largest RSS, refreshed about every two seconds. CPU sorting reorders those selected processes.
- A GPU flag indicating an open GPU device, rather than a per-process GPU utilization percentage.
- Watched model details and a highlighted row for its PID. The watched process is sampled every hardware sample, including in benchmark mode.

### 5 — Benchmark

- Six benchmark-condition checks against your expected power mode and CPU/GPU clocks, plus competing monitors and thermal/OC conditions.
- Expected versus observed values and a drift log for failures, recoveries, watched-process changes, and workload-engine changes.
- Sampling profile, live power, fan information, and throttle/OC counters.

The common header shows the power mode and detected clock state: `dynamic`, `jetson_clocks` (CPU and GPU pinned to their maximums), or `locked` (pinned below maximum). The label describes observed clock limits; it does not prove which tool set them. EMC is excluded from this classification.

## Tested hardware

Tested on the **Jetson Orin Nano Super Developer Kit**, running **JetPack 6.2.3 (L4T R36.5.2)**.

Other Orin modules and JetPack versions are untested. Sensors are discovered by name, so they may work where the same interfaces are exposed. Please report issues with your module, JetPack/L4T version, and the missing or unexpected readings.

## Installation on the Jetson

Run these steps in a terminal **on the Jetson**, either locally or over SSH. The commands assume the source lives at `~/jetson-monitor`. Administrator access is used to install build tools; running jmon itself needs no sudo.

### a. Check prerequisites

Confirm that the device has a Jetson Linux installation:

```sh
cat /etc/nv_tegra_release
```

The tested board reports R36, revision 5.2. Check available disk space:

```sh
df -h "$HOME"
```

Allow roughly **1 GB for the Rust toolchain**, plus space for source, downloaded dependencies, and build output. You also need internet access to the Ubuntu package mirrors, Rust servers, crates.io, and your source repository host. This checks DNS resolution for the Rust download hosts:

```sh
getent hosts sh.rustup.rs static.rust-lang.org crates.io
```

### b. Install build tools

```sh
sudo apt update && sudo apt install -y build-essential pkg-config curl git
```

Confirm HTTPS access to the Rust installer before continuing:

```sh
curl -I https://sh.rustup.rs
```

### c. Install Rust with rustup

Install the default stable toolchain for the Jetson's ARM64 CPU:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustc --version
```

The version command should print the installed Rust version. In a new shell, load the Cargo environment again if needed.

**Non-interactive SSH commands do not necessarily load `~/.cargo/bin` into PATH.** Source the environment in that command, or use the full path to Cargo:

```sh
source ~/.cargo/env
cargo --version
```

```sh
"$HOME/.cargo/bin/cargo" --version
```

### d. Get the source

**Quickest — install from crates.io** (needs Rust from step c; installs `jmon` into `~/.cargo/bin`, already on your PATH after `source ~/.cargo/env`):

```sh
cargo install jmon
jmon
```

If you install this way you can skip steps e and f. Put your benchmark targets in `~/.config/jmon/expected.conf` (see *Benchmark mode & expected.conf*). Update later with `cargo install jmon --force`.

**Or build from source** (needed to run the tests or `--root fixture`):

```sh
git clone https://github.com/aliftffd/jmon.git ~/jetson-monitor
```

Alternatively, if you already have the folder on your laptop, copy it to the Jetson. Run this **on the laptop**, replacing `<jetson>` with an SSH destination such as `your-user@192.168.1.50`:

```sh
scp -r ~/jetson-monitor '<jetson>:~/'
```

Include `Cargo.toml`, `Cargo.lock`, `src/`, `fixture/`, `examples/`, and `expected.conf`. An existing `target/` directory can be large and is unnecessary to transfer; jmon will be built on the Jetson. Continue the remaining steps on the Jetson.

### e. Build and test

```sh
cd ~/jetson-monitor && cargo build --release
```

Allow about **40 seconds on an Orin Nano after dependencies have downloaded**; the first build can take longer. The executable is `target/release/jmon`.

Run the regular test suite:

```sh
cargo test --release
```

Optionally, run the read-only sensor check against the actual Jetson board. This test is ignored by the regular suite and expects the tested board's sensors:

```sh
cargo test --release -- --ignored live_board --nocapture
```

Try the monitor from the project directory:

```sh
./target/release/jmon
```

Press **q** or **Esc** to exit.

### f. Install the command

Create a small wrapper so you can launch jmon from any directory. It changes to the project directory before running the executable, which lets jmon find `./expected.conf`:

```sh
mkdir -p ~/.local/bin
cat > ~/.local/bin/jmon <<'EOF'
#!/bin/sh
cd "$HOME/jetson-monitor" && exec ./target/release/jmon "$@"
EOF
chmod +x ~/.local/bin/jmon
```

Ubuntu's default profile adds `~/.local/bin` to PATH at the next login once that directory exists. Load the profile now and check the command:

```sh
source ~/.profile
command -v jmon
jmon
```

The wrapper uses the executable in the source folder, so keep that folder in place. Relative paths passed through the wrapper are also resolved from `~/jetson-monitor`; use absolute paths for logs or configuration stored elsewhere.

### g. Update

For a Git checkout, pull the latest source and rebuild on the Jetson:

```sh
source ~/.cargo/env
cd ~/jetson-monitor && git pull && cargo build --release
```

Exit and restart jmon to use the new build. The wrapper needs no update. If you copied a source snapshot without Git metadata, copy the updated source and repeat the build step.

### h. Uninstall

These commands remove the wrapper and the project folder, including its build output and any local configuration edits:

```sh
rm -f "$HOME/.local/bin/jmon"
rm -rf "$HOME/jetson-monitor"
```

If you installed Rust only for jmon, you can optionally remove the Rust toolchain too:

```sh
rustup self uninstall
```

## Remote viewing from a laptop

After installing the wrapper on the Jetson, run this on your laptop. Replace `<jetson>` with the Jetson's SSH hostname or user/address:

```sh
ssh -t <jetson> jmon
```

The **`-t` is required for the TUI**: it allocates a terminal for the remote command. If the remote shell cannot find the wrapper, call it explicitly:

```sh
ssh -t <jetson> '~/.local/bin/jmon'
```

A truecolor terminal is recommended; a 256-color terminal works. Use at least **80 columns × 24 rows**; **160 × 45** gives the best view of charts and panels. Resize the laptop terminal to change the remote layout. Only the terminal display runs on your laptop; monitoring and workload-log reads happen on the Jetson.

## Usage

```text
jmon [--root DIR] [--expected FILE] [--bench]
     [--watch NAME | --pid N] [--workload FILE]
     [--deadline-ms MS] [--window SECONDS]
```

Use a space between a flag and its value. Choose one of `--watch` or `--pid`; if both are supplied, the last one takes effect.

| Flag | Meaning |
| --- | --- |
| `--root DIR` | Read board sysfs/procfs and related files under `DIR` instead of `/`, for example a captured fixture. jmon's own overhead still comes from the real process. |
| `--expected FILE` | Load benchmark targets from this file. Without it, jmon uses the first file that exists of `./expected.conf`, `~/.config/jmon/expected.conf` (or `$XDG_CONFIG_HOME/jmon/expected.conf`), `/etc/jmon/expected.conf`; otherwise the built-in targets: mode 0 / 15W, CPU 1728 MHz, GPU 612 MHz. |
| `--bench` | Start with the benchmark preset: 1 Hz sampling, process table off, fan animation paused. It starts on Overview; press **5** to view the checks. |
| `--watch NAME` | Follow a process whose command line contains `NAME`, and find a new matching PID after it exits. Use a distinctive substring when several processes may match. |
| `--pid N` | Follow a specific numeric PID. It does not follow a replacement process with a new PID. |
| `--workload FILE` | Follow newly written workload log lines containing `lat_ms` and optional metrics. The file may be created after jmon starts. |
| `--deadline-ms MS` | With a workload log, count request/batch latencies strictly greater than `MS` as deadline misses. No deadline is set by default. |
| `--window SECONDS` | Set the workload rolling window, default 10 seconds. Values must be at least 1 second; fractional seconds are accepted. Use 30–60 seconds for LLMs. |

Normal monitoring:

```sh
jmon
```

Benchmark preset with an explicit configuration:

```sh
jmon --bench --expected "$HOME/jetson-monitor/expected.conf"
```

Follow a model and its latency log:

```sh
jmon --watch my-model --workload "$HOME/model-workload.log" --deadline-ms 50 --window 10
```

Attach to an existing process, replacing the example PID:

```sh
jmon --pid 1234
```

Run against the included captured board data:

```sh
cd ~/jetson-monitor
./target/release/jmon --root fixture
```

### Key bindings

| Key | Action |
| --- | --- |
| **1–5** | Select Overview, Power, Thermal, Processes, or Benchmark. Selecting tab 5 alone does not change the sampling preset. |
| **b** | Toggle benchmark mode. Enabling it selects tab 5 and resets sampling to 1 Hz; disabling it resets sampling to 2 Hz. |
| **Space** | Freeze/unfreeze the displayed snapshot. Sampling, workload following, history, and condition checks continue. |
| **+** | Halve the sample interval, down to 250 ms (4 Hz). |
| **-** | Double the sample interval, up to 4000 ms (0.25 Hz). |
| **s** | Toggle the Processes table between descending RSS and descending CPU%. |
| **q** / **Esc** | Quit and restore the terminal. |

Normal mode starts at 500 ms per sample (2 Hz). The **+/-** controls also work in benchmark mode, so they can override its initial 1 Hz interval.

## Benchmark mode & expected.conf

Benchmark mode is a monitoring preset for an experiment you run separately. It starts at **1 Hz**, disables the general process table, and pauses fan animation. Hardware readings, the watched model, workload following, condition checks, and drift tracking remain active. It does not start a model or configure the board.

### The six checks

| Check | What passes |
| --- | --- |
| Power profile | The observed nvpmodel mode ID equals `power_mode_id`. `power_mode_name` is the expected display label; the name itself is not compared. An unreadable mode is UNKNOWN. |
| CPU lock | Every online core's current clock equals `cpu_mhz`, and each core's minimum and maximum limits are equal. No online CPU readings means UNKNOWN. |
| GPU lock | The current GPU clock equals `gpu_mhz`, and its minimum and maximum limits are equal. A missing GPU reading means UNKNOWN. |
| jtop.service | No jtop process is detected. Detection examines process command lines, including the service process. |
| tegrastats | No tegrastats process is detected. |
| Thermal | No exposed thermal zone has reached its software trip point, and the OC event counters have not increased since jmon's initial sample. |

The overall banner is **NOT OK** if any check fails, **UNKNOWN** if a check is unknown and none fail, and **OK** when all six pass. An OC increase keeps the Thermal check failed for that monitor session even after the board cools. The Thermal check evaluates only available zones/counters and does not become UNKNOWN when they are absent. Process detection normally updates about every five seconds and can also refresh during process-table scans.

The separate clock-drop counters measure sampled episodes below pinned CPU/GPU limits. The drift log records check-state transitions, including restored conditions, with newest events first. It is an in-memory session log.

### Configuration format

The supplied [expected.conf](expected.conf) contains:

```ini
# Targets for the 15 W campaign
power_mode_id = 0
power_mode_name = 15W
cpu_mhz = 1728
gpu_mhz = 612
```

Use one `key=value` pair per line; spaces around the key and value are allowed. Empty lines and lines starting with `#` are ignored. Use unquoted text for the mode name and whole numbers for the ID and MHz values. Unknown keys and invalid numeric values are ignored; missing values retain the defaults above. Comments belong on their own lines.

| Key | Meaning |
| --- | --- |
| `power_mode_id` | Numeric nvpmodel mode ID that must match. |
| `power_mode_name` | Human-readable name shown as the expected profile. |
| `cpu_mhz` | Expected current clock for every online CPU core, in MHz, with equal min/max limits. |
| `gpu_mhz` | Expected current GPU clock in MHz, with equal min/max limits. |

To use your own file:

```sh
jmon --bench --expected "$HOME/campaign.conf"
```

The file is read at startup. Restart jmon after editing it. If a specified file is unreadable, jmon silently falls back to the built-in defaults, so check the TARGET line on tab 5.

### Setting the board's clocks

**Setting power modes and locking clocks is your responsibility. jmon never changes hardware settings.** For the supplied campaign on the tested board, select mode 0 first:

```sh
sudo nvpmodel -m 0
```

Then run your own clock-lock script to pin the CPU to **1728 MHz** and GPU to **612 MHz**, and inspect tab 5. There is no clock-lock script bundled with jmon. Mode IDs and available frequencies depend on the board's configuration.

The NVIDIA `jetson_clocks` utility locks clocks to **MAX**, including **1020 MHz GPU** on the tested Orin Nano Super. That differs from the supplied **612 MHz** GPU campaign target and will fail that target's GPU check. Choose the hardware settings for your campaign, and make `expected.conf` describe those settings.

## Monitoring a model

### Follow the process and establish a memory baseline

**START JMON BEFORE THE MODEL.** Let jmon collect at least one sample while the target is absent, then launch the model in another terminal:

```sh
jmon --watch my-model
```

`--watch NAME` matches a command-line substring and follows restarts. A specific PID can be selected instead:

```sh
jmon --pid 1234
```

Attaching to a process that already exists still gives CPU, RSS, swap, and GPU-flag readings, but its model-memory estimate is `n/a`: there was no sample before the PID appeared. Restart the model while jmon watches its name to get a new baseline.

The model-memory estimate is:

```text
MODEL MEM ≈ system MemAvailable in the sample before this PID appeared
            − system MemAvailable now
```

This includes GPU buffers that consume shared system memory. It is a **system-wide estimate**: other allocations, frees, and cache changes also affect it, and it can be negative. Keep unrelated activity steady when comparing models.

RSS is **CPU-visible memory only**. Board checks verified that llama.cpp GPU weights are not included in RSS, while PyTorch GPU allocations are reflected in RSS. RSS therefore cannot be treated as a consistent measure of total model memory across runtimes.

Per-process GPU% is unavailable on Orin; `nvidia-smi` reports N/A. jmon's GPU flag checks for open GPU device files: **●** means detected, **?** means the file descriptors could not be inspected, and a blank means no GPU device was detected or the process was below the general table's inspection threshold.

### Write a workload log

Use [src/workload.rs](src/workload.rs) as the format reference. Write **one whitespace-separated `key=value` line per completed request or batch**, ending with a newline:

```text
ts=8251.7 lat_ms=1.19 batch=1 engine=v5_fp16 stage_pre=0.21 stage_inf=0.86 stage_post=0.12
```

| Field | Required? | Meaning |
| --- | --- | --- |
| `lat_ms` | Yes | Completed request/batch latency in milliseconds, as a finite, nonnegative number. |
| `batch` | No | Positive integer work count, default 1. Use batch size for inference; use generated-token count for the LLM example. |
| `tok_ms` | No | Generation milliseconds per token, finite and nonnegative. Enables ms/token p50/p95 and the tok/s label. |
| `engine` | No | A label without spaces, such as a model or precision name. The latest supplied label is retained. |
| `stage_*` | No | Stage durations in milliseconds, such as `stage_pre`, `stage_inf`, `stage_post`, `stage_prompt`, or `stage_gen`. jmon shows each stage's mean over the window. |
| `ts` | No | Accepted but unused. Windowing uses line arrival time, not this timestamp. |

Unknown keys are ignored. Missing/invalid `lat_ms` or an invalid `batch` makes a line bad; the Power tab shows the bad-line count. Finish timing **after GPU synchronization**, and **flush every line** so buffering does not distort arrival times. jmon reads the log; your model process writes it.

For a generic PyTorch/TensorRT loop, import `time`, open `log` once as an append-mode text file, and set `batch_size` to the number of inputs. In the four lines below, `infer(inputs)` is your inference call and `gpu_synchronize()` waits for all measured GPU work: use `torch.cuda.synchronize` for PyTorch, or synchronization of the CUDA stream used by your TensorRT runner. Put this inside your existing loop:

```python
gpu_synchronize(); started = time.perf_counter()
outputs = infer(inputs)
gpu_synchronize()
print(f"lat_ms={(time.perf_counter() - started) * 1000:.3f} batch={batch_size} engine=my_model", file=log, flush=True)
```

Follow the same file on the Jetson:

```sh
jmon --watch my-model --workload "$HOME/model-workload.log" --window 10 --deadline-ms 50
```

The rolling statistics include throughput (sum of `batch` divided by the covered time), request-latency p50/p95/p99, and optional token-latency p50/p95. p50 is the median; p95 and p99 describe the slower tail. Deadline misses compare **`lat_ms`**, not `tok_ms`, with the supplied threshold and are shown for the current window and the whole session.

At startup, an existing log is followed from its end, so old runs are skipped. jmon can wait for a missing file and follows later creation, rotation, or truncation from the start. It processes complete newline-terminated lines. The workload panel appears on Overview, with full details on Power. A stalled warning means no new line has arrived for longer than three times the typical gap, with a minimum threshold of three seconds.

### LLM quick start with llama.cpp

The included [examples/llm_jmon.py](examples/llm_jmon.py) starts a CUDA llama.cpp server, sends prompts, writes one workload line per completed request, and stops its server when finished.

You need Python 3, a GGUF model that fits your board, and **llama.cpp already built with CUDA** with its executable at:

```text
~/llama.cpp/build/bin/llama-server
```

jmon does not build llama.cpp or download a model. The helper uses Python's standard library. Check the interpreter and server path:

```sh
python3 --version
test -x "$HOME/llama.cpp/build/bin/llama-server"
```

Use two terminals on the Jetson (two SSH sessions are fine). Begin with no other llama-server instance running so the watch selects this run.

**Terminal 1 — start jmon first**, then wait until it shows readings and the watched model is not running:

```sh
jmon --watch llama-server --workload "$HOME/jmon-llm.log" --window 60
```

**Terminal 2 — run the helper**, replacing the example model path with your GGUF file:

```sh
python3 "$HOME/jetson-monitor/examples/llm_jmon.py" \
  "$HOME/models/model.gguf" --requests 30 --tokens 128
```

Use **`--window 60`** in jmon for this example: LLM requests can take several seconds, and a longer window keeps statistics useful between completions. The helper defaults to 30 requests, at most 128 generated tokens per request, port 8091, and `~/jmon-llm.log`. Its `--requests`, `--tokens`, `--port`, and `--log` options change those values; if you change the log, pass the same path to jmon. Server output goes to `~/llama-server.log`. Both logs are overwritten for a new helper run.

The helper launches llama-server with 99 requested GPU layers and a 2048-token context, then waits for it to become ready. Its lines look like this:

```text
lat_ms=4200.0 batch=128 tok_ms=31.70 engine=my-model stage_prompt=142.4 stage_gen=4057.6
```

Here **`batch` is generated tokens**, so throughput reads as **tok/s**. `tok_ms` comes from the server's generation timing per token; request `lat_ms` includes the full request. Request p50/p95/p99 vary with answer length, so use ms/token when comparing generation speed across short and long answers. `stage_prompt` and `stage_gen` show prompt and generation time separately.

## Overhead

Measured on the Orin Nano Super:

| Measurement | Approximate cost |
| --- | --- |
| Proportional set size (PSS) | 1.9 MB |
| Resident set size (RSS) | 2.7 MB |
| Normal mode CPU | 1–2% of one CPU core |
| Benchmark mode | Lower overhead with 1 Hz sampling, no general process table, and paused fan animation |
| jtop service, for comparison | About 50 MB of memory |

PSS accounts proportionally for shared pages; RSS counts all resident pages mapped into the process. Actual cost varies with terminal size, sampling rate, process count, and workload-log traffic. The footer reports jmon's own CPU and RSS.

## Limitations / FAQ

- **Why are EMC and engine busy% n/a?** Their interfaces are root-only. This monitor leaves them unavailable; it does show readable engine clocks. Running with sudo does not add unimplemented counters.
- **Where is per-process GPU memory?** This JetPack has no `/sys/kernel/debug/nvmap` interface to use for it. Shared RAM, the model-memory estimate, and CPU-visible RSS answer different questions; use the estimate for an approximate model footprint.
- **Why does another user's process show a GPU `?`?** Without sudo, jmon may be unable to inspect that process's file descriptors. The rest of monitoring remains usable as your normal user.
- **Why can a small GPU process have a blank flag?** The general process table skips GPU-device inspection below 64 MiB of anonymous RSS (`RssAnon`). Thus small GPU users under 64 MiB RSS are not checked, and some processes with larger total RSS can also be skipped. An explicitly watched process is checked regardless of size.
- **Can short throttle events be missed?** Yes. Clock drops are sampled, counted only while CPU/GPU clocks are locked, and can be shorter than the sampling interval. OC counter increases provide an additional signal.
- **Are drift timestamps wall-clock times?** No. They are relative to this monitor session, formatted as `T+mm:ss`.
- **How much history is kept?** Charts retain 120 samples: about 60 seconds at normal 2 Hz or 120 seconds at the initial benchmark 1 Hz. The workload window is separate, defaults to 10 seconds, and retains at most 2000 entries even with a longer window.
- **Can jtop and jmon run together?** Yes. For benchmark campaigns, stop the jtop service to reduce interference and satisfy the jtop check. Closing only the jtop client may leave the service running:

```sh
sudo systemctl stop jtop
```

Stop any tegrastats run as well if you want all six checks to pass. jmon detects other monitors but does not stop them.

## Troubleshooting

### “jmon: command not found”

Reload the profile after creating the wrapper, or start a new login session:

```sh
source ~/.profile
command -v jmon
```

For a customized profile that does not add the directory, add it to the current shell's PATH and try again:

```sh
export PATH="$HOME/.local/bin:$PATH"
jmon
```

To bypass PATH entirely:

```sh
"$HOME/.local/bin/jmon"
```

If Cargo is the missing command, load its environment:

```sh
source ~/.cargo/env
```

### “~ not expanded after =”

In option forms such as `--flag=~/path`, shells may pass `~` literally; use `$HOME` when constructing such paths. **jmon itself only accepts space-separated flag values**, so avoid the equals form altogether:

```sh
jmon --expected "$HOME/jetson-monitor/expected.conf" --workload "$HOME/model-workload.log"
```

An unrecognized option prints usage and exits. A wrong expected-file path instead falls back to defaults; check the displayed targets. A wrong workload path can leave the panel waiting for its first line.

### nvpmodel shows n/a

jmon reads the mode ID from `/var/lib/nvpmodel/status`, expecting a value such as `pmode:0000`, and resolves its name from `/etc/nvpmodel.conf`. Inspect the files as your normal user:

```sh
cat /var/lib/nvpmodel/status
ls -l /var/lib/nvpmodel/status /etc/nvpmodel.conf
```

If the status file is missing, unreadable, or in a different format, the profile is unknown. A missing name mapping alone falls back to a label such as `mode 0`. Check the mode with NVIDIA's tool:

```sh
sudo nvpmodel -q
```

Report unexpected formats with your JetPack version. Changing `expected.conf` changes the target, not the observed mode. If using `--root`, the corresponding files must be under that root.

### Fan art is blank in a tiny window

The layout prioritizes numeric readings when there is insufficient space for the fan's 15 × 7 character art. Enlarge the terminal toward 160 × 45. In benchmark mode the art is deliberately static; the hardware fan continues operating under the board's own controller.

### SSH display fails or does not behave like a terminal

A remote command launched without a pseudo-terminal cannot host the TUI correctly. Reconnect with `-t`:

```sh
ssh -t <jetson> '~/.local/bin/jmon'
```

### Model memory is n/a or workload statistics stay empty

Start jmon before the model and allow a sample before launching it. For workload data, check that the writer and jmon use the same absolute path, every line contains valid `lat_ms`, each line ends with a newline, and the writer flushes. Existing lines present when jmon starts are skipped. Increase the workload window for long LLM requests.

## Development

| Path | Responsibility |
| --- | --- |
| [src/model.rs](src/model.rs) | Shared data contract for sensors, app state, and UI, including memory and metric semantics. |
| [src/sensors.rs](src/sensors.rs) | Sensor discovery, sysfs/procfs reads, process watching, and model-memory baseline. |
| [src/app.rs](src/app.rs) | Application state, history, expected targets, condition checks, drift events, and fan animation timing. |
| [src/workload.rs](src/workload.rs) | Workload log parsing/following and rolling statistics. |
| [src/main.rs](src/main.rs) | CLI usage and parsing, sampler thread, terminal event loop, and key bindings. |
| [src/ui/](src/ui/) | ratatui presentation for the five tabs and shared widgets. |
| [fixture/](fixture/) | Captured sysfs/procfs and configuration snapshot for offline sensor/UI tests. |
| [examples/llm_jmon.py](examples/llm_jmon.py) | llama.cpp workload writer and two-terminal example. |

The regular suite runs offline against `fixture/` and generated test data, without Jetson hardware. Dependencies must be downloaded for the first build; Cargo can then be forced offline. The real-board sensor test is ignored unless explicitly requested.

```sh
cd ~/jetson-monitor
cargo test
```

```sh
cargo test --offline
```

To run the UI on any Linux machine with Rust and a terminal, use the captured snapshot:

```sh
cd ~/jetson-monitor
cargo run -- --root fixture
```

Fixture values are static captured readings; this mode is useful for layout and missing-sensor behavior, not live host monitoring. jmon's own overhead still reflects the running process.

Formatting and Clippy must stay clean:

```sh
cargo fmt
cargo fmt --check
cargo clippy
```

## License

MIT — see [LICENSE](LICENSE).
