/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Opt-in fixed-size UI/client profiling. Scopes only record counters; the
//! reporter owns formatting/logging. Timings are inclusive, not additive, and
//! thread CPU excludes other threads, the daemon and GPU execution.

use std::fmt::Write;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TARGET: &str = "TaskManager::Profile";
const LIMIT: usize = 8192;
const BUCKETS: usize = 32;
static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Phase {
    Startup,
    AppInit,
    Logic,
    Apply,
    GpuSnapshot,
    PerformanceUpdate,
    AppsUpdate,
    ServicesUpdate,
    DetailsUpdate,
    ServicesViewUpdate,
    TableTree,
    TableGpu,
    TableTotals,
    TableFormat,
    TableFlatten,
    Ui,
    ProcessesDraw,
    PerformanceDraw,
    DetailsDraw,
    ServicesDraw,
    DialogsDraw,
    IconDecode,
    IconUpload,
    GatherStartup,
    GatherCycle,
    Processes,
    Users,
    Sockets,
    Apps,
    Disks,
    Gpus,
    Cpu,
    Memory,
    Network,
    Fans,
    Batteries,
    UserServices,
    SystemServices,
    GatherIcons,
    DiskSort,
    Action,
    IpcEncode,
    IpcSend,
    IpcWait,
    IpcDecode,
    SocketDump,
    SocketOwners,
}

const NAMES: [&str; 47] = [
    "startup",
    "app.init",
    "ui.logic",
    "ui.apply",
    "ui.gpu-snapshot",
    "ui.performance-update",
    "ui.apps-update",
    "ui.services-update",
    "ui.details-update",
    "ui.services-view-update",
    "table.tree",
    "table.gpu",
    "table.totals",
    "table.format",
    "table.flatten",
    "ui.draw",
    "draw.processes",
    "draw.performance",
    "draw.details",
    "draw.services",
    "draw.dialogs",
    "icons.decode",
    "icons.upload",
    "gather.startup",
    "gather.cycle",
    "gather.processes",
    "gather.users",
    "gather.sockets",
    "gather.apps",
    "gather.disks",
    "gather.gpus",
    "gather.cpu",
    "gather.memory",
    "gather.network",
    "gather.fans",
    "gather.batteries",
    "gather.user-services",
    "gather.system-services",
    "gather.icons",
    "gather.disk-sort",
    "gather.action",
    "ipc.encode",
    "ipc.send",
    "ipc.wait",
    "ipc.decode",
    "sockets.dump",
    "sockets.owners",
];

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Counter {
    Published,
    Overwritten,
    Applied,
    Overrun,
    RequestFull,
    EventFull,
}
const COUNTER_NAMES: [&str; 6] = [
    "published",
    "overwritten",
    "applied",
    "overrun",
    "request_full",
    "event_full",
];
static COUNTERS: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
static STATS: [Aggregate; NAMES.len()] = [const { Aggregate::new() }; NAMES.len()];

pub fn enabled() -> bool {
    ENABLED.load(Relaxed)
}

pub fn count(counter: Counter) {
    if enabled() {
        COUNTERS[counter as usize].fetch_add(1, Relaxed);
    }
}

struct Aggregate {
    calls: AtomicU64,
    wall: AtomicU64,
    max: AtomicU64,
    cpu: AtomicU64,
    cpu_calls: AtomicU64,
    items: AtomicU64,
    buckets: [AtomicU64; BUCKETS],
}

