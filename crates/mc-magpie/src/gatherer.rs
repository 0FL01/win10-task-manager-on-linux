/* crates/mc-magpie/src/gatherer.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::sync::atomic::{self, AtomicBool, AtomicU64};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::client::{
    App, Battery, Client, Connection, Cpu, Disk, ErrorEjectFailed, Fan, Gpu, Memory, MemoryDevice,
    Process, Service, SmartData,
};
use crate::log_compat::{g_critical, g_debug, g_warning};
use crate::profile::{self, Counter, Phase, Span};
use crate::{About, Icon, Pid, BASE_INTERVAL, INTERVAL_STEP};

/// Callback used to wake the consumer (typically `egui::Context::request_repaint`).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Capacity of the gatherer to UI event channel.
const EVENT_CAPACITY: usize = 64;
/// Capacity of the UI to gatherer request channel.
const REQUEST_CAPACITY: usize = 64;

/// One complete snapshot of the system state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Readings {
    /// Old daemons/fixtures have completed inventories. Pending is not hot-unplug.
    #[serde(default)]
    pub inventories_pending: bool,
    #[serde(default)]
    pub hardware: Option<crate::HardwareTelemetry>,
    /// Access validity generation; zero preserves legacy recorded fixtures.
    #[serde(default)]
    pub hardware_epoch: u64,
    pub cpu: Cpu,
    pub mem_info: Memory,
    pub mem_devices: Vec<MemoryDevice>,
    pub disks_info: Vec<Disk>,
    /// Cached, noninteractive SMART snapshots supplied by the host daemon.
    #[serde(default)]
    pub smart_data: HashMap<String, SmartData>,
    pub network_connections: HashMap<String, Connection>,
    pub gpus: HashMap<String, Gpu>,
    pub fans: Vec<Fan>,
    pub batteries: Vec<Battery>,

    pub running_apps: HashMap<String, App>,
    pub running_processes: HashMap<u32, Process>,
    /// Owner of each process (`pid → user name`), read from `/proc` by the gatherer
    #[serde(default)]
    pub process_users: HashMap<u32, String>,
    /// Per-process network usage could not be measured (netlink `sock_diag` unavailable). When
    /// available it counts TCP traffic of the user's own processes only (see `sockets`).
    #[serde(default)]
    pub network_unavailable: bool,

    pub user_services: HashMap<u64, Service>,
    pub system_services: HashMap<u64, Service>,
    pub includes_app_data: bool,
}

impl Default for Readings {
    fn default() -> Self {
        Self::new()
    }
}

impl Readings {
    pub fn new() -> Self {
        Self {
            inventories_pending: false,
            hardware: None,
            hardware_epoch: 0,
            cpu: Default::default(),
            mem_info: Memory::default(),
            mem_devices: vec![],
            disks_info: vec![],
            smart_data: HashMap::new(),
            network_connections: Default::default(),
            gpus: HashMap::new(),
            fans: vec![],
            batteries: vec![],

            running_apps: HashMap::new(),
            running_processes: HashMap::new(),
            process_users: HashMap::new(),
            network_unavailable: false,

            user_services: HashMap::new(),
            system_services: HashMap::new(),
            includes_app_data: true,
        }
    }
}

/// Requests from the UI. None of them block the caller; results arrive as [`Event`]s.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    TerminateProcesses(Vec<Pid>),
    KillProcesses(Vec<Pid>),
    ServiceLogs(u64, Option<NonZeroU32>),
    StartService(u64),
    StopService(u64),
    RestartService(u64),
    EnableService(u64),
    DisableService(u64),
    EjectDisk(String),
    SmartData(String),
    AboutSystem,
    ReadSpd {
        id: u64,
    },
    InstallHardwareService {
        id: u64,
    },
    SetHardwareAccess {
        id: u64,
        enabled: bool,
        interactive: bool,
    },
    CancelHardware {
        id: u64,
    },
    /// Ends the current wait: the next snapshot is gathered immediately
    RefreshNow,
}

/// Results and notifications from the gatherer thread.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Event {
    IconsReady(HashMap<String, Icon>),
    ServiceLogs {
        service_id: u64,
        logs: String,
    },
    EjectResult {
        disk_id: String,
        result: Result<(), ErrorEjectFailed>,
    },
    SmartData {
        disk_id: String,
        data: Option<SmartData>,
    },
    About(About),
    HardwareAccess {
        /// None is an unsolicited sampling/status notification, not an acknowledgement.
        id: Option<u64>,
        status: crate::HardwareStatus,
        result: Result<(), String>,
    },
    Spd {
        id: u64,
        epoch: u64,
        privileged: bool,
        result: Result<Vec<crate::SpdEeprom>, String>,
    },
    /// The gatherer hit an unrecoverable error; the application should show it and exit.
    Fatal(String),
}

struct Outbox {
    readings: Arc<crate::LatestSlot<Readings>>,
    events: SyncSender<Event>,
    waker: Waker,
}

impl Outbox {
    fn publish(&self, readings: Readings) {
        let before = profile::enabled().then(|| self.readings.dropped());
        self.readings.publish(readings);
        profile::count(Counter::Published);
        if before.is_some_and(|before| self.readings.dropped() > before) {
            profile::count(Counter::Overwritten);
        }
        (self.waker)();
    }

    /// Preserves the event and whether delivery failed because the receiver is full or gone.
    #[allow(clippy::result_large_err)]
    fn send(&self, event: Event) -> Result<(), TrySendError<Event>> {
        let result = self.events.try_send(event);
        if let Err(TrySendError::Full(event)) = &result {
            if profile::enabled() {
                profile::count(Counter::EventFull);
            } else if matches!(event, Event::About(_)) {
                // Live About delivery owns retries; this is not a dropped response.
                g_debug!("TaskManager::Gatherer", "UI About event channel is full");
            } else {
                g_warning!(
                    "TaskManager::Gatherer",
                    "UI is not draining events, dropping {:?}",
                    std::mem::discriminant(event)
                );
            }
        }
        (self.waker)();
        result
    }
}

const ABOUT_RETRY: Duration = Duration::from_millis(100);

// There is one retained response per gatherer, so keep it inline rather than allocating a box
// solely to shrink this single state value.
#[allow(clippy::large_enum_variant)]
#[derive(Default)]
enum AboutDelivery {
    #[default]
    Idle,
    Polling {
        next: Instant,
    },
    Ready {
        about: About,
        next: Instant,
    },
}

impl AboutDelivery {
    fn request(&mut self, now: Instant) {
        if matches!(self, Self::Idle) {
            *self = Self::Polling { next: now };
        }
    }

    fn deadline(&self) -> Option<Instant> {
        match self {
            Self::Idle => None,
            Self::Polling { next } | Self::Ready { next, .. } => Some(*next),
        }
    }

    /// At most one fetch and one delivery. A completed response is never fetched again while
    /// delivery is blocked, and each retry delay starts after the attempt finishes.
    fn service(
        &mut self,
        outbox: &Outbox,
        mut fetch: impl FnMut() -> Option<About>,
        mut now: impl FnMut() -> Instant,
    ) -> bool {
        let Some(next) = self.deadline() else {
            return true;
        };
        if now() < next {
            return true;
        }

        let about = match std::mem::take(self) {
            Self::Polling { .. } => {
                let _profile = Span::new(Phase::Action);
                match fetch() {
                    Some(about) if about.ready != Some(false) => about,
                    _ => {
                        *self = Self::Polling {
                            next: now() + ABOUT_RETRY,
                        };
                        return true;
                    }
                }
            }
            Self::Ready { about, .. } => about,
            Self::Idle => unreachable!("idle About delivery has no deadline"),
        };

        match outbox.send(Event::About(about)) {
            Ok(()) => true,
            Err(TrySendError::Full(Event::About(about))) => {
                *self = Self::Ready {
                    about,
                    next: now() + ABOUT_RETRY,
                };
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
            Err(TrySendError::Full(_)) => unreachable!("only an About event was sent"),
        }
    }
}

pub struct MagpieClient {
    speed: Arc<AtomicU64>,
    reduced_activity: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,

    requests: SyncSender<Request>,
    events: Receiver<Event>,
    readings: Arc<crate::LatestSlot<Readings>>,
    hardware: crate::hardware_worker::Handle,
}

impl Drop for MagpieClient {
    fn drop(&mut self) {
        self.running.store(false, atomic::Ordering::Release);

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl MagpieClient {
    /// Starts the Magpie daemon and the gatherer thread.
    pub fn new(waker: Waker) -> Self {
        Self::spawn(waker, true, gather_from_magpie)
    }

    /// Replays recorded readings instead of talking to Magpie; useful for tests and benchmarks.
    pub fn from_fixtures(
        readings: Vec<Readings>,
        icons: HashMap<String, Icon>,
        waker: Waker,
    ) -> Self {
        Self::spawn(waker, false, move |ctx| {
            replay_fixtures(ctx, readings, icons)
        })
    }

    fn spawn(
        waker: Waker,
        hardware_available: bool,
        body: impl FnOnce(ThreadCtx) + Send + 'static,
    ) -> Self {
        let speed = Arc::new(AtomicU64::new(
            (BASE_INTERVAL / INTERVAL_STEP).round() as u64
        ));
        let reduced_activity = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let readings = Arc::new(crate::LatestSlot::new());

        let (req_tx, req_rx) = mpsc::sync_channel(REQUEST_CAPACITY);
        let (ev_tx, ev_rx) = mpsc::sync_channel(EVENT_CAPACITY);
        let hardware = crate::hardware_worker::Handle::spawn(
            hardware_available,
            running.clone(),
            speed.clone(),
            ev_tx.clone(),
            waker.clone(),
        );

        {
            let ev_tx = ev_tx.clone();
            let waker = waker.clone();
            crate::set_fatal_hook(Box::new(move |message| {
                let _ = ev_tx.try_send(Event::Fatal(message));
                waker();
            }));
        }

        let ctx = ThreadCtx {
            requests: req_rx,
            outbox: Outbox {
                readings: readings.clone(),
                events: ev_tx,
                waker,
            },
            running: running.clone(),
            speed: speed.clone(),
            reduced_activity: reduced_activity.clone(),
            hardware_control: hardware.control.clone(),
            hardware_samples: hardware.samples.clone(),
        };

        let thread = std::thread::Builder::new()
            .name("mc-gatherer".into())
            .spawn(move || body(ctx))
            .expect("Failed to spawn the gatherer thread");

        Self {
            speed,
            reduced_activity,
            running,
            thread: Some(thread),
            requests: req_tx,
            events: ev_rx,
            readings,
            hardware,
        }
    }

    /// Refresh interval in multiples of [`INTERVAL_STEP`].
    pub fn set_update_speed(&self, speed: u64) {
        self.speed.store(speed.max(1), atomic::Ordering::Release);
    }

    /// When set, processes, apps and services are not refreshed.
    pub fn set_reduced_activity(&self, reduced: bool) {
        self.reduced_activity
            .store(reduced, atomic::Ordering::Release);
    }

    /// Queues a request without blocking. Returns `false` if it had to be dropped.
    pub fn send(&self, request: Request) -> bool {
        use crate::HardwareOperation as Op;
        match request {
            Request::ReadSpd { id } => return self.hardware.submit(id, Op::Spd),
            Request::InstallHardwareService { id } => return self.hardware.submit(id, Op::Install),
            Request::SetHardwareAccess {
                id,
                enabled,
                interactive,
            } => {
                return self.hardware.submit(
                    id,
                    if enabled {
                        Op::Enable { interactive }
                    } else {
                        Op::Disable
                    },
                )
            }
            Request::CancelHardware { id } => return self.hardware.cancel(id),
            _ => {}
        }
        match self.requests.try_send(request) {
            Ok(()) => true,
            Err(TrySendError::Full(request)) => {
                if profile::enabled() {
                    profile::count(Counter::RequestFull);
                } else {
                    g_critical!(
                        "TaskManager::SysInfo",
                        "Gatherer request queue is full, dropping {request:?}"
                    );
                }
                false
            }
            Err(TrySendError::Disconnected(request)) => {
                g_critical!(
                    "TaskManager::SysInfo",
                    "Gatherer is gone, dropping {request:?}"
                );
                false
            }
        }
    }

    /// Most recent snapshot published since the last call.
    pub fn take_readings(&self) -> Option<Readings> {
        self.readings.take()
    }

    pub fn try_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Nonblocking invalidation even when critical events are waiting behind UI backpressure.
    pub fn hardware_epoch(&self) -> u64 {
        self.hardware.control.epoch.load(atomic::Ordering::Acquire)
    }

    /// `(published, overwritten before being consumed)` snapshot counters.
    pub fn readings_stats(&self) -> (u64, u64) {
        (self.readings.published(), self.readings.dropped())
    }
}

struct ThreadCtx {
    requests: Receiver<Request>,
    outbox: Outbox,
    running: Arc<AtomicBool>,
    speed: Arc<AtomicU64>,
    reduced_activity: Arc<AtomicBool>,
    hardware_control: Arc<crate::hardware_access::Control>,
    hardware_samples: Arc<crate::LatestSlot<crate::hardware_worker::Sample>>,
}

impl ThreadCtx {
    fn running(&self) -> bool {
        self.running.load(atomic::Ordering::Acquire)
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(
            ((self.speed.load(atomic::Ordering::Relaxed) as f64 * INTERVAL_STEP) * 1000.) as u64,
        )
    }

    /// Services requests until `deadline` (or [`Request::RefreshNow`]), in slices so `running`
    /// is checked regularly.
    fn serve_until(&self, deadline: Instant, mut handle: impl FnMut(Request)) -> bool {
        const SLICE: Duration = Duration::from_millis(100);

        loop {
            if !self.running() {
                return false;
            }

            let now = Instant::now();
            if now >= deadline {
                return true;
            }

            match self.requests.recv_timeout((deadline - now).min(SLICE)) {
                Ok(Request::RefreshNow) => return true,
                Ok(request) => handle(request),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    /// Live-only wait: About retries stay inside this wait, independent of the metrics cadence.
    /// The private fetch/clock closures allow testing this loop without starting the daemon.
    fn serve_magpie_until(
        &self,
        deadline: Instant,
        about: &mut AboutDelivery,
        mut fetch_about: impl FnMut() -> Option<About>,
        mut handle: impl FnMut(Request),
        mut now: impl FnMut() -> Instant,
    ) -> bool {
        const SLICE: Duration = Duration::from_millis(100);

        loop {
            if !self.running() {
                return false;
            }
            // Service admitted About work even if the metrics deadline has already expired.
            if !about.service(&self.outbox, &mut fetch_about, &mut now) || !self.running() {
                return false;
            }

            let current = now();
            if current >= deadline {
                return true;
            }
            let wake_at = about.deadline().map_or(deadline, |next| next.min(deadline));
            let timeout = wake_at.saturating_duration_since(current).min(SLICE);
            match self.requests.recv_timeout(timeout) {
                Ok(Request::RefreshNow) => return true,
                Ok(Request::AboutSystem) => about.request(now()),
                Ok(request) => handle(request),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }
}

fn handle_request(magpie: &Client, outbox: &Outbox, request: Request) {
    let _profile = Span::new(Phase::Action);
    match request {
        // Handled by the live wait loop.
        Request::RefreshNow | Request::AboutSystem => {}
        Request::TerminateProcesses(pids) => magpie.terminate_processes(pids),
        Request::KillProcesses(pids) => magpie.kill_processes(pids),
        Request::StartService(id) => magpie.start_service(id),
        Request::StopService(id) => magpie.stop_service(id),
        Request::RestartService(id) => magpie.restart_service(id),
        Request::EnableService(id) => magpie.enable_service(id),
        Request::DisableService(id) => magpie.disable_service(id),
        Request::ServiceLogs(service_id, pid) => {
            let logs = magpie.service_logs(service_id, pid);
            let _ = outbox.send(Event::ServiceLogs { service_id, logs });
        }
        Request::EjectDisk(disk_id) => {
            let result = magpie.eject_disk(disk_id.clone());
            let _ = outbox.send(Event::EjectResult { disk_id, result });
        }
        Request::SmartData(disk_id) => {
            let data = magpie.smart_data(disk_id.clone());
            let _ = outbox.send(Event::SmartData { disk_id, data });
        }
        Request::SetHardwareAccess { .. }
        | Request::InstallHardwareService { .. }
        | Request::ReadSpd { .. }
        | Request::CancelHardware { .. } => {}
    }
}

macro_rules! timed {
    ($what: literal, $phase: ident, $e: expr) => {{
        let _profile = Span::new(Phase::$phase);
        let timer = Instant::now();
        let result = $e;
        g_debug!(
            "TaskManager::Perf",
            concat!($what, " load took: {:?}"),
            timer.elapsed()
        );
        result
    }};
}

fn gather_from_magpie(ctx: ThreadCtx) {
    let startup = Span::new(Phase::GatherStartup);
    let magpie = Client::new();
    magpie.start();

    // Memory devices do not change while running, read them only once
    let mem_devices = magpie.memory_devices();
    let mut known_icons = HashSet::<String>::new();
    let mut users = crate::users::UserResolver::default();
    let mut sockets = crate::sockets::SocketTraffic::default();
    let mut hardware_sample: Option<crate::hardware_worker::Sample> = None;
    let mut trimmer = crate::footprint::Trimmer::new();
    let ready_at = Instant::now();
    let mut startup_refresh = 0;
    let mut about = AboutDelivery::default();
    drop(startup);

    while ctx.running() {
        trimmer.tick();
        let cycle_start = Instant::now();
        let mut cycle = Span::new(Phase::GatherCycle);
        let reduced = ctx.reduced_activity.load(atomic::Ordering::Acquire);

        let mut readings = Readings::new();
        readings.mem_devices = mem_devices.clone();
        readings.includes_app_data = !reduced;
        // Sets Client's processor-count normalization before reading process percentages.
        readings.cpu = timed!("CPU info", Cpu, magpie.cpu());

        if !reduced {
            readings.running_processes = timed!("Process", Processes, magpie.processes());
            readings.process_users = timed!(
                "Process users",
                Users,
                users.resolve(&readings.running_processes)
            );
            // Per-process network traffic from TCP socket counters, no privileges needed
            readings.network_unavailable = !timed!(
                "Socket traffic",
                Sockets,
                sockets.sample(&mut readings.running_processes)
            );
            readings.running_apps = timed!("Running apps", Apps, magpie.apps());
        }

        let disks = timed!("Disks info", Disks, magpie.disks_snapshot());
        readings.inventories_pending = disks.ready == Some(false);
        readings.disks_info = disks.disks;
        readings.smart_data = disks.smart_data;
        let gpus = timed!("GPU info", Gpus, magpie.gpus_snapshot());
        readings.inventories_pending |= gpus.ready == Some(false);
        readings.gpus = gpus.gpus;
        readings.mem_info = timed!("Memory info", Memory, magpie.memory());
        if let Some(sample) = ctx.hardware_samples.take() {
            hardware_sample = Some(sample);
        }
        readings.hardware_epoch = ctx.hardware_control.epoch.load(atomic::Ordering::Acquire);
        if ctx
            .hardware_control
            .sampling
            .load(atomic::Ordering::Acquire)
        {
            if let Some(sample) = &hardware_sample {
                if sample.epoch == readings.hardware_epoch
                    && sample.at.elapsed() <= ctx.interval() * 3
                {
                    readings.hardware.clone_from(&sample.data);
                }
            }
        }
        readings.network_connections = timed!(
            "Network devices info",
            Network,
            magpie.network_connections()
        );
        readings.fans = timed!("Fans info", Fans, magpie.fans_info());
        readings.batteries = timed!("Batteries info", Batteries, magpie.batteries_info());

        if !reduced {
            readings.user_services = timed!("User services", UserServices, magpie.user_services());
            readings.system_services =
                timed!("System services", SystemServices, magpie.system_services());

            let _profile = Span::new(Phase::GatherIcons);
            let missing = readings
                .running_apps
                .keys()
                .filter(|id| !known_icons.contains(*id))
                .cloned()
                .collect::<Vec<_>>();

            if !missing.is_empty() {
                let icons = magpie.app_icons(missing.clone());
                // Early app matching can finish before its icon batch. Remember only returned
                // entries so a later snapshot retries icons still being resolved.
                known_icons.extend(icons.keys().cloned());

                if !icons.is_empty() {
                    if let Err(
                        TrySendError::Full(Event::IconsReady(icons))
                        | TrySendError::Disconnected(Event::IconsReady(icons)),
                    ) = ctx.outbox.send(Event::IconsReady(icons))
                    {
                        // Ask again during the next cycle
                        for id in icons.keys() {
                            known_icons.remove(id);
                        }
                    }
                }
            }
        }

        let sort = Span::new(Phase::DiskSort);
        readings
            .disks_info
            .sort_unstable_by(|d1, d2| d1.id.cmp(&d2.id));
        drop(sort);

        if !ctx.running() {
            break;
        }

        cycle.items(readings.running_processes.len());
        if ready_at.elapsed() < Duration::from_secs(1) {
            g_debug!("TaskManager::Startup", "sample processes={} apps={} cpu_threads={} memory_bytes={} disks={} gpus={} pending={}",
                readings.running_processes.len(), readings.running_apps.len(),
                readings.cpu.core_usage_percent.len(), readings.mem_info.mem_total,
                readings.disks_info.len(), readings.gpus.len(), readings.inventories_pending);
        }
        ctx.outbox.publish(readings);
        drop(cycle);

        g_debug!(
            "TaskManager::Perf",
            "Read cycle took {:?}",
            cycle_start.elapsed()
        );

        let deadline = startup_deadline(ready_at, &mut startup_refresh)
            .unwrap_or_else(|| cycle_start + ctx.interval());
        if Instant::now() > deadline {
            profile::count(Counter::Overrun);
        }
        if !ctx.serve_magpie_until(
            deadline,
            &mut about,
            || magpie.try_about(),
            |request| handle_request(&magpie, &ctx.outbox, request),
            Instant::now,
        ) {
            break;
        }
    }
}

/// Three bounded startup follow-ups fill independently collected device/icon caches and collect
/// short-interval counter deltas. There is no startup polling after the half-second warmup.
fn startup_deadline(ready_at: Instant, next: &mut usize) -> Option<Instant> {
    let offset = [100, 250, 500].get(*next)?;
    *next += 1;
    Some(ready_at + Duration::from_millis(*offset))
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    #[test]
    fn warmup_is_bounded_and_does_not_replace_normal_refresh_interval() {
        let start = Instant::now();
        let mut next = 0;
        for offset in [100, 250, 500] {
            assert_eq!(
                startup_deadline(start, &mut next),
                Some(start + Duration::from_millis(offset))
            );
        }
        for _ in 0..10 {
            assert_eq!(startup_deadline(start, &mut next), None);
        }
    }
}

#[cfg(test)]
mod about_delivery_tests {
    use super::*;
    use std::cell::Cell;

    const WATCHDOG: Duration = Duration::from_secs(3);

    fn outbox(capacity: usize, waker: Waker) -> (Outbox, Receiver<Event>) {
        let (events, receiver) = mpsc::sync_channel(capacity);
        (
            Outbox {
                readings: Arc::new(crate::LatestSlot::new()),
                events,
                waker,
            },
            receiver,
        )
    }

    fn context() -> (ThreadCtx, SyncSender<Request>, Receiver<Event>) {
        let running = Arc::new(AtomicBool::new(true));
        let (requests, receiver) = mpsc::sync_channel(REQUEST_CAPACITY);
        let (outbox, events) = outbox(EVENT_CAPACITY, Arc::new(|| {}));
        (
            ThreadCtx {
                requests: receiver,
                outbox,
                running: running.clone(),
                speed: Arc::new(AtomicU64::new(200)),
                reduced_activity: Arc::new(AtomicBool::new(false)),
                hardware_control: Arc::new(crate::hardware_access::Control::new(running)),
                hardware_samples: Arc::new(crate::LatestSlot::new()),
            },
            requests,
            events,
        )
    }

    #[test]
    fn pending_and_error_retry_from_fetch_completion_without_postponement() {
        let (outbox, events) = outbox(1, Arc::new(|| {}));
        let start = Instant::now();
        let current = Cell::new(start);
        let mut about = AboutDelivery::default();
        assert_eq!(about.deadline(), None);
        about.request(start);

        for ready in [Some(false), None] {
            let completed_at = current.get() + Duration::from_secs(2);
            assert!(about.service(
                &outbox,
                || {
                    current.set(completed_at);
                    ready.map(|ready| About {
                        ready: Some(ready),
                        ..Default::default()
                    })
                },
                || current.get(),
            ));
            let next = completed_at + ABOUT_RETRY;
            assert_eq!(about.deadline(), Some(next));
            assert!(events.try_recv().is_err());

            about.request(completed_at + Duration::from_millis(50));
            assert_eq!(about.deadline(), Some(next));
            current.set(next - Duration::from_nanos(1));
            assert!(about.service(&outbox, || panic!("not due"), || current.get()));
            current.set(next);
        }

        // A successful legacy response is complete, not an error-shaped default.
        assert!(about.service(&outbox, || Some(About::default()), || current.get()));
        let Event::About(delivered) = events.try_recv().unwrap() else {
            panic!("expected About");
        };
        assert_eq!(delivered, About::default());
        assert_eq!(about.deadline(), None);
        assert!(about.service(
            &outbox,
            || panic!("completed discovery has no timer"),
            || start + Duration::from_secs(60),
        ));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn bounded_backpressure_retains_the_exact_response_and_only_retries_delivery() {
        let start = Instant::now();
        let elapsed_ms = Arc::new(AtomicU64::new(0));
        let waker_time = elapsed_ms.clone();
        let (outbox, events) = outbox(
            1,
            Arc::new(move || {
                // Delivery completion, including the waker, happens after the fetch finishes.
                waker_time.fetch_add(40, atomic::Ordering::Relaxed);
            }),
        );
        outbox
            .events
            .try_send(Event::IconsReady(HashMap::new()))
            .unwrap();
        let mut completed = About {
            ready: Some(true),
            ..Default::default()
        };
        completed.os_info.name = Some("retained response allocation".into());
        let expected = completed.clone();
        let name_pointer = completed.os_info.name.as_ref().unwrap().as_ptr();
        let mut fetched = Some(completed);
        let now = || start + Duration::from_millis(elapsed_ms.load(atomic::Ordering::Relaxed));
        let mut about = AboutDelivery::default();
        about.request(start);
        assert!(about.service(
            &outbox,
            || {
                elapsed_ms.store(500, atomic::Ordering::Relaxed);
                fetched.take()
            },
            now,
        ));
        let AboutDelivery::Ready {
            about: retained, ..
        } = &about
        else {
            panic!("full channel must retain completed About");
        };
        assert_eq!(retained, &expected);
        assert_eq!(
            retained.os_info.name.as_ref().unwrap().as_ptr(),
            name_pointer
        );
        assert_eq!(about.deadline(), Some(start + Duration::from_millis(640)));
        about.request(start + Duration::from_millis(600));
        assert_eq!(about.deadline(), Some(start + Duration::from_millis(640)));

        elapsed_ms.store(639, atomic::Ordering::Relaxed);
        assert!(about.service(&outbox, || panic!("must not refetch"), now));
        elapsed_ms.store(640, atomic::Ordering::Relaxed);
        assert!(about.service(&outbox, || panic!("must not refetch"), now));
        assert_eq!(about.deadline(), Some(start + Duration::from_millis(780)));

        assert!(matches!(events.try_recv(), Ok(Event::IconsReady(_))));
        elapsed_ms.store(780, atomic::Ordering::Relaxed);
        assert!(about.service(&outbox, || panic!("must not refetch"), now));
        let Event::About(delivered) = events.try_recv().unwrap() else {
            panic!("expected retained About");
        };
        assert_eq!(delivered, expected);
        assert_eq!(
            delivered.os_info.name.as_ref().unwrap().as_ptr(),
            name_pointer
        );
        assert_eq!(elapsed_ms.load(atomic::Ordering::Relaxed), 820);
        assert_eq!(about.deadline(), None);
    }

    #[test]
    fn disconnected_delivery_stops_without_refetching() {
        let (outbox, events) = outbox(1, Arc::new(|| {}));
        outbox
            .events
            .try_send(Event::IconsReady(HashMap::new()))
            .unwrap();
        let start = Instant::now();
        let mut about = AboutDelivery::default();
        about.request(start);
        assert!(about.service(&outbox, || Some(About::default()), || start));
        drop(events);
        assert!(!about.service(
            &outbox,
            || panic!("must not refetch"),
            || start + ABOUT_RETRY,
        ));
        assert_eq!(about.deadline(), None);
    }

    #[test]
    fn live_about_retries_stay_in_the_same_long_metrics_wait() {
        let (ctx, requests, events) = context();
        ctx.outbox.publish(Readings::new());
        let readings = ctx.outbox.readings.clone();
        let (waiting, waiting_rx) = mpsc::sync_channel(1);
        let (attempts, attempts_rx) = mpsc::sync_channel(3);
        let (actions, actions_rx) = mpsc::sync_channel(1);
        let (returned, returned_rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut about = AboutDelivery::default();
            let mut fetches = 0;
            let mut first_clock = true;
            let result = ctx.serve_magpie_until(
                Instant::now() + Duration::from_secs(10),
                &mut about,
                || {
                    fetches += 1;
                    attempts.try_send(fetches).unwrap();
                    match fetches {
                        1 => Some(About {
                            ready: Some(false),
                            ..Default::default()
                        }),
                        2 => None,
                        3 => Some(About {
                            ready: Some(true),
                            ..Default::default()
                        }),
                        _ => panic!("completed About must not be polled"),
                    }
                },
                |request| {
                    assert_eq!(request, Request::SmartData("probe".into()));
                    actions.try_send(()).unwrap();
                },
                || {
                    if first_clock {
                        first_clock = false;
                        waiting.try_send(()).unwrap();
                    }
                    Instant::now()
                },
            );
            returned.send((result, fetches, about.deadline())).unwrap();
        });

        waiting_rx.recv_timeout(WATCHDOG).unwrap();
        requests.send(Request::AboutSystem).unwrap();
        for attempt in 1..=3 {
            assert_eq!(attempts_rx.recv_timeout(WATCHDOG).unwrap(), attempt);
            assert!(matches!(
                returned_rx.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
        }
        assert!(matches!(
            events.recv_timeout(WATCHDOG),
            Ok(Event::About(About {
                ready: Some(true),
                ..
            }))
        ));
        assert_eq!(
            readings.published(),
            1,
            "About timers must not publish metrics"
        );
        requests.send(Request::SmartData("probe".into())).unwrap();
        actions_rx.recv_timeout(WATCHDOG).unwrap();
        requests.send(Request::RefreshNow).unwrap();
        assert_eq!(returned_rx.recv_timeout(WATCHDOG).unwrap(), (true, 3, None));
        thread.join().unwrap();
        assert_eq!(readings.published(), 1);
    }

    #[test]
    fn live_wait_observes_stop_without_waiting_for_metrics() {
        let (ctx, _requests, _events) = context();
        let running = ctx.running.clone();
        let (waiting, waiting_rx) = mpsc::sync_channel(1);
        let (returned, returned_rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut first_clock = true;
            let result = ctx.serve_magpie_until(
                Instant::now() + Duration::from_secs(10),
                &mut AboutDelivery::default(),
                || panic!("no About request"),
                |_| panic!("no action request"),
                || {
                    if first_clock {
                        first_clock = false;
                        waiting.try_send(()).unwrap();
                    }
                    Instant::now()
                },
            );
            returned.send(result).unwrap();
        });
        waiting_rx.recv_timeout(WATCHDOG).unwrap();
        running.store(false, atomic::Ordering::Release);
        assert!(!returned_rx.recv_timeout(WATCHDOG).unwrap());
        thread.join().unwrap();
    }

    #[test]
    fn expired_metrics_deadline_still_services_due_about_once_and_rechecks_stop() {
        let (ctx, _requests, events) = context();
        let start = Instant::now();
        let mut about = AboutDelivery::default();
        about.request(start);
        let mut fetches = 0;
        assert!(ctx.serve_magpie_until(
            start,
            &mut about,
            || {
                fetches += 1;
                Some(About {
                    ready: Some(false),
                    ..Default::default()
                })
            },
            |_| panic!("expired metrics deadline must not receive actions"),
            || start,
        ));
        assert_eq!(fetches, 1);
        assert_eq!(about.deadline(), Some(start + ABOUT_RETRY));
        assert!(events.try_recv().is_err());
        assert_eq!(ctx.outbox.readings.published(), 0);

        assert!(!ctx.serve_magpie_until(
            start,
            &mut about,
            || {
                ctx.running.store(false, atomic::Ordering::Release);
                None
            },
            |_| panic!("stopped wait must not receive actions"),
            || start + ABOUT_RETRY,
        ));
    }
}

fn replay_fixtures(ctx: ThreadCtx, readings: Vec<Readings>, icons: HashMap<String, Icon>) {
    if !icons.is_empty() {
        let _ = ctx.outbox.send(Event::IconsReady(icons));
    }

    if readings.is_empty() {
        g_warning!("TaskManager::Gatherer", "Fixture contains no readings");
        return;
    }

    let mut index = 0;
    while ctx.running() {
        let cycle_start = Instant::now();
        let reduced = ctx.reduced_activity.load(atomic::Ordering::Acquire);

        let mut snapshot = readings[index % readings.len()].clone();
        if reduced {
            snapshot.running_processes.clear();
            snapshot.running_apps.clear();
            snapshot.user_services.clear();
            snapshot.system_services.clear();
            snapshot.includes_app_data = false;
        }
        ctx.outbox.publish(snapshot);
        index += 1;

        let deadline = cycle_start + ctx.interval();
        let outbox = &ctx.outbox;
        if !ctx.serve_until(deadline, |request| {
            g_debug!(
                "TaskManager::Gatherer",
                "Fixture source ignoring {request:?}"
            );
            let _ = match request {
                Request::ServiceLogs(service_id, _) => outbox.send(Event::ServiceLogs {
                    service_id,
                    logs: String::new(),
                }),
                Request::EjectDisk(disk_id) => outbox.send(Event::EjectResult {
                    disk_id,
                    result: Ok(()),
                }),
                Request::SmartData(disk_id) => outbox.send(Event::SmartData {
                    disk_id,
                    data: None,
                }),
                Request::AboutSystem => outbox.send(Event::About(About::default())),
                // Hardware admission/completion is independent of gatherer request queues.
                Request::ReadSpd { .. }
                | Request::SetHardwareAccess { .. }
                | Request::InstallHardwareService { .. }
                | Request::CancelHardware { .. } => Ok(()),
                _ => Ok(()),
            };
        }) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_snapshots_and_refresh_continue_during_held_authorization() {
        let speed = Arc::new(AtomicU64::new(1));
        let reduced_activity = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let readings = Arc::new(crate::LatestSlot::new());
        let (requests, receiver) = mpsc::sync_channel(REQUEST_CAPACITY);
        let (events, event_receiver) = mpsc::sync_channel(EVENT_CAPACITY);
        let hardware = crate::hardware_worker::Handle::pending_authorization_for_test(
            running.clone(),
            speed.clone(),
            events.clone(),
            Arc::new(|| {}),
        );
        let ctx = ThreadCtx {
            requests: receiver,
            outbox: Outbox {
                readings: readings.clone(),
                events,
                waker: Arc::new(|| {}),
            },
            running: running.clone(),
            speed: speed.clone(),
            reduced_activity: reduced_activity.clone(),
            hardware_control: hardware.control.clone(),
            hardware_samples: hardware.samples.clone(),
        };
        let thread = std::thread::spawn(move || {
            replay_fixtures(ctx, crate::synthetic_series(2, 1), HashMap::new())
        });
        let client = MagpieClient {
            speed,
            reduced_activity,
            running,
            thread: Some(thread),
            requests,
            events: event_receiver,
            readings,
            hardware,
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while client.readings_stats().0 < 2 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            client
                .hardware
                .control
                .active
                .load(atomic::Ordering::Acquire),
            1
        );
        let before = client.readings_stats().0;
        assert!(client.send(Request::RefreshNow));
        while client.readings_stats().0 == before {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(client.send(Request::CancelHardware { id: 1 }));
        loop {
            if let Some(Event::HardwareAccess {
                id: Some(1),
                result: Err(_),
                ..
            }) = client.try_event()
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        drop(client);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    use std::sync::atomic::AtomicUsize;

    fn counting_waker() -> (Waker, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        (
            Arc::new(move || {
                c.fetch_add(1, atomic::Ordering::Relaxed);
            }),
            count,
        )
    }

    fn reading(total: f32) -> Readings {
        let mut r = Readings::new();
        r.cpu.total_usage_percent = total;
        r.running_processes.insert(
            1,
            Process {
                name: "init".into(),
                pid: 1,
                ..Default::default()
            },
        );
        r
    }

    fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn fixture_source_publishes_and_wakes() {
        let (waker, wakes) = counting_waker();
        let client =
            MagpieClient::from_fixtures(vec![reading(10.), reading(20.)], HashMap::new(), waker);
        client.set_update_speed(1);

        let first = wait_for(|| client.take_readings());
        assert!(first.cpu.total_usage_percent == 10. || first.cpu.total_usage_percent == 20.);
        assert!(wakes.load(atomic::Ordering::Relaxed) >= 1);
    }

    #[test]
    fn refresh_now_ends_the_wait() {
        let (waker, _) = counting_waker();
        let client =
            MagpieClient::from_fixtures(vec![reading(10.), reading(20.)], HashMap::new(), waker);
        // 10 s interval: without RefreshNow the second snapshot would not arrive in time
        client.set_update_speed(200);
        let first = wait_for(|| client.take_readings());
        let started = Instant::now();
        client.send(Request::RefreshNow);
        let second = wait_for(|| client.take_readings());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_ne!(
            first.cpu.total_usage_percent,
            second.cpu.total_usage_percent
        );
    }

    #[test]
    fn reduced_activity_strips_app_data() {
        let (waker, _) = counting_waker();
        let client = MagpieClient::from_fixtures(vec![reading(10.)], HashMap::new(), waker);
        client.set_update_speed(1);
        client.set_reduced_activity(true);

        let r = wait_for(|| client.take_readings().filter(|r| !r.includes_app_data));
        assert!(r.running_processes.is_empty());
    }

    #[test]
    fn queries_are_answered_with_events() {
        let (waker, _) = counting_waker();
        let client = MagpieClient::from_fixtures(vec![reading(10.)], HashMap::new(), waker);
        client.set_update_speed(1);
        assert!(client.send(Request::ServiceLogs(42, None)));

        let event = wait_for(|| client.try_event());
        match event {
            Event::ServiceLogs { service_id, .. } => assert_eq!(service_id, 42),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn slow_consumer_does_not_grow_memory() {
        let (waker, _) = counting_waker();
        let client = MagpieClient::from_fixtures(vec![reading(10.)], HashMap::new(), waker);
        client.set_update_speed(1);

        wait_for(|| {
            let (published, _) = client.readings_stats();
            (published >= 5).then_some(())
        });
        let (published, dropped) = client.readings_stats();
        assert!(dropped >= published - 1);
        assert!(client.take_readings().is_some());
        drop(client);
    }

    #[test]
    fn readings_roundtrip_through_json() {
        let r = reading(42.);
        let json = serde_json::to_string(&r).unwrap();
        let back: Readings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cpu.total_usage_percent, 42.);
        assert_eq!(back.running_processes[&1].name, "init");
    }
}
