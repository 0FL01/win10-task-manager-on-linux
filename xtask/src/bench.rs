/* xtask/src/bench.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! `cargo xtask bench-idle`: compares idle resource usage of the wgpu and glow renderers
//! inside a nested virtual KWin compositor, so the user's desktop is not disturbed.

use std::collections::HashMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{dist, root};

const SOCKET_PREFIX: &str = "wayland-mcbench";
const MEMORY_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct Options {
    egui: PathBuf,
    egui_glow: PathBuf,
    seconds: u64,
    warmup: u64,
    repeats: usize,
    out: PathBuf,
}

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    EguiWgpu,
    EguiGlow,
}

impl Variant {
    fn name(&self) -> &'static str {
        match self {
            Variant::EguiWgpu => "egui-wgpu",
            Variant::EguiGlow => "egui-glow",
        }
    }
}

#[derive(Default, Clone)]
struct Sample {
    ui_cpu_percent: f64,
    daemon_cpu_percent: Option<f64>,
    memory: Vec<MemoryPoint>,
    vram_kib: Option<u64>,
    frames: Option<u64>,
    p95_ms: Option<f64>,
    readings: Option<u64>,
    valid: bool,
}

struct Row {
    scenario: String,
    variant: Variant,
    daemon: Option<PathBuf>,
    samples: Vec<Sample>,
}

struct Backend {
    daemon: PathBuf,
    hwdb: PathBuf,
    locale: Option<PathBuf>,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
struct Memory {
    rss_kib: Option<u64>,
    pss_kib: Option<u64>,
}

#[derive(Clone)]
struct MemoryPoint {
    seconds: f64,
    ui: Memory,
    daemon: Memory,
}

#[derive(Debug, PartialEq, Eq)]
struct ProcStat {
    ticks: u64,
    start_time: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct DaemonIdentity {
    pid: u32,
    start_time: u64,
}

/// Own spawned processes through every early-return path, including socket startup failures.
struct ManagedChild {
    child: Child,
    descendants: Vec<u32>,
}

impl ManagedChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            descendants: vec![],
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.descendants.extend(children(self.child.id()));
        self.descendants.sort_unstable();
        self.descendants.dedup();
        // A separate process group also catches children orphaned before warmup completes.
        let _ = Command::new("kill")
            .args(["-TERM", "--"])
            .arg(format!("-{}", self.child.id()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.child.kill();
        for pid in &self.descendants {
            if Path::new(&format!("/proc/{pid}")).exists() {
                let _ = Command::new("kill")
                    .arg(pid.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        let _ = self.child.wait();
    }
}

fn clk_tck() -> f64 {
    Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(100.)
}

fn proc_stat(pid: u32) -> Option<ProcStat> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_proc_stat(&stat)
}

fn parse_proc_stat(stat: &str) -> Option<ProcStat> {
    let rest = stat.get(stat.rfind(')')? + 2..)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if matches!(*fields.first()?, "Z" | "X") {
        return None;
    }
    // fields[11] = utime, fields[12] = stime (1-based 14/15 minus pid and comm)
    Some(ProcStat {
        ticks: fields
            .get(11)?
            .parse::<u64>()
            .ok()?
            .checked_add(fields.get(12)?.parse::<u64>().ok()?)?,
        start_time: fields.get(19)?.parse().ok()?,
    })
}

fn children(pid: u32) -> Vec<u32> {
    let mut out = vec![];
    let Ok(dir) = fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let Some(p) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(format!("/proc/{p}/stat")) else {
            continue;
        };
        let Some(close) = stat.rfind(')') else {
            continue;
        };
        let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
        if fields.get(1).and_then(|s| s.parse::<u32>().ok()) == Some(pid) {
            out.push(p);
            out.extend(children(p));
        }
    }
    out
}

fn kib_field(text: &str, key: &str) -> Option<u64> {
    let line = text.lines().find_map(|l| l.strip_prefix(key))?;
    let mut fields = line.split_whitespace();
    let value = fields.next()?.parse().ok()?;
    (fields.next()? == "kB").then_some(value)
}

fn parse_proc_memory(text: &str) -> Memory {
    Memory {
        rss_kib: kib_field(text, "Rss:"),
        pss_kib: kib_field(text, "Pss:"),
    }
}

fn proc_memory(pid: u32) -> Memory {
    let mut memory = fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .map(|s| parse_proc_memory(&s))
        .unwrap_or_default();
    if memory.rss_kib.is_none() {
        memory.rss_kib = fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|s| kib_field(&s, "VmRSS:"));
    }
    memory
}

/// Match the actual executable, not its truncated comm or an arbitrary descendant.
fn daemon_stat(pid: u32, expected: &Path) -> Option<ProcStat> {
    if fs::read_link(format!("/proc/{pid}/exe")).ok()? != expected {
        return None;
    }
    proc_stat(pid)
}

fn find_daemon(ui_pid: u32, expected: &Path) -> Option<DaemonIdentity> {
    let mut matching = children(ui_pid).into_iter().filter_map(|pid| {
        Some(DaemonIdentity {
            pid,
            start_time: daemon_stat(pid, expected)?.start_time,
        })
    });
    let daemon = matching.next()?;
    // Multiple matching descendants cannot establish a single daemon CPU window.
    matching.next().is_none().then_some(daemon)
}

fn daemon_memory(daemon: Option<&DaemonIdentity>, expected: &Path) -> Memory {
    let Some(daemon) = daemon else {
        return Memory::default();
    };
    let memory = proc_memory(daemon.pid);
    if daemon_stat(daemon.pid, expected).is_some_and(|s| s.start_time == daemon.start_time) {
        memory
    } else {
        Memory::default()
    }
}

fn cpu_percent(start: &ProcStat, end: &ProcStat, tck: f64, elapsed: f64) -> Option<f64> {
    if start.start_time != end.start_time || tck <= 0. || elapsed <= 0. {
        return None;
    }
    Some(end.ticks.checked_sub(start.ticks)? as f64 / tck / elapsed * 100.)
}

/// Sum of `drm-memory-*` / `drm-total-*` entries over the process' DRM file descriptors,
/// de-duplicated by `drm-client-id`.
fn vram_kib(pid: u32) -> Option<u64> {
    let mut clients: HashMap<String, u64> = HashMap::new();
    let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fdinfo")) else {
        return None;
    };
    for entry in dir.flatten() {
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Some(client) = text
            .lines()
            .find(|l| l.starts_with("drm-client-id:"))
            .map(|l| l.to_owned())
        else {
            continue;
        };
        let mut total = None;
        for line in text.lines() {
            if (line.starts_with("drm-memory-") || line.starts_with("drm-total-"))
                && let Some(v) = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse::<u64>().ok())
            {
                let unit = line.split_whitespace().nth(2).unwrap_or("KiB");
                let value = match unit {
                    "MiB" => v * 1024,
                    "GiB" => v * 1024 * 1024,
                    "B" => v / 1024,
                    _ => v,
                };
                *total.get_or_insert(0) += value;
            }
        }
        if let Some(total) = total {
            clients.insert(client, total);
        }
    }
    (!clients.is_empty()).then(|| clients.values().sum())
}

fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

/// A fresh nested compositor per run: a long-lived idle KWin eventually stops frame callbacks
/// (idle/DPMS), which would make clients look hidden and skew the measurement.
fn start_compositor(socket_name: &str) -> Result<ManagedChild, String> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR not set")?;
    let socket = PathBuf::from(runtime).join(socket_name);
    let _ = fs::remove_file(&socket);
    let child = ManagedChild::new(
        Command::new("kwin_wayland")
            .args([
                "--virtual",
                "--socket",
                socket_name,
                "--width",
                "1280",
                "--height",
                "800",
            ])
            .stdout(Stdio::null())
            // Keep startup errors visible instead of reporting them as absent frames.
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("failed to start kwin_wayland: {e}"))?,
    );
    if !wait_for_socket(&socket, Duration::from_secs(15)) {
        return Err("nested compositor did not create its socket".into());
    }
    Ok(child)
}

fn write_egui_settings(dir: &Path, page: &str) -> std::io::Result<()> {
    let d = dir.join("taskmanager");
    fs::create_dir_all(&d)?;
    fs::write(
        d.join("config.toml"),
        format!(
            "window-width = 1280\nwindow-height = 800\n\
             window-selected-page = \"{page}\"\nperformance-selected-page = \"cpu\"\n"
        ),
    )
}

fn run_once(
    opts: &Options,
    backend: &Backend,
    variant: Variant,
    page: &str,
    synthetic: Option<usize>,
    tck: f64,
    tag: &str,
) -> Result<Sample, String> {
    let tmp = std::env::temp_dir().join(format!("mc-bench-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let stats = tmp.join("stats.txt");
    let total = opts.warmup + opts.seconds + 2;

    let magpie_dir = backend
        .daemon
        .parent()
        .ok_or("daemon binary has no parent directory")?;
    let path = format!(
        "{}:{}",
        magpie_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    write_egui_settings(&tmp, page).map_err(|e| e.to_string())?;
    let (bin, renderer) = match variant {
        Variant::EguiWgpu => (&opts.egui, "wgpu"),
        Variant::EguiGlow => (&opts.egui_glow, "glow"),
    };
    let mut cmd = Command::new(bin);
    cmd.arg("--renderer")
        .arg(renderer)
        .arg("--bench-seconds")
        .arg(total.to_string())
        .arg("--stats-file")
        .arg(&stats);
    if let Some(n) = synthetic {
        cmd.arg("--synthetic").arg(n.to_string());
    }
    cmd.env_remove("DISPLAY")
        .env("XDG_CONFIG_HOME", &tmp)
        .env("PATH", path)
        .env("MC_MAGPIE_HW_DB", &backend.hwdb)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(locale) = &backend.locale {
        cmd.env("MC_LOCALEDIR", locale);
    }

    let socket_name = format!("{SOCKET_PREFIX}-{tag}");
    let compositor = start_compositor(&socket_name)?;
    cmd.env("WAYLAND_DISPLAY", &socket_name);

    let result = measure(
        opts,
        variant,
        &mut cmd,
        &stats,
        &backend.daemon,
        synthetic.is_none(),
        tck,
    );
    drop(compositor);

    let mut sample = result?;
    if sample.frames.unwrap_or(0) == 0 {
        return Err("egui rendered no frames (window not visible); run discarded".into());
    }
    sample.valid = true;
    let _ = fs::remove_dir_all(&tmp);
    Ok(sample)
}

fn measure(
    opts: &Options,
    variant: Variant,
    cmd: &mut Command,
    stats: &Path,
    daemon_binary: &Path,
    live: bool,
    tck: f64,
) -> Result<Sample, String> {
    let mut child = ManagedChild::new(
        cmd.process_group(0)
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", variant.name()))?,
    );
    let pid = child.child.id();
    sleep(Duration::from_secs(opts.warmup));
    if let Ok(Some(status)) = child.child.try_wait() {
        return Err(format!("{} exited during warmup: {status}", variant.name()));
    }

    child.descendants = children(pid);
    let ui0 = proc_stat(pid).ok_or("app exited during warmup")?;
    let daemon0 = live.then(|| find_daemon(pid, daemon_binary)).flatten();
    let d0 = daemon0
        .as_ref()
        .and_then(|d| daemon_stat(d.pid, daemon_binary));
    let mut daemon_window_valid = daemon0.is_some() && d0.is_some();
    let t0 = Instant::now();
    let mut memory = vec![MemoryPoint {
        seconds: 0.,
        ui: proc_memory(pid),
        daemon: daemon_memory(daemon0.as_ref(), daemon_binary),
    }];
    let end = t0 + Duration::from_secs(opts.seconds);
    let mut next = (t0 + MEMORY_INTERVAL).min(end);
    let (ui1, d1, elapsed) = loop {
        sleep(next.saturating_duration_since(Instant::now()));
        if let Ok(Some(status)) = child.child.try_wait() {
            return Err(format!(
                "{} exited during measurement: {status}",
                variant.name()
            ));
        }
        let ui = proc_stat(pid).ok_or("app exited during measurement")?;
        let daemon = live.then(|| find_daemon(pid, daemon_binary)).flatten();
        daemon_window_valid &= daemon.is_some() && daemon == daemon0;
        let ticks = daemon
            .as_ref()
            .and_then(|d| daemon_stat(d.pid, daemon_binary));
        daemon_window_valid &= ticks
            .as_ref()
            .zip(daemon.as_ref())
            .is_some_and(|(s, d)| s.start_time == d.start_time);
        let elapsed = t0.elapsed().as_secs_f64();
        memory.push(MemoryPoint {
            seconds: elapsed,
            ui: proc_memory(pid),
            daemon: daemon_memory(daemon.as_ref(), daemon_binary),
        });
        if next == end {
            break (ui, ticks, elapsed);
        }
        next = (next + MEMORY_INTERVAL).min(end);
    };
    let daemon_cpu_percent = if daemon_window_valid {
        d0.as_ref()
            .zip(d1.as_ref())
            .and_then(|(a, b)| cpu_percent(a, b, tck, elapsed))
    } else {
        None
    };
    if live && daemon_cpu_percent.is_none() {
        eprintln!(
            "[bench] daemon CPU unavailable: no verified, unchanged daemon over the CPU window"
        );
    }

    let mut sample = Sample {
        ui_cpu_percent: cpu_percent(&ui0, &ui1, tck, elapsed).ok_or("invalid UI CPU window")?,
        daemon_cpu_percent,
        memory,
        vram_kib: vram_kib(pid),
        ..Default::default()
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(_)) = child.child.try_wait() {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.child.kill();
            let _ = child.child.wait();
            break;
        }
        sleep(Duration::from_millis(100));
    }
    {
        let text = fs::read_to_string(stats).map_err(|e| {
            format!(
                "app did not save benchmark statistics at {}: {e}",
                stats.display()
            )
        })?;
        let get = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")))
                .map(|v| v.to_owned())
        };
        sample.frames = get("frames").and_then(|v| v.parse().ok());
        sample.p95_ms = get("ui_pass_p95_ms").and_then(|v| v.parse().ok());
        sample.readings = get("readings_applied").and_then(|v| v.parse().ok());
    }
    Ok(sample)
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn median_text(values: impl Iterator<Item = f64>, precision: usize) -> String {
    let value = median(values.collect());
    if value.is_nan() {
        "n/a".into()
    } else {
        format!("{value:.precision$}")
    }
}

fn memory_summary(points: &[MemoryPoint], daemon: bool) -> (Memory, Memory) {
    let get = |p: &MemoryPoint| if daemon { p.daemon } else { p.ui };
    let final_sample = points.last().map(get).unwrap_or_default();
    let maximum = points
        .iter()
        .map(get)
        .fold(Memory::default(), |a, b| Memory {
            rss_kib: a.rss_kib.max(b.rss_kib),
            pss_kib: a.pss_kib.max(b.pss_kib),
        });
    (final_sample, maximum)
}

fn parse(args: &[String]) -> Result<Options, String> {
    let root = root();
    let mut o = Options {
        egui: root.join("target/release/taskmanager"),
        // One binary has both renderers; `--renderer` selects
        egui_glow: root.join("target/release/taskmanager"),
        seconds: 40,
        warmup: 8,
        repeats: 3,
        out: root.join("perf/bench-latest.md"),
    };
    let mut glow = None;
    let mut custom_binary = false;
    let mut explicit_out = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("missing value for {a}"))
        };
        match a.as_str() {
            "--gtk" => return Err("--gtk is no longer supported; bench-idle benchmarks the wgpu and glow renderers (use --egui or --egui-glow to override their binaries)".into()),
            "--egui" => { o.egui = next()?.into(); custom_binary = true; }
            "--egui-glow" => { glow = Some(PathBuf::from(next()?)); custom_binary = true; }
            "--seconds" => o.seconds = next()?.parse().map_err(|_| "invalid --seconds")?,
            "--warmup" => o.warmup = next()?.parse().map_err(|_| "invalid --warmup")?,
            "--repeats" => o.repeats = next()?.parse().map_err(|_| "invalid --repeats")?,
            "--out" => { o.out = next()?.into(); explicit_out = true; }
            _ => return Err(format!("unknown bench-idle option {a}")),
        }
    }
    if o.seconds == 0 || o.repeats == 0 {
        return Err("--seconds and --repeats must be greater than zero".into());
    }
    o.egui_glow = glow.unwrap_or_else(|| o.egui.clone());
    if custom_binary && !explicit_out {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        o.out = root.join(format!("target/diagnostic/bench-{stamp}.md"));
    }
    Ok(o)
}

fn sibling_daemon(binary: &Path) -> Option<PathBuf> {
    let binary = fs::canonicalize(binary).ok()?;
    let sibling = binary.parent()?.join("missioncenter-magpie");
    fs::canonicalize(sibling).ok().filter(|p| p.is_file())
}

fn prepare_backend(
    binary: &Path,
    root: &Path,
    default_daemon: &mut Option<PathBuf>,
    default_hwdb: &mut Option<PathBuf>,
) -> Result<Backend, String> {
    let daemon = if let Some(sibling) = sibling_daemon(binary) {
        sibling
    } else {
        if default_daemon.is_none() {
            *default_daemon = Some(dist::magpie_binary(root)?);
        }
        fs::canonicalize(default_daemon.as_ref().unwrap()).map_err(|e| e.to_string())?
    };
    let prefix = fs::canonicalize(binary)
        .map_err(|e| e.to_string())?
        .parent()
        .and_then(Path::parent)
        .ok_or("UI binary has no bundle prefix")?
        .to_path_buf();
    let bundled_hwdb = prefix.join("share/missioncenter/hw.db");
    let hwdb = if let Some(path) = std::env::var_os("MC_MAGPIE_HW_DB") {
        path.into()
    } else if bundled_hwdb.is_file() {
        bundled_hwdb
    } else {
        if default_hwdb.is_none() {
            *default_hwdb = Some(dist::hwdb(root, &root.join("target/hwdb"))?);
        }
        default_hwdb.as_ref().unwrap().clone()
    };
    let locale = std::env::var_os("MC_LOCALEDIR")
        .map(PathBuf::from)
        .or_else(|| {
            let path = prefix.join("share/locale");
            path.is_dir().then_some(path)
        });
    Ok(Backend {
        daemon,
        hwdb,
        locale,
    })
}

pub fn bench_idle(args: &[String]) -> Result<(), String> {
    let opts = parse(args)?;
    for p in [&opts.egui, &opts.egui_glow] {
        if !p.exists() {
            return Err(format!("missing binary {}", p.display()));
        }
    }
    let root = root();
    let mut default_daemon = None;
    let mut default_hwdb = None;
    let backends = [
        prepare_backend(&opts.egui, &root, &mut default_daemon, &mut default_hwdb)?,
        prepare_backend(
            &opts.egui_glow,
            &root,
            &mut default_daemon,
            &mut default_hwdb,
        )?,
    ];
    let tck = clk_tck();

    let mut rows: Vec<Row> = vec![];

    // (name, page, synthetic process count, variants)
    type Scenario<'a> = (String, &'a str, Option<usize>, Vec<Variant>);
    let scenarios: Vec<Scenario> = vec![
        (
            "performance/cpu".into(),
            "performance-page",
            None,
            vec![Variant::EguiWgpu, Variant::EguiGlow],
        ),
        (
            "apps, 1000 synthetic processes".into(),
            "apps-page",
            Some(1000),
            vec![Variant::EguiWgpu, Variant::EguiGlow],
        ),
    ];

    for (scenario, page, synthetic, variants) in &scenarios {
        for variant in variants {
            let backend = &backends[match variant {
                Variant::EguiWgpu => 0,
                Variant::EguiGlow => 1,
            }];
            let mut samples = vec![];
            for r in 0..opts.repeats {
                eprintln!("[bench] {scenario} / {} / run {}", variant.name(), r + 1);
                match run_once(
                    &opts,
                    backend,
                    *variant,
                    page,
                    *synthetic,
                    tck,
                    &format!("{}-{r}", variant.name()),
                ) {
                    Ok(s) => samples.push(s),
                    Err(e) => eprintln!("[bench] run failed: {e}"),
                }
            }
            rows.push(Row {
                scenario: scenario.clone(),
                variant: *variant,
                daemon: synthetic.is_none().then(|| backend.daemon.clone()),
                samples,
            });
        }
    }

    write_report(&opts, &rows)
}

fn write_report(opts: &Options, rows: &[Row]) -> Result<(), String> {
    let mut md = String::new();
    md.push_str(&format!(
        "# Idle benchmark (generated by `cargo xtask bench-idle`)\n\n\
         Nested `kwin_wayland --virtual` 1280x800, refresh interval 1 s, warmup {} s, measurement {} s, {} repeats, medians shown.\n\
         CPU = % of one core. Daemon CPU requires the matching executable and unchanged PID/start time throughout the sampled window. Frames/readings/p95 come from the egui app itself.\n\n\
          Runs without rendered frames are discarded; `valid` = kept runs. Rows with no valid runs make the benchmark fail after saving this report.\n\n\
         UI wgpu: `{}`; UI glow: `{}`. These binaries are used as supplied, without rebuilding the UI.\n\
         RUST_LOG: `{}`.\n\n",
        opts.warmup, opts.seconds, opts.repeats,
        opts.egui.display(), opts.egui_glow.display(),
        std::env::var("RUST_LOG").unwrap_or_default(),
    ));
    for row in rows.iter().filter(|r| r.daemon.is_some()) {
        md.push_str(&format!(
            "- {} / {} daemon: `{}`\n",
            row.scenario,
            row.variant.name(),
            row.daemon.as_ref().unwrap().display()
        ));
    }
    md.push_str("\n| scenario | variant | valid | UI CPU % | daemon CPU % | GPU mem MiB | frames/reading | UI pass p95 ms |\n|---|---|---|---|---|---|---|---|\n");
    for row in rows {
        let s = &row.samples;
        let frames_per_reading = median_text(
            s.iter().filter_map(|x| match (x.frames, x.readings) {
                (Some(f), Some(r)) if r > 0 => Some(f as f64 / r as f64),
                _ => None,
            }),
            2,
        );
        let p95 = median_text(s.iter().filter_map(|x| x.p95_ms), 2);
        md.push_str(&format!(
            "| {} | {} | {}/{} | {} | {} | {} | {} | {} |\n",
            row.scenario,
            row.variant.name(),
            s.iter().filter(|x| x.valid).count(),
            opts.repeats,
            median_text(s.iter().map(|x| x.ui_cpu_percent), 2),
            median_text(s.iter().filter_map(|x| x.daemon_cpu_percent), 2),
            median_text(
                s.iter()
                    .filter_map(|x| x.vram_kib)
                    .map(|v| v as f64 / 1024.),
                1
            ),
            frames_per_reading,
            p95,
        ));
    }
    md.push_str("\n## Process memory (MiB)\n\nExternal RSS/PSS samples at measurement start, every 10 s, and measurement end. Final values and maximum observed values are distinct; the sampled maximum is not an absolute peak. `n/a` means absent/unreadable, including the synthetic scenario's daemon. RSS falls back to VmRSS when smaps_rollup is unavailable; PSS has no fallback. Each summary component is the median of available run values.\n\n| scenario | variant | UI final RSS / PSS | UI maximum observed RSS / PSS | daemon final RSS / PSS | daemon maximum observed RSS / PSS |\n|---|---|---|---|---|---|\n");
    for row in rows {
        let pair = |daemon, maximum| {
            let memories: Vec<_> = row
                .samples
                .iter()
                .map(|s| {
                    let (final_sample, max) = memory_summary(&s.memory, daemon);
                    if maximum { max } else { final_sample }
                })
                .collect();
            format!(
                "{} / {}",
                median_text(
                    memories
                        .iter()
                        .filter_map(|m| m.rss_kib)
                        .map(|v| v as f64 / 1024.),
                    1
                ),
                median_text(
                    memories
                        .iter()
                        .filter_map(|m| m.pss_kib)
                        .map(|v| v as f64 / 1024.),
                    1
                ),
            )
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            row.scenario,
            row.variant.name(),
            pair(false, false),
            pair(false, true),
            pair(true, false),
            pair(true, true)
        ));
    }
    md.push_str("\n### Individual memory observations\n\nSeconds are relative to the post-warmup measurement start.\n\n| scenario | variant | run | seconds | UI RSS MiB | UI PSS MiB | daemon RSS MiB | daemon PSS MiB |\n|---|---|---|---|---|---|---|---|\n");
    for row in rows {
        for (run, sample) in row.samples.iter().enumerate() {
            for point in &sample.memory {
                let mib = |value: Option<u64>| {
                    value
                        .map(|v| format!("{:.1}", v as f64 / 1024.))
                        .unwrap_or_else(|| "n/a".into())
                };
                md.push_str(&format!(
                    "| {} | {} | {} | {:.3} | {} | {} | {} | {} |\n",
                    row.scenario,
                    row.variant.name(),
                    run + 1,
                    point.seconds,
                    mib(point.ui.rss_kib),
                    mib(point.ui.pss_kib),
                    mib(point.daemon.rss_kib),
                    mib(point.daemon.pss_kib)
                ));
            }
        }
    }
    if let Some(parent) = opts.out.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&opts.out, &md).map_err(|e| e.to_string())?;
    println!("{md}");
    println!("written to {}", opts.out.display());
    let invalid: Vec<String> = rows
        .iter()
        .filter(|row| !row.samples.iter().any(|sample| sample.valid))
        .map(|row| format!("{} / {}", row.scenario, row.variant.name()))
        .collect();
    if !invalid.is_empty() {
        return Err(format!(
            "no valid measurements for {}; report saved to {}",
            invalid.join(", "),
            opts.out.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parser_defaults() {
        let opts = parse(&[]).unwrap();
        assert_eq!(opts.egui, root().join("target/release/taskmanager"));
        assert_eq!(opts.egui_glow, opts.egui);
        assert_eq!(opts.seconds, 40);
        assert_eq!(opts.warmup, 8);
        assert_eq!(opts.repeats, 3);
        assert_eq!(opts.out, root().join("perf/bench-latest.md"));
    }

    #[test]
    fn parser_overrides() {
        let opts = parse(&args(&[
            "--egui",
            "wgpu-app",
            "--egui-glow",
            "glow-app",
            "--seconds",
            "12",
            "--warmup",
            "2",
            "--repeats",
            "4",
            "--out",
            "report.md",
        ]))
        .unwrap();
        assert_eq!(opts.egui, Path::new("wgpu-app"));
        assert_eq!(opts.egui_glow, Path::new("glow-app"));
        assert_eq!(opts.seconds, 12);
        assert_eq!(opts.warmup, 2);
        assert_eq!(opts.repeats, 4);
        assert_eq!(opts.out, Path::new("report.md"));
    }

    #[test]
    fn parser_rejects_legacy_flag() {
        for values in [&["--gtk"][..], &["--gtk", "old-app"][..]] {
            let error = parse(&args(values)).unwrap_err();
            assert!(error.contains("--gtk is no longer supported"));
            assert!(error.contains("wgpu and glow"));
        }
    }

    #[test]
    fn one_custom_binary_selects_both_renderers_and_preserves_default_report() {
        let opts = parse(&args(&["--egui", "bundle/bin/taskmanager"])).unwrap();
        assert_eq!(opts.egui_glow, opts.egui);
        assert!(opts.out.starts_with(root().join("target/diagnostic")));
        assert_ne!(opts.out, root().join("perf/bench-latest.md"));
        let opts = parse(&args(&[
            "--egui-glow",
            "glow",
            "--egui",
            "wgpu",
            "--out",
            "custom.md",
        ]))
        .unwrap();
        assert_eq!(opts.egui, Path::new("wgpu"));
        assert_eq!(opts.egui_glow, Path::new("glow"));
        assert_eq!(opts.out, Path::new("custom.md"));
    }

    #[test]
    fn parser_rejects_missing_or_invalid_measurement_window() {
        for values in [
            &["--egui"][..],
            &["--out"][..],
            &["--seconds", "0"][..],
            &["--seconds", "bad"][..],
            &["--repeats", "0"][..],
        ] {
            assert!(parse(&args(values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn memory_parser_preserves_unknown_and_real_zero() {
        assert_eq!(
            parse_proc_memory("Rss: 2048 kB\nPss: 1024 kB\nPss_Anon: 1 kB\n"),
            Memory {
                rss_kib: Some(2048),
                pss_kib: Some(1024)
            }
        );
        assert_eq!(
            parse_proc_memory("Pss: 0 kB\n"),
            Memory {
                rss_kib: None,
                pss_kib: Some(0)
            }
        );
        for text in [
            "",
            "Pss_Anon: 100 kB\n",
            "Rss: -1 kB\nPss: bad kB\n",
            "Rss: 4 bytes\nPss: 5\n",
        ] {
            assert_eq!(parse_proc_memory(text), Memory::default());
        }
        assert_eq!(kib_field("VmRSS: 400 kB\n", "VmRSS:"), Some(400));
        assert_eq!(proc_memory(u32::MAX), Memory::default());
    }

    #[test]
    fn sampled_maximum_does_not_replace_an_unavailable_final_value() {
        let points = [
            MemoryPoint {
                seconds: 0.,
                ui: Memory {
                    rss_kib: Some(100),
                    pss_kib: Some(20),
                },
                daemon: Memory::default(),
            },
            MemoryPoint {
                seconds: 10.,
                ui: Memory {
                    rss_kib: Some(80),
                    pss_kib: None,
                },
                daemon: Memory::default(),
            },
        ];
        assert_eq!(memory_summary(&points, false), (points[1].ui, points[0].ui));
        assert_eq!(
            memory_summary(&points, true),
            (Memory::default(), Memory::default())
        );
    }

    #[test]
    fn cpu_window_rejects_pid_reuse_and_backwards_counters() {
        let start = ProcStat {
            ticks: 10,
            start_time: 2,
        };
        assert_eq!(
            cpu_percent(
                &start,
                &ProcStat {
                    ticks: 30,
                    start_time: 2
                },
                100.,
                10.
            ),
            Some(2.)
        );
        assert_eq!(
            cpu_percent(
                &start,
                &ProcStat {
                    ticks: 30,
                    start_time: 3
                },
                100.,
                10.
            ),
            None
        );
        assert_eq!(
            cpu_percent(
                &start,
                &ProcStat {
                    ticks: 9,
                    start_time: 2
                },
                100.,
                10.
            ),
            None
        );
        assert_eq!(cpu_percent(&start, &start, 100., 0.), None);
        let executable = std::env::current_exe().unwrap();
        assert!(daemon_stat(std::process::id(), &executable).is_some());
        assert!(daemon_stat(std::process::id(), Path::new("/not/the/daemon")).is_none());
        let fields = "S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19";
        assert_eq!(
            parse_proc_stat(&format!("123 (comm with ) spaces) {fields}")),
            Some(ProcStat {
                ticks: 23,
                start_time: 19
            })
        );
        assert!(parse_proc_stat("123 (broken)").is_none());
    }

    #[test]
    fn sibling_bundle_uses_matching_backend_without_building_default() {
        let dir = std::env::temp_dir().join(format!("mc-bench-bundle-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::create_dir_all(dir.join("share/missioncenter")).unwrap();
        fs::create_dir_all(dir.join("share/locale")).unwrap();
        let binary = dir.join("bin/taskmanager");
        let daemon = dir.join("bin/missioncenter-magpie");
        fs::write(&binary, []).unwrap();
        fs::write(&daemon, []).unwrap();
        fs::write(dir.join("share/missioncenter/hw.db"), []).unwrap();
        let mut default_daemon = None;
        let mut default_hwdb = None;
        let backend =
            prepare_backend(&binary, &root(), &mut default_daemon, &mut default_hwdb).unwrap();
        assert_eq!(backend.daemon, fs::canonicalize(&daemon).unwrap());
        assert!(default_daemon.is_none());
        assert!(default_hwdb.is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_measurements_fail_after_saving_report() {
        let mut opts = parse(&[]).unwrap();
        let dir = std::env::temp_dir().join(format!("mc-bench-report-test-{}", std::process::id()));
        opts.out = dir.join("report.md");
        let rows = [Row {
            scenario: "performance/cpu".into(),
            variant: Variant::EguiWgpu,
            daemon: None,
            samples: vec![],
        }];
        let error = write_report(&opts, &rows).unwrap_err();
        assert!(error.contains("no valid measurements"));
        assert!(error.contains("performance/cpu / egui-wgpu"));
        let report = fs::read_to_string(&opts.out).unwrap();
        assert!(report.contains("| performance/cpu | egui-wgpu | 0/3 | n/a"));
        assert!(report.contains("Rows with no valid runs make the benchmark fail"));

        let rows = [Row {
            scenario: "performance/cpu".into(),
            variant: Variant::EguiWgpu,
            daemon: None,
            samples: vec![Sample {
                frames: Some(6),
                readings: Some(3),
                valid: true,
                ..Default::default()
            }],
        }];
        write_report(&opts, &rows).unwrap();
        let report = fs::read_to_string(&opts.out).unwrap();
        assert!(report.contains("| egui-wgpu | 1/3 | 0.00 | n/a | n/a | 2.00 | n/a |"));
        assert!(report.contains("| egui-wgpu | n/a / n/a | n/a / n/a | n/a / n/a | n/a / n/a |"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn managed_child_cleans_up_process_group_without_warmup_snapshot() {
        let mut child = ManagedChild::new(
            Command::new("sh")
                .args(["-c", "sleep 60 >/dev/null & printf '%s\\n' \"$!\"; wait"])
                .process_group(0)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let pid = child.child.id();
        let mut line = String::new();
        BufReader::new(child.child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let descendant: u32 = line.trim().parse().unwrap();
        assert!(child.descendants.is_empty());
        drop(child);
        assert!(proc_stat(pid).is_none(), "spawned process was not reaped");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let stat = fs::read_to_string(format!("/proc/{descendant}/stat"));
            if stat.is_err() || stat.is_ok_and(|s| s[s.rfind(')').unwrap() + 2..].starts_with('Z'))
            {
                break;
            }
            assert!(Instant::now() < deadline, "descendant is still running");
            sleep(Duration::from_millis(10));
        }
    }
}
