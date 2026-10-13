/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Fixed-purpose, opt-in daemon diagnostics. All timings are inclusive: nested
//! phases must not be summed. CPU is the calling thread only (not D-Bus's Tokio
//! runtime, Rayon peers or forked probe children). No clocks or reporter exist
//! until the effective `TaskManager::Profile=trace` target is enabled.

use std::fmt::Write;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{mpsc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{Clock, Domain, MetricError, Phase, ProcError, TARGET};

const REPORT_LIMIT: usize = 8 * 1024;
const BUCKETS: usize = 32; // powers of two microseconds, last bucket is overflow
const DOMAINS: usize = 11;
const EXTRA: usize = 18;
const PHASES: usize = 5 * DOMAINS + EXTRA;
static ENABLED: AtomicBool = AtomicBool::new(false);
static CPU_CLOCK: OnceLock<Clock> = OnceLock::new();
static STATS: [Aggregate; PHASES] = [const { Aggregate::new() }; PHASES];
static ERRORS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];

const DOMAIN_NAMES: [&str; DOMAINS] = [
    "about",
    "apps",
    "battery",
    "cpu",
    "disks",
    "fans",
    "gpus",
    "memory",
    "network",
    "processes",
    "services",
];

impl Phase {
    fn index(self) -> usize {
        match self {
            Self::Init(d) => d as usize,
            Self::Request(d) => DOMAINS + d as usize,
            Self::Refresh(d) => 2 * DOMAINS + d as usize,
            Self::Publish(d) => 3 * DOMAINS + d as usize,
            Self::Queue(d) => 4 * DOMAINS + d as usize,
            Self::Startup => 5 * DOMAINS,
            Self::RequestTotal => 5 * DOMAINS + 1,
            Self::ProcessEnumeration => 5 * DOMAINS + 2,
            Self::ProcessMaps => 5 * DOMAINS + 3,
            Self::ProcessScan => 5 * DOMAINS + 4,
            Self::ProcessTree => 5 * DOMAINS + 5,
            Self::ProcessGpuMerge => 5 * DOMAINS + 6,
            Self::GpuInventory => 5 * DOMAINS + 7,
            Self::GpuFdinfo => 5 * DOMAINS + 8,
            Self::GpuNative => 5 * DOMAINS + 9,
            Self::GpuNvml => 5 * DOMAINS + 10,
            Self::GpuNativeInit => 5 * DOMAINS + 11,
            Self::GpuForkedProbes => 5 * DOMAINS + 12,
            Self::GpuNvmlInit => 5 * DOMAINS + 13,
            Self::AppsInventory => 5 * DOMAINS + 14,
            Self::AppsSnapshot => 5 * DOMAINS + 15,
            Self::AppsMatching => 5 * DOMAINS + 16,
            Self::AppsIcons => 5 * DOMAINS + 17,
        }
    }
}

const EXTRA_NAMES: [&str; EXTRA] = [
    "startup.cache",
    "request.total",
    "proc.enumerate",
    "proc.maps",
    "proc.scan",
    "proc.tree",
    "proc.gpu-merge",
    "gpu.inventory",
    "gpu.fdinfo",
    "gpu.native",
    "gpu.nvml",
    "gpu.native-init",
    "gpu.forked-probes",
    "gpu.nvml-init",
    "apps.inventory",
    "apps.snapshot",
    "apps.matching",
    "apps.icons",
];

pub fn enabled() -> bool {
    ENABLED.load(Relaxed)
}