impl Aggregate {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            wall: AtomicU64::new(0),
            max: AtomicU64::new(0),
            cpu: AtomicU64::new(0),
            cpu_calls: AtomicU64::new(0),
            items: AtomicU64::new(0),
            buckets: [const { AtomicU64::new(0) }; BUCKETS],
        }
    }

    fn record(&self, wall: u64, cpu: Option<u64>, items: u64) {
        self.wall.fetch_add(wall, Relaxed);
        self.max.fetch_max(wall, Relaxed);
        if let Some(cpu) = cpu {
            self.cpu.fetch_add(cpu, Relaxed);
            self.cpu_calls.fetch_add(1, Relaxed);
        }
        self.items.fetch_add(items, Relaxed);
        let micros = wall.div_ceil(1000).max(1);
        let bucket = (64 - (micros - 1).leading_zeros()) as usize;
        self.buckets[bucket.min(BUCKETS - 1)].fetch_add(1, Relaxed);
        self.calls.fetch_add(1, Relaxed);
    }

    fn snapshot(&self) -> Sample {
        let buckets: [u64; BUCKETS] = std::array::from_fn(|i| self.buckets[i].load(Relaxed));
        let n: u64 = buckets.iter().sum();
        let rank = n - n / 20;
        let mut seen = 0;
        let max = self.max.load(Relaxed);
        let p95 = buckets
            .iter()
            .enumerate()
            .find_map(|(i, count)| {
                seen += count;
                (seen >= rank && *count != 0).then(|| {
                    if i == BUCKETS - 1 {
                        max
                    } else {
                        (1u64 << i) * 1000
                    }
                })
            })
            .unwrap_or(0);
        Sample {
            calls: self.calls.load(Relaxed),
            wall: self.wall.load(Relaxed),
            max,
            p95,
            cpu: self.cpu.load(Relaxed),
            cpu_calls: self.cpu_calls.load(Relaxed),
            items: self.items.load(Relaxed),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Sample {
    calls: u64,
    wall: u64,
    max: u64,
    p95: u64,
    cpu: u64,
    cpu_calls: u64,
    items: u64,
}

/// Same-thread guard: moving a thread CPU interval to another thread is invalid.
pub struct Span {
    phase: Phase,
    start: Option<(Instant, Option<u64>)>,
    items: u64,
    _same_thread: PhantomData<Rc<()>>,
}

impl Span {
    pub fn new(phase: Phase) -> Self {
        Self::start(phase, enabled(), thread_cpu)
    }

    fn start(phase: Phase, enabled: bool, clock: fn() -> Option<u64>) -> Self {
        Self {
            phase,
            start: enabled.then(|| (Instant::now(), clock())),
            items: 0,
            _same_thread: PhantomData,
        }
    }

    pub fn items(&mut self, items: usize) {
        self.items = items as u64;
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some((wall, cpu)) = self.start {
            let cpu = cpu
                .zip(thread_cpu())
                .and_then(|(start, end)| end.checked_sub(start));
            STATS[self.phase as usize].record(
                wall.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                cpu,
                self.items,
            );
        }
    }
}

fn thread_cpu() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: initialized writable timespec and the calling-thread clock.
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) } != 0 {
            return None;
        }
        u64::try_from(time.tv_sec)
            .ok()?
            .checked_mul(1_000_000_000)?
            .checked_add(u64::try_from(time.tv_nsec).ok()?)
    }
    #[cfg(not(target_os = "linux"))]
    None
}

pub struct Reporter(Option<(mpsc::SyncSender<()>, std::thread::JoinHandle<()>)>);

