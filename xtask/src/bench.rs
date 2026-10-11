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
use std::time::{Duration, Instant};

use crate::{dist, root};

const SOCKET_PREFIX: &str = "wayland-mcbench";

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
    daemon_cpu_percent: f64,
    pss_kib: u64,
    vram_kib: u64,
    frames: Option<u64>,
    p95_ms: Option<f64>,
    readings: Option<u64>,
    valid: bool,
}

struct Row {
    scenario: String,
    variant: Variant,
    samples: Vec<Sample>,
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

fn proc_ticks(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // fields[11] = utime, fields[12] = stime (1-based 14/15 minus pid and comm)
    Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
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

fn pss_kib(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Pss:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

/// Sum of `drm-memory-*` / `drm-total-*` entries over the process' DRM file descriptors,
/// de-duplicated by `drm-client-id`.
fn vram_kib(pid: u32) -> u64 {
    let mut clients: HashMap<String, u64> = HashMap::new();
    let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fdinfo")) else {
        return 0;
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
        let mut total = 0;
        for line in text.lines() {
            if (line.starts_with("drm-memory-") || line.starts_with("drm-total-"))
                && let Some(v) = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse::<u64>().ok())
            {
                let unit = line.split_whitespace().nth(2).unwrap_or("KiB");
                total += match unit {
                    "MiB" => v * 1024,
                    "GiB" => v * 1024 * 1024,
                    "B" => v / 1024,
                    _ => v,
                };
            }
        }
        clients.insert(client, total);
    }
    clients.values().sum()
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

#[allow(clippy::too_many_arguments)]
fn run_once(
    opts: &Options,
    daemon: &Path,
    hwdb: &Path,
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

    let magpie_dir = daemon
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
        .env("MC_MAGPIE_HW_DB", hwdb)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());

    let socket_name = format!("{SOCKET_PREFIX}-{tag}");
    let compositor = start_compositor(&socket_name)?;
    cmd.env("WAYLAND_DISPLAY", &socket_name);

    let result = measure(opts, variant, &mut cmd, &stats, tck);
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

    let daemon_pids = children(pid);
    child.descendants.clone_from(&daemon_pids);
    let ui0 = proc_ticks(pid).ok_or("app exited during warmup")?;
    let d0: u64 = daemon_pids.iter().filter_map(|p| proc_ticks(*p)).sum();
    let t0 = Instant::now();
    sleep(Duration::from_secs(opts.seconds));
    if let Ok(Some(status)) = child.child.try_wait() {
        return Err(format!(
            "{} exited during measurement: {status}",
            variant.name()
        ));
    }
    let ui1 = proc_ticks(pid).ok_or("app exited during measurement")?;
    let d1: u64 = daemon_pids.iter().filter_map(|p| proc_ticks(*p)).sum();
    let elapsed = t0.elapsed().as_secs_f64();

    let mut sample = Sample {
        ui_cpu_percent: (ui1 - ui0) as f64 / tck / elapsed * 100.,
        daemon_cpu_percent: d1.saturating_sub(d0) as f64 / tck / elapsed * 100.,
        pss_kib: pss_kib(pid),
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
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = || it.next().cloned().unwrap_or_default();
        match a.as_str() {
            "--gtk" => return Err("--gtk is no longer supported; bench-idle benchmarks the wgpu and glow renderers (use --egui or --egui-glow to override their binaries)".into()),
            "--egui" => o.egui = next().into(),
            "--egui-glow" => o.egui_glow = next().into(),
            "--seconds" => o.seconds = next().parse().unwrap_or(o.seconds),
            "--warmup" => o.warmup = next().parse().unwrap_or(o.warmup),
            "--repeats" => o.repeats = next().parse().unwrap_or(o.repeats),
            "--out" => o.out = next().into(),
            _ => {}
        }
    }
    Ok(o)
}

pub fn bench_idle(args: &[String]) -> Result<(), String> {
    let opts = parse(args)?;
    for p in [&opts.egui, &opts.egui_glow] {
        if !p.exists() {
            return Err(format!("missing binary {}", p.display()));
        }
    }
    let root = root();
    let daemon = dist::magpie_binary(&root)?;
    let hwdb = dist::hwdb(&root, &root.join("target/hwdb"))?;
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
            let mut samples = vec![];
            for r in 0..opts.repeats {
                eprintln!("[bench] {scenario} / {} / run {}", variant.name(), r + 1);
                match run_once(
                    &opts,
                    &daemon,
                    &hwdb,
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
         CPU = % of one core. `daemon` = magpie child process. Frames/readings/p95 come from the egui app itself.\n\n\
          Runs without rendered frames are discarded; `valid` = kept runs. Rows with no valid runs make the benchmark fail after saving this report.\n\n\
         | scenario | variant | valid | UI CPU % | daemon CPU % | PSS MiB | GPU mem MiB | frames/reading | UI pass p95 ms |\n\
         |---|---|---|---|---|---|---|---|---|\n",
        opts.warmup, opts.seconds, opts.repeats
    ));
    for row in rows {
        let s = &row.samples;
        let f = |g: &dyn Fn(&Sample) -> f64| median(s.iter().map(g).collect());
        let frames_per_reading = {
            let v: Vec<f64> = s
                .iter()
                .filter_map(|x| match (x.frames, x.readings) {
                    (Some(f), Some(r)) if r > 0 => Some(f as f64 / r as f64),
                    _ => None,
                })
                .collect();
            if v.is_empty() {
                "n/a".to_owned()
            } else {
                format!("{:.2}", median(v))
            }
        };
        let p95 = {
            let v: Vec<f64> = s.iter().filter_map(|x| x.p95_ms).collect();
            if v.is_empty() {
                "n/a".to_owned()
            } else {
                format!("{:.2}", median(v))
            }
        };
        md.push_str(&format!(
            "| {} | {} | {}/{} | {:.2} | {:.2} | {:.1} | {:.1} | {} | {} |\n",
            row.scenario,
            row.variant.name(),
            s.iter().filter(|x| x.valid).count(),
            opts.repeats,
            f(&|x| x.ui_cpu_percent),
            f(&|x| x.daemon_cpu_percent),
            f(&|x| x.pss_kib as f64) / 1024.,
            f(&|x| x.vram_kib as f64) / 1024.,
            frames_per_reading,
            p95,
        ));
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
    fn invalid_measurements_fail_after_saving_report() {
        let mut opts = parse(&[]).unwrap();
        let dir = std::env::temp_dir().join(format!("mc-bench-report-test-{}", std::process::id()));
        opts.out = dir.join("report.md");
        let rows = [Row {
            scenario: "performance/cpu".into(),
            variant: Variant::EguiWgpu,
            samples: vec![],
        }];
        let error = write_report(&opts, &rows).unwrap_err();
        assert!(error.contains("no valid measurements"));
        assert!(error.contains("performance/cpu / egui-wgpu"));
        let report = fs::read_to_string(&opts.out).unwrap();
        assert!(report.contains("| performance/cpu | egui-wgpu | 0/3 | NaN"));
        assert!(report.contains("Rows with no valid runs make the benchmark fail"));

        let rows = [Row {
            scenario: "performance/cpu".into(),
            variant: Variant::EguiWgpu,
            samples: vec![Sample {
                frames: Some(6),
                readings: Some(3),
                valid: true,
                ..Default::default()
            }],
        }];
        write_report(&opts, &rows).unwrap();
        let report = fs::read_to_string(&opts.out).unwrap();
        assert!(report.contains("| egui-wgpu | 1/3 | 0.00 | 0.00 | 0.0 | 0.0 | 2.00 | n/a |"));
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
        assert!(proc_ticks(pid).is_none(), "spawned process was not reaped");
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