struct Aggregate {
    calls: AtomicU64,
    wall: AtomicU64,
    max: AtomicU64,
    cpu: AtomicU64,
    cpu_calls: AtomicU64,
    items: AtomicU64,
    coalesced: AtomicU64,
    histogram: [AtomicU64; BUCKETS],
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
            coalesced: AtomicU64::new(0),
            histogram: [const { AtomicU64::new(0) }; BUCKETS],
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
        let bucket = (u64::BITS - (micros - 1).leading_zeros()) as usize;
        self.histogram[bucket.min(BUCKETS - 1)].fetch_add(1, Relaxed);
        self.calls.fetch_add(1, Relaxed);
    }

    // Approximate, relaxed snapshots: no locks on collector threads and no reset
    // race. Cumulative counters retain low-frequency startup work at final exit.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            calls: self.calls.load(Relaxed),
            wall: self.wall.load(Relaxed),
            max: self.max.load(Relaxed),
            cpu: self.cpu.load(Relaxed),
            cpu_calls: self.cpu_calls.load(Relaxed),
            items: self.items.load(Relaxed),
            coalesced: self.coalesced.load(Relaxed),
            histogram: std::array::from_fn(|i| self.histogram[i].load(Relaxed)),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Snapshot {
    calls: u64,
    wall: u64,
    max: u64,
    cpu: u64,
    cpu_calls: u64,
    items: u64,
    coalesced: u64,
    histogram: [u64; BUCKETS],
}

impl Snapshot {
    fn active(self) -> bool {
        self.calls != 0 || self.coalesced != 0
    }

    fn p95(self) -> u64 {
        let samples: u64 = self.histogram.iter().sum();
        let rank = samples.saturating_sub(samples / 20);
        let mut seen: u64 = 0;
        for (i, count) in self.histogram.into_iter().enumerate() {
            seen = seen.saturating_add(count);
            if seen >= rank && count != 0 {
                // Last bucket's finite upper bound is the observed max.
                return if i == BUCKETS - 1 {
                    self.max
                } else {
                    (1u64 << i) * 1000
                };
            }
        }
        0
    }
}

/// A same-thread inclusive scope. `!Send` prevents accidental cross-thread CPU deltas.
pub struct Span {
    start: Option<(Instant, Option<u64>, Clock)>,
    phase: Phase,
    items: u64,
    _same_thread: PhantomData<Rc<()>>,
}

impl Span {
    pub fn new(phase: Phase) -> Self {
        Self::start(phase, enabled(), || {
            CPU_CLOCK.get().and_then(|clock| clock())
        })
    }

    fn start(phase: Phase, on: bool, clock: Clock) -> Self {
        Self {
            start: if on {
                Some((Instant::now(), clock(), clock))
            } else {
                None
            },
            phase,
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
        if let Some((wall, cpu, clock)) = self.start {
            let elapsed = clock()
                .zip(cpu)
                .and_then(|(end, start)| end.checked_sub(start));
            STATS[self.phase.index()].record(nanos(wall.elapsed()), elapsed, self.items);
        }
    }
}

fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

/// Transfer only the wall-clock enqueue timestamp to Rayon, never caller CPU.
pub struct QueueTicket(Option<Instant>, Domain);

impl QueueTicket {
    pub fn new(domain: Domain) -> Self {
        Self(
            if enabled() {
                Some(Instant::now())
            } else {
                None
            },
            domain,
        )
    }

    pub fn arrived(self) {
        if let Some(start) = self.0 {
            STATS[Phase::Queue(self.1).index()].record(nanos(start.elapsed()), None, 0);
        }
    }
}

pub fn coalesced(phase: Phase) {
    if enabled() {
        STATS[phase.index()].coalesced.fetch_add(1, Relaxed);
    }
}

fn io_category(error: &std::io::Error) -> ProcError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ProcError::Exited,
        std::io::ErrorKind::PermissionDenied => ProcError::Permission,
        _ if error.raw_os_error() == Some(3) => ProcError::Exited, // Linux ESRCH
        _ => ProcError::Unexpected,
    }
}

/// True means a PID-local diagnostic was aggregated, so omit its identity log.
/// Root/fatal errors are counted separately and retain their existing logs.
pub fn proc_error(category: ProcError) -> bool {
    if !enabled() {
        return false;
    }
    let previous = ERRORS[category as usize].fetch_add(1, Relaxed);
    if previous == 0 && matches!(category, ProcError::Invalid | ProcError::Unexpected) {
        // Preserve unexpected failures without leaking a PID/path/input or
        // logging every affected process. Subsequent occurrences stay counted.
        log::warn!(target: TARGET, "daemon proc diagnostic {category:?}; repeats aggregated");
    }
    category != ProcError::Root
}