impl Reporter {
    /// Call once after initializing the logger, before creating UI/workers.
    pub fn start() -> Self {
        if !log::log_enabled!(target: TARGET, log::Level::Trace) {
            return Self(None);
        }
        let (tx, rx) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("mc-profile".into())
            .spawn(move || {
                let started = Instant::now();
                while matches!(
                    rx.recv_timeout(Duration::from_secs(10)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    emit(false, started.elapsed());
                }
                emit(true, started.elapsed());
            }) {
            Ok(thread) => {
                ENABLED.store(true, Relaxed);
                Self(Some((tx, thread)))
            }
            Err(error) => {
                log::warn!(target: TARGET, "UI profile reporter unavailable: {error}");
                Self(None)
            }
        }
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        if let Some((tx, thread)) = self.0.take() {
            ENABLED.store(false, Relaxed);
            let _ = tx.try_send(());
            drop(tx);
            let _ = thread.join();
        }
    }
}

fn emit(final_report: bool, elapsed: Duration) {
    let samples = std::array::from_fn(|i| STATS[i].snapshot());
    let counters = std::array::from_fn(|i| COUNTERS[i].load(Relaxed));
    log::trace!(target: TARGET, "{}", report(&samples, counters, final_report, elapsed));
}

fn report(
    samples: &[Sample; NAMES.len()],
    counters: [u64; 6],
    final_report: bool,
    elapsed: Duration,
) -> String {
    let mut out = String::with_capacity(LIMIT);
    let _ = write!(out, "ui profile {} elapsed_s={:.3} cumulative; inclusive/do-not-sum; current-thread CPU, not renderer/GPU; relaxed snapshots; p95~ bucket upper;",
        if final_report { "final" } else { "10s" }, elapsed.as_secs_f64());
    for (name, value) in COUNTER_NAMES.iter().zip(counters) {
        let _ = write!(out, " {name}={value}");
    }
    let mut cpu: [usize; NAMES.len()] = std::array::from_fn(|i| i);
    let mut wall = cpu;
    cpu.sort_unstable_by_key(|i| (std::cmp::Reverse(samples[*i].cpu), *i));
    wall.sort_unstable_by_key(|i| (std::cmp::Reverse(samples[*i].wall), *i));
    let mut selected = [false; NAMES.len()];
    for (name, indices) in [("top5_cpu", cpu), ("top5_wall", wall)] {
        let _ = write!(out, " {name}=[");
        for i in indices
            .into_iter()
            .filter(|i| {
                samples[*i].calls != 0 && (name != "top5_cpu" || samples[*i].cpu_calls != 0)
            })
            .take(5)
        {
            selected[i] = true;
            append(&mut out, i, samples[i]);
        }
        out.push(']');
    }
    if final_report {
        wall.sort_unstable_by_key(|i| (samples[*i].calls, *i));
        out.push_str(" other_active=[");
        for i in wall {
            if samples[i].calls != 0 && !selected[i] {
                append(&mut out, i, samples[i]);
            }
        }
        out.push(']');
    }
    out
}

fn append(out: &mut String, i: usize, s: Sample) {
    if out.len() + 300 > LIMIT - 64 {
        if !out.ends_with(" omitted;") {
            out.push_str(" omitted;");
        }
        return;
    }
    let _ = write!(
        out,
        " {} n={} wall_ms={:.3} max_ms={:.3} p95~_le_ms={:.3} cpu_ms=",
        NAMES[i],
        s.calls,
        s.wall as f64 / 1e6,
        s.max as f64 / 1e6,
        s.p95 as f64 / 1e6
    );
    if s.cpu_calls == 0 {
        out.push_str("unavailable");
    } else {
        let _ = write!(out, "{:.3}", s.cpu as f64 / 1e6);
    }
    let _ = write!(out, " cpu_n={} items={};", s.cpu_calls, s.items);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_profiling_never_reads_clock_or_starts_reporter() {
        let span = Span::start(Phase::Startup, false, || panic!("disabled clock"));
        assert!(span.start.is_none());
        drop(span);
        assert!(Reporter::start().0.is_none());
    }

    #[test]
    fn aggregates_and_reports_are_bounded_and_unknown_cpu_is_not_zero() {
        let stats = Aggregate::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..100 {
                        stats.record(3000, None, 2);
                    }
                });
            }
        });
        let s = stats.snapshot();
        assert_eq!(
            (s.calls, s.wall, s.p95, s.items),
            (400, 1_200_000, 4000, 800)
        );
        let mut text = String::new();
        append(&mut text, 0, s);
        assert!(text.contains("cpu_ms=unavailable"));
        let maximum = Sample {
            calls: u64::MAX,
            wall: u64::MAX,
            max: u64::MAX,
            p95: u64::MAX,
            cpu: u64::MAX,
            cpu_calls: u64::MAX,
            items: u64::MAX,
        };
        for final_report in [false, true] {
            let text = report(
                &[maximum; NAMES.len()],
                [u64::MAX; 6],
                final_report,
                Duration::from_secs(10),
            );
            assert!(text.len() <= LIMIT);
            assert!(text.contains("top5_cpu=[") && text.contains("top5_wall=["));
        }
    }
}
