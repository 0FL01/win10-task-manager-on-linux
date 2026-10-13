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

    /// Returns the event back if the channel is full.
    #[allow(clippy::result_large_err)]
    fn send(&self, event: Event) -> Result<(), Event> {
        let result = match self.events.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(event)) => {
                if profile::enabled() {
                    profile::count(Counter::EventFull);
                } else {
                    g_warning!(
                        "TaskManager::Gatherer",
                        "UI is not draining events, dropping {:?}",
                        std::mem::discriminant(&event)
                    );
                }
                Err(event)
            }
            Err(TrySendError::Disconnected(event)) => Err(event),
        };
        (self.waker)();
        result
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
}

fn handle_request(magpie: &Client, outbox: &Outbox, request: Request) {
    let _profile = Span::new(Phase::Action);
    match request {
        // Handled by `serve_until`
        Request::RefreshNow => {}
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
        Request::AboutSystem => {
            let _ = outbox.send(Event::About(magpie.about()));
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
    drop(startup);

    while ctx.running() {
        let cycle_start = Instant::now();
        let mut cycle = Span::new(Phase::GatherCycle);
        let reduced = ctx.reduced_activity.load(atomic::Ordering::Acquire);

        let mut readings = Readings::new();
        readings.mem_devices = mem_devices.clone();
        readings.includes_app_data = !reduced;

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
        readings.disks_info = disks.disks;
        readings.smart_data = disks.smart_data;
        readings.gpus = timed!("GPU info", Gpus, magpie.gpus());
        readings.cpu = timed!("CPU info", Cpu, magpie.cpu());
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
                known_icons.extend(missing);

                if !icons.is_empty() {
                    if let Err(Event::IconsReady(icons)) = ctx.outbox.send(Event::IconsReady(icons))
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
        ctx.outbox.publish(readings);
        drop(cycle);

        g_debug!(
            "TaskManager::Perf",
            "Read cycle took {:?}",
            cycle_start.elapsed()
        );

        let deadline = cycle_start + ctx.interval();
        if Instant::now() > deadline {
            profile::count(Counter::Overrun);
        }
        if !ctx.serve_until(deadline, |request| {
            handle_request(&magpie, &ctx.outbox, request)
        }) {
            break;
        }
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