pub fn proc_io_error(error: &std::io::Error) -> bool {
    proc_error(io_category(error))
}

/// Preserve one sanitized warning; count repeated optional metric failures.
/// Ordinary logging and metric fallback behavior remain unchanged when disabled.
pub fn metric_error(category: MetricError) -> bool {
    if !enabled() {
        return false;
    }
    if ERRORS[6 + category as usize].fetch_add(1, Relaxed) == 0 {
        log::warn!(target: TARGET, "daemon metric diagnostic {category:?}; repeats aggregated");
    }
    true
}

/// One daemon lifecycle, started only after its logger is initialized. The
/// injected native function must return CLOCK_THREAD_CPUTIME_ID in nanoseconds,
/// or None if unavailable; the platform crate needs no extra dependency/ABI.
pub struct Reporter {
    thread: Option<(mpsc::SyncSender<()>, JoinHandle<()>)>,
}

impl Reporter {
    pub fn start(clock: Clock) -> Self {
        if !log::log_enabled!(target: TARGET, log::Level::Trace) {
            return Self { thread: None };
        }
        let _ = CPU_CLOCK.set(clock);
        let (tx, rx) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("magpie-profile".into())
            .spawn(move || reporter_loop(rx, Duration::from_secs(10), emit))
        {
            Ok(thread) => {
                ENABLED.store(true, Relaxed);
                Self {
                    thread: Some((tx, thread)),
                }
            }
            Err(error) => {
                log::warn!(target: TARGET, "daemon profile reporter unavailable: {error}");
                Self { thread: None }
            }
        }
    }
}

fn reporter_loop(rx: mpsc::Receiver<()>, interval: Duration, mut emit: impl FnMut(bool)) {
    while matches!(
        rx.recv_timeout(interval),
        Err(mpsc::RecvTimeoutError::Timeout)
    ) {
        emit(false);
    }
    emit(true);
}

impl Drop for Reporter {
    fn drop(&mut self) {
        if let Some((stop, thread)) = self.thread.take() {
            // Wake immediately, not after the 10-second reporting deadline.
            // Joining only this reporter never waits on collectors/probes.
            ENABLED.store(false, Relaxed);
            let _ = stop.try_send(());
            drop(stop);
            let _ = thread.join();
        }
    }
}

fn emit(final_report: bool) {
    let snapshots = std::array::from_fn(|i| STATS[i].snapshot());
    let errors = std::array::from_fn(|i| ERRORS[i].load(Relaxed));
    log::trace!(target: TARGET, "{}", report(&snapshots, errors, final_report));
}

fn report(snapshots: &[Snapshot; PHASES], errors: [u64; 9], final_report: bool) -> String {
    let mut out = String::with_capacity(REPORT_LIMIT);
    let _ = write!(out,
        "daemon profile {} cumulative; inclusive/do-not-sum; thread_cpu excludes Tokio peers/children; queue wall-only; relaxed snapshots; p95~ bucket upper; proc_errors exited={} permission={} empty={} invalid={} unexpected={} root={}; metric_errors read={} value={} network_type={};",
        if final_report { "final" } else { "10s" }, errors[0], errors[1], errors[2], errors[3], errors[4], errors[5], errors[6], errors[7], errors[8]);
    let mut cpu: [usize; PHASES] = std::array::from_fn(|i| i);
    let mut wall = cpu;
    cpu.sort_unstable_by_key(|i| (std::cmp::Reverse(snapshots[*i].cpu), *i));
    wall.sort_unstable_by_key(|i| (std::cmp::Reverse(snapshots[*i].wall), *i));
    let mut selected = [false; PHASES];
    // Keep attribution visible even when a small stage misses the tops or the
    // final report fills with initialization phases. Still one bounded record.
    out.push_str(" apps_stages=[");
    for phase in [
        Phase::AppsInventory,
        Phase::AppsSnapshot,
        Phase::AppsMatching,
        Phase::AppsIcons,
    ] {
        let i = phase.index();
        if snapshots[i].active() {
            selected[i] = true;
            append_phase(&mut out, i, snapshots[i]);
        }
    }
    out.push(']');
    for (name, indices) in [("top5_cpu", cpu), ("top5_wall", wall)] {
        let _ = write!(out, " {name}=[");
        for i in indices
            .into_iter()
            .filter(|i| {
                snapshots[*i].calls != 0 && (name != "top5_cpu" || snapshots[*i].cpu_calls != 0)
            })
            .take(5)
        {
            selected[i] = true;
            append_phase(&mut out, i, snapshots[i]);
        }
        out.push(']');
    }
    if final_report {
        // After the tops, prioritize low-call phases (including initialization).
        wall.sort_unstable_by_key(|i| (snapshots[*i].calls, *i));
        out.push_str(" other_active=[");
        for i in wall {
            if snapshots[i].active() && !selected[i] {
                append_phase(&mut out, i, snapshots[i]);
            }
        }
        out.push(']');
    } else {
        let skips = snapshots
            .iter()
            .fold(0u64, |total, s| total.saturating_add(s.coalesced));
        let _ = write!(out, " coalesced_total={skips}");
    }
    out
}

fn append_phase(out: &mut String, i: usize, s: Snapshot) {
    // Reserve room for closing brackets and an explicit truncation marker.
    if out.len() + 360 > REPORT_LIMIT - 64 {
        if !out.ends_with(" omitted;") {
            out.push_str(" omitted;");
        }
        return;
    }
    if i < 5 * DOMAINS {
        let kind = ["init", "request", "refresh", "publish", "queue"][i / DOMAINS];
        let _ = write!(out, " {kind}.{}", DOMAIN_NAMES[i % DOMAINS]);
    } else {
        let _ = write!(out, " {}", EXTRA_NAMES[i - 5 * DOMAINS]);
    }
    let _ = write!(
        out,
        " n={} wall_ms={:.3} max_ms={:.3} p95~_le_ms={:.3} cpu_ms=",
        s.calls,
        s.wall as f64 / 1e6,
        s.max as f64 / 1e6,
        s.p95() as f64 / 1e6
    );
    if s.cpu_calls == 0 {
        out.push_str("unavailable");
    } else {
        let _ = write!(out, "{:.3}", s.cpu as f64 / 1e6);
    }
    let _ = write!(
        out,
        " cpu_n={} items={} coalesced={};",
        s.cpu_calls, s.items, s.coalesced
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_span_and_queue_do_not_read_clocks() {
        fn forbidden() -> Option<u64> {
            panic!("disabled CPU clock read")
        }
        let span = Span::start(Phase::Startup, false, forbidden);
        assert!(span.start.is_none());
        drop(span);
        assert!(QueueTicket::new(Domain::Cpu).0.is_none());
        let reporter = Reporter::start(forbidden);
        assert!(reporter.thread.is_none());
        assert!(!metric_error(MetricError::Read));
    }

    #[test]
    fn reporter_stops_and_joins_with_one_final_report() {
        let counts = std::sync::Arc::new((AtomicU64::new(0), AtomicU64::new(0)));
        let thread_counts = counts.clone();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (stop_tx, stop_rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            reporter_loop(stop_rx, Duration::ZERO, |final_report| {
                if final_report {
                    thread_counts.1.fetch_add(1, Relaxed);
                } else {
                    thread_counts.0.fetch_add(1, Relaxed);
                    let _ = started_tx.try_send(());
                }
            });
        });
        started_rx.recv().unwrap();
        drop(Reporter {
            thread: Some((stop_tx, thread)),
        });
        assert!(counts.0.load(Relaxed) >= 1);
        assert_eq!(counts.1.load(Relaxed), 1);
    }

    #[test]
    fn unavailable_cpu_clock_scope_records_wall_without_a_cpu_sample() {
        let stats = &STATS[Phase::GpuNvmlInit.index()];
        let before = stats.snapshot();
        let scope = Span::start(Phase::GpuNvmlInit, true, || None);
        assert!(scope.start.is_some());
        drop(scope);
        let after = stats.snapshot();
        assert_eq!(after.calls, before.calls + 1);
        assert_eq!(after.cpu_calls, before.cpu_calls);
    }

    #[test]
    fn unavailable_cpu_is_not_zero_and_histogram_has_upper_bounds() {
        let stats = Aggregate::new();
        for _ in 0..20 {
            stats.record(3_000, None, 2);
        }
        let s = stats.snapshot();
        assert_eq!(
            (s.calls, s.wall, s.max, s.cpu_calls, s.items),
            (20, 60_000, 3_000, 0, 40)
        );
        assert_eq!(s.p95(), 4_000);
        let mut text = String::new();
        append_phase(&mut text, 0, s);
        assert!(text.contains("cpu_ms=unavailable"));
        let mut snapshots = [Snapshot::default(); PHASES];
        snapshots[0] = s;
        assert!(report(&snapshots, [0; 9], false).contains("top5_cpu=[]"));
        stats.record(u64::MAX, Some(0), 0);
        assert_eq!(stats.snapshot().cpu_calls, 1);
    }

    #[test]
    fn concurrent_aggregation_keeps_counts() {
        let stats = Aggregate::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..1000 {
                        stats.record(2000, Some(1000), 3);
                    }
                });
            }
        });
        let s = stats.snapshot();
        assert_eq!(
            (s.calls, s.wall, s.cpu, s.cpu_calls, s.items),
            (4000, 8_000_000, 4_000_000, 4000, 12000)
        );
        assert_eq!(s.histogram.iter().sum::<u64>(), 4000);
    }

    #[test]
    fn report_is_bounded_even_at_max_counts_and_preserves_rare_phases() {
        let mut snapshots = [Snapshot {
            calls: u64::MAX,
            wall: u64::MAX,
            max: u64::MAX,
            cpu: u64::MAX,
            cpu_calls: u64::MAX,
            items: u64::MAX,
            coalesced: u64::MAX,
            histogram: [0; BUCKETS],
        }; PHASES];
        snapshots[Phase::GpuForkedProbes.index()].calls = 1;
        for final_report in [false, true] {
            let text = report(&snapshots, [u64::MAX; 9], final_report);
            assert!(text.len() <= REPORT_LIMIT, "{}", text.len());
            assert!(text.contains("top5_cpu=[") && text.contains("top5_wall=["));
            if final_report {
                assert!(text.contains("gpu.forked-probes"));
            }
        }
    }

    #[test]
    fn proc_errors_distinguish_churn_from_unexpected_io() {
        assert_eq!(
            io_category(&std::io::Error::from(std::io::ErrorKind::NotFound)),
            ProcError::Exited
        );
        assert_eq!(
            io_category(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            ProcError::Permission
        );
        assert_eq!(
            io_category(&std::io::Error::from(std::io::ErrorKind::InvalidData)),
            ProcError::Unexpected
        );
    }

    #[test]
    fn apps_stages_remain_visible_in_bounded_reports() {
        let snapshots = [Snapshot {
            calls: u64::MAX,
            wall: u64::MAX,
            max: u64::MAX,
            cpu: u64::MAX,
            cpu_calls: u64::MAX,
            items: u64::MAX,
            coalesced: u64::MAX,
            histogram: [0; BUCKETS],
        }; PHASES];
        for final_report in [false, true] {
            let text = report(&snapshots, [u64::MAX; 9], final_report);
            assert!(text.len() <= REPORT_LIMIT);
            for name in [
                "apps.inventory",
                "apps.snapshot",
                "apps.matching",
                "apps.icons",
            ] {
                assert!(text.contains(name), "missing {name}");
            }
        }
    }
}
