/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Optional NVML supplementation, including devices absent from NVTOP's fixed
//! startup inventory. Every sample resolves its handle by PCI identity afresh.
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;

use magpie_platform::gpus::{
    ClientTelemetry, EngineKind, Process, ProcessIdentity, ProcessKind, ProcessObservation,
    TelemetryState,
};

type Status = u32;
type Handle = *mut c_void;
type Init = unsafe extern "C" fn() -> Status;
type Shutdown = unsafe extern "C" fn() -> Status;
type GetHandle = unsafe extern "C" fn(*const c_char, *mut Handle) -> Status;
type GetVersion = unsafe extern "C" fn(*mut c_char, u32) -> Status;
type GetMemoryV1 = unsafe extern "C" fn(Handle, *mut MemoryV1) -> Status;
type GetMemoryV2 = unsafe extern "C" fn(Handle, *mut MemoryV2) -> Status;
type GetName = unsafe extern "C" fn(Handle, *mut c_char, u32) -> Status;
type GetUtilization = unsafe extern "C" fn(Handle, *mut Utilization) -> Status;
type GetVideoUtilization = unsafe extern "C" fn(Handle, *mut u32, *mut u32) -> Status;
type GetUnsigned = unsafe extern "C" fn(Handle, *mut u32) -> Status;
type GetUnsignedParameter = unsafe extern "C" fn(Handle, u32, *mut u32) -> Status;
type GetProcessesV1 = unsafe extern "C" fn(Handle, *mut u32, *mut ProcessV1) -> Status;
type GetProcessesV2 = unsafe extern "C" fn(Handle, *mut u32, *mut ProcessV2) -> Status;
type GetProcessUtilization =
    unsafe extern "C" fn(Handle, *mut ProcessUtilization, *mut u32, u64) -> Status;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: u32,
    memory: u32,
}
#[repr(C)]
#[derive(Clone, Default)]
struct ProcessV1 {
    pid: u32,
    used_memory: u64,
}
#[repr(C)]
#[derive(Clone, Default)]
struct ProcessV2 {
    pid: u32,
    used_memory: u64,
    gpu_instance: u32,
    compute_instance: u32,
}
#[repr(C)]
#[derive(Clone, Default)]
struct ProcessUtilization {
    pid: u32,
    timestamp: u64,
    sm: u32,
    memory: u32,
    encode: u32,
    decode: u32,
}

#[repr(C)]
#[derive(Default)]
struct MemoryV1 {
    total: u64,
    free: u64,
    used: u64,
}
#[repr(C)]
#[derive(Default)]
struct MemoryV2 {
    version: u32,
    total: u64,
    reserved: u64,
    free: u64,
    used: u64,
}

fn memory_v2_version() -> u32 {
    std::mem::size_of::<MemoryV2>() as u32 | (2 << 24)
}

#[derive(Debug)]
pub(super) struct Memory {
    pub total: u64,
    pub used: u64,
    pub reserved: Option<u64>,
}

pub(super) struct Nvml {
    _library: libloading::Library,
    shutdown: Shutdown,
    api: Api,
}

#[derive(Default)]
struct Api {
    handle: Option<GetHandle>,
    version: Option<GetVersion>,
    memory_v1: Option<GetMemoryV1>,
    memory_v2: Option<GetMemoryV2>,
    name: Option<GetName>,
    utilization: Option<GetUtilization>,
    encode: Option<GetVideoUtilization>,
    decode: Option<GetVideoUtilization>,
    temperature: Option<GetUnsignedParameter>,
    power: Option<GetUnsigned>,
    power_limit: Option<GetUnsigned>,
    clock: Option<GetUnsignedParameter>,
    max_clock: Option<GetUnsignedParameter>,
    fan: Option<GetUnsigned>,
    pcie_gen: Option<GetUnsigned>,
    pcie_lanes: Option<GetUnsigned>,
    max_pcie_gen: Option<GetUnsigned>,
    max_pcie_lanes: Option<GetUnsigned>,
    // v2 has the stable, documented MIG IDs; do not alias incompatible structs.
    processes_v2: [Option<GetProcessesV2>; 3],
    processes_v1: [Option<GetProcessesV1>; 3],
    process_utilization: Option<GetProcessUtilization>,
}

pub(super) struct Sample {
    pub name: Option<String>,
    pub overall: Result<f32, TelemetryState>,
    pub encode: Result<f32, TelemetryState>,
    pub decode: Result<f32, TelemetryState>,
    pub memory: Result<Memory, TelemetryState>,
    pub sensors: Sensors,
    pub processes: Processes,
}

#[derive(Default)]
pub(super) struct Sensors {
    pub temperature_c: Option<f32>,
    pub power_draw_watts: Option<f32>,
    pub max_power_draw_watts: Option<f32>,
    pub clock_speed_mhz: Option<u32>,
    pub memory_speed_mhz: Option<u32>,
    pub max_clock_speed_mhz: Option<u32>,
    pub max_memory_speed_mhz: Option<u32>,
    pub fan_speed_percent: Option<f32>,
    pub pcie_gen: Option<u32>,
    pub pcie_lanes: Option<u32>,
    pub max_pcie_gen: Option<u32>,
    pub max_pcie_lanes: Option<u32>,
}

pub(super) struct Processes {
    entries: HashMap<u32, Process>,
    samples: HashMap<u32, ProcessUtilization>,
    memory_partial: std::collections::HashSet<u32>,
    list_state: TelemetryState,
    usage_state: TelemetryState,
    timestamp: u64,
}

/// No device handles or old measurements live here. The caller removes this
/// cursor on inventory removal/replacement; PID start times gate new samples.
#[derive(Default)]
pub(super) struct ProcessCursor {
    pub timestamp: u64,
    owners: HashMap<u32, u64>,
}

pub(super) struct Clients {
    pub processes: HashMap<u32, Process>,
    pub clients: Vec<ClientTelemetry>,
    pub observations: Vec<ProcessObservation>,
}

impl ProcessCursor {
    pub fn apply(
        &mut self,
        source: &Processes,
        identities: &HashMap<u32, u64>,
        proc_root: &Path,
    ) -> Clients {
        let mut result = Clients {
            processes: HashMap::new(),
            clients: vec![],
            observations: vec![],
        };
        let mut owners = HashMap::new();
        for (&pid, process) in &source.entries {
            let identity = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat"))
                .ok()
                .and_then(|text| super::fdinfo::starttime_from_stat(&text));
            let Some(start) = identity.filter(|start| identities.get(&pid) == Some(start)) else {
                result.observations.push(ProcessObservation {
                    pid,
                    start_time_ticks: identity,
                    state: TelemetryState::Stale as i32,
                });
                continue;
            };
            owners.insert(pid, start);
            // Samples from before we observed this PID identity must not be
            // attributed to a newly started/recycled process with the same PID.
            let known_owner = self.owners.get(&pid) == Some(&start);
            let usage = source.samples.get(&pid).filter(|_| known_owner);
            let usage_state = if usage.is_some() {
                TelemetryState::Partial // SM total is not a 3D/compute breakdown.
            } else if source.usage_state == TelemetryState::Available {
                TelemetryState::Warmup // No new sample is not proof of zero usage.
            } else {
                source.usage_state
            };
            let mut process = *process;
            process.gpu_usage_percent = usage.map(|sample| sample.sm as f32);
            process.encode_usage_percent = usage.map(|sample| sample.encode as f32);
            process.decode_usage_percent = usage.map(|sample| sample.decode as f32);
            let engines = usage
                .map(|sample| {
                    vec![
                        super::native_engine(
                            "encode",
                            EngineKind::Encode,
                            Some(sample.encode as f32),
                        ),
                        super::native_engine(
                            "decode",
                            EngineKind::Decode,
                            Some(sample.decode as f32),
                        ),
                    ]
                })
                .unwrap_or_default();
            result.clients.push(ClientTelemetry {
                id: format!("nvml:{pid}:{start}"),
                owners: vec![ProcessIdentity {
                    pid,
                    start_time_ticks: start,
                }],
                engines,
                overall_usage_percent: process.gpu_usage_percent,
                dedicated_memory_bytes: process.memory_usage_bytes,
                usage_state: usage_state as i32,
                memory_state: if process.memory_usage_bytes.is_none() {
                    TelemetryState::Unsupported
                } else if source.memory_partial.contains(&pid)
                    || source.list_state != TelemetryState::Available
                {
                    TelemetryState::Partial
                } else {
                    TelemetryState::Available
                } as i32,
                memory_is_resident: process.memory_usage_bytes.map(|_| true),
                ..Default::default()
            });
            result.observations.push(ProcessObservation {
                pid,
                start_time_ticks: Some(start),
                state: usage_state as i32,
            });
            result.processes.insert(pid, process);
        }
        if source.list_state == TelemetryState::Available {
            for (&pid, &start) in identities {
                if source.entries.contains_key(&pid) {
                    continue;
                }
                let after = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat"));
                let state = match &after {
                    Ok(text) if super::fdinfo::starttime_from_stat(text) == Some(start) => {
                        TelemetryState::Available // Complete list: observed, known idle.
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                        TelemetryState::PermissionDenied
                    }
                    _ => TelemetryState::Stale,
                };
                result.observations.push(ProcessObservation {
                    pid,
                    start_time_ticks: Some(start),
                    state: state as i32,
                });
            }
        }
        self.owners = owners;
        self.timestamp = self.timestamp.max(source.timestamp);
        result.clients.sort_by(|a, b| a.id.cmp(&b.id));
        result
            .observations
            .sort_by_key(|observation| observation.pid);
        result
    }
}

fn state(status: Status) -> TelemetryState {
    match status {
        3 => TelemetryState::Unsupported,
        4 => TelemetryState::PermissionDenied,
        1 | 6 | 9 | 15 => TelemetryState::DriverUnavailable,
        _ => TelemetryState::Stale,
    }
}

impl Nvml {
    pub fn new() -> Option<Self> {
        let _profile =
            magpie_platform::profile::Span::new(magpie_platform::profile::Phase::GpuNvmlInit);
        // NVML initialization is reference counted. Our reference is separate
        // from NVTOP's, and the library outlives every copied symbol.
        unsafe {
            let library = libloading::Library::new("libnvidia-ml.so.1").ok()?;
            let init = library
                .get::<Init>(b"nvmlInit_v2\0")
                .or_else(|_| library.get::<Init>(b"nvmlInit\0"))
                .ok()?;
            let shutdown = *library.get::<Shutdown>(b"nvmlShutdown\0").ok()?;
            if init() != 0 {
                return None;
            }
            let api = Api {
                handle: library
                    .get::<GetHandle>(b"nvmlDeviceGetHandleByPciBusId_v2\0")
                    .or_else(|_| library.get::<GetHandle>(b"nvmlDeviceGetHandleByPciBusId\0"))
                    .ok()
                    .map(|s| *s),
                version: library
                    .get::<GetVersion>(b"nvmlSystemGetDriverVersion\0")
                    .ok()
                    .map(|s| *s),
                memory_v1: library
                    .get::<GetMemoryV1>(b"nvmlDeviceGetMemoryInfo\0")
                    .ok()
                    .map(|s| *s),
                memory_v2: library
                    .get::<GetMemoryV2>(b"nvmlDeviceGetMemoryInfo_v2\0")
                    .ok()
                    .map(|s| *s),
                name: library
                    .get::<GetName>(b"nvmlDeviceGetName\0")
                    .ok()
                    .map(|s| *s),
                utilization: library
                    .get::<GetUtilization>(b"nvmlDeviceGetUtilizationRates\0")
                    .ok()
                    .map(|s| *s),
                encode: library
                    .get::<GetVideoUtilization>(b"nvmlDeviceGetEncoderUtilization\0")
                    .ok()
                    .map(|s| *s),
                decode: library
                    .get::<GetVideoUtilization>(b"nvmlDeviceGetDecoderUtilization\0")
                    .ok()
                    .map(|s| *s),
                temperature: library
                    .get::<GetUnsignedParameter>(b"nvmlDeviceGetTemperature\0")
                    .ok()
                    .map(|s| *s),
                power: library
                    .get::<GetUnsigned>(b"nvmlDeviceGetPowerUsage\0")
                    .ok()
                    .map(|s| *s),
                power_limit: library
                    .get::<GetUnsigned>(b"nvmlDeviceGetPowerManagementLimit\0")
                    .ok()
                    .map(|s| *s),
                clock: library
                    .get::<GetUnsignedParameter>(b"nvmlDeviceGetClockInfo\0")
                    .ok()
                    .map(|s| *s),
                max_clock: library
                    .get::<GetUnsignedParameter>(b"nvmlDeviceGetMaxClockInfo\0")
                    .ok()
                    .map(|s| *s),
                fan: library
                    .get::<GetUnsigned>(b"nvmlDeviceGetFanSpeed\0")
                    .ok()
                    .map(|s| *s),
                pcie_gen: library
                    .get::<GetUnsigned>(b"nvmlDeviceGetCurrPcieLinkGeneration\0")
                    .ok()
                    .map(|s| *s),
                pcie_lanes: library
                    .get::<GetUnsigned>(b"nvmlDeviceGetCurrPcieLinkWidth\0")
                    .ok()
                    .map(|s| *s),
                max_pcie_gen: library
                    .get::<GetUnsigned>(b"nvmlDeviceGetMaxPcieLinkGeneration\0")
                    .ok()
                    .map(|s| *s),
                max_pcie_lanes: library
                    .get::<GetUnsigned>(b"nvmlDeviceGetMaxPcieLinkWidth\0")
                    .ok()
                    .map(|s| *s),
                processes_v2: [
                    b"nvmlDeviceGetGraphicsRunningProcesses_v2\0".as_slice(),
                    b"nvmlDeviceGetComputeRunningProcesses_v2\0".as_slice(),
                    b"nvmlDeviceGetMPSComputeRunningProcesses_v2\0".as_slice(),
                ]
                .map(|name| library.get::<GetProcessesV2>(name).ok().map(|s| *s)),
                processes_v1: [
                    b"nvmlDeviceGetGraphicsRunningProcesses\0".as_slice(),
                    b"nvmlDeviceGetComputeRunningProcesses\0".as_slice(),
                    b"nvmlDeviceGetMPSComputeRunningProcesses\0".as_slice(),
                ]
                .map(|name| library.get::<GetProcessesV1>(name).ok().map(|s| *s)),
                process_utilization: library
                    .get::<GetProcessUtilization>(b"nvmlDeviceGetProcessUtilization\0")
                    .ok()
                    .map(|s| *s),
            };
            Some(Self {
                _library: library,
                shutdown,
                api,
            })
        }
    }

    pub fn driver_version(&self) -> Option<String> {
        self.api.driver_version()
    }

    pub fn sample(&self, pci: &str, timestamp: u64) -> Result<Sample, TelemetryState> {
        let mut profile =
            magpie_platform::profile::Span::new(magpie_platform::profile::Phase::GpuNvml);
        profile.items(1);
        self.api.sample(pci, timestamp)
    }
}

impl Api {
    fn driver_version(&self) -> Option<String> {
        let mut buffer = [0 as c_char; 128];
        let result = unsafe { (self.version?)(buffer.as_mut_ptr(), buffer.len() as u32) };
        if result != 0 {
            return None;
        }
        buffer[127] = 0;
        let version = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().ok()?;
        (!version.is_empty()).then(|| version.to_owned())
    }

    fn sample(&self, pci: &str, timestamp: u64) -> Result<Sample, TelemetryState> {
        let pci = CString::new(pci).map_err(|_| TelemetryState::Unsupported)?;
        let mut handle = std::ptr::null_mut();
        let result =
            unsafe { (self.handle.ok_or(TelemetryState::Unsupported)?)(pci.as_ptr(), &mut handle) };
        if result != 0 {
            return Err(state(result));
        }
        if handle.is_null() {
            return Err(TelemetryState::DriverUnavailable);
        }
        let sensors = self.sensors(handle)?;
        let mut buffer = [0 as c_char; 128];
        let name = self.name.and_then(|query| {
            if unsafe { query(handle, buffer.as_mut_ptr(), buffer.len() as u32) } != 0 {
                return None;
            }
            buffer[127] = 0;
            let text = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().ok()?;
            (!text.is_empty()).then(|| text.to_owned())
        });
        let overall = self
            .utilization
            .ok_or(TelemetryState::Unsupported)
            .and_then(|query| {
                let mut value = Utilization::default();
                let status = unsafe { query(handle, &mut value) };
                percent(status, value.gpu)
            });
        let video = |query: Option<GetVideoUtilization>| {
            let query = query.ok_or(TelemetryState::Unsupported)?;
            let (mut value, mut period) = (0, 0);
            let status = unsafe { query(handle, &mut value, &mut period) };
            percent(status, value)
        };
        Ok(Sample {
            name,
            overall,
            encode: video(self.encode),
            decode: video(self.decode),
            memory: self.memory(handle),
            sensors,
            processes: self.processes(handle, timestamp),
        })
    }

    fn sensors(&self, handle: Handle) -> Result<Sensors, TelemetryState> {
        let scalar = |query: Option<GetUnsigned>| {
            optional_sensor(|out| query.map(|query| unsafe { query(handle, out) }))
        };
        let parameter = |query: Option<GetUnsignedParameter>, kind| {
            optional_sensor(|out| query.map(|query| unsafe { query(handle, kind, out) }))
        };
        // NVML_TEMPERATURE_GPU = 0, NVML_CLOCK_GRAPHICS = 0, NVML_CLOCK_MEM = 2.
        // Report the graphics clock itself, not an inferred 3D/compute clock.
        Ok(Sensors {
            temperature_c: parameter(self.temperature, 0)?.map(|value| value as f32),
            power_draw_watts: scalar(self.power)?.map(|value| value as f32 / 1000.),
            max_power_draw_watts: scalar(self.power_limit)?.map(|value| value as f32 / 1000.),
            clock_speed_mhz: parameter(self.clock, 0)?,
            memory_speed_mhz: parameter(self.clock, 2)?,
            max_clock_speed_mhz: parameter(self.max_clock, 0)?,
            max_memory_speed_mhz: parameter(self.max_clock, 2)?,
            fan_speed_percent: scalar(self.fan)?.map(|value| value as f32),
            pcie_gen: scalar(self.pcie_gen)?,
            pcie_lanes: scalar(self.pcie_lanes)?,
            max_pcie_gen: scalar(self.max_pcie_gen)?,
            max_pcie_lanes: scalar(self.max_pcie_lanes)?,
        })
    }

    fn memory(&self, handle: Handle) -> Result<Memory, TelemetryState> {
        if let Some(query) = self.memory_v2 {
            let mut value = MemoryV2 {
                version: memory_v2_version(),
                ..Default::default()
            };
            let status = unsafe { query(handle, &mut value) };
            if status == 0 {
                return validate(value.total, value.used, value.free, Some(value.reserved));
            }
            // Only unsupported/version mismatch permits the older query.
            if !matches!(status, 3 | 25) {
                return Err(state(status));
            }
        }
        let query = self.memory_v1.ok_or(TelemetryState::Unsupported)?;
        let mut value = MemoryV1::default();
        let status = unsafe { query(handle, &mut value) };
        if status != 0 {
            return Err(state(status));
        }
        validate(value.total, value.used, value.free, None)
    }

    fn process_list(&self, handle: Handle, index: usize) -> Result<Vec<ProcessV1>, TelemetryState> {
        if let Some(query) = self.processes_v2[index] {
            let result = query_array(|count, entries| unsafe { query(handle, count, entries) });
            match result {
                Ok(entries) => {
                    return Ok(entries
                        .into_iter()
                        .map(|entry| ProcessV1 {
                            pid: entry.pid,
                            used_memory: entry.used_memory,
                        })
                        .collect())
                }
                Err(TelemetryState::Unsupported) => {}
                Err(error) => return Err(error),
            }
        }
        let query = self.processes_v1[index].ok_or(TelemetryState::Unsupported)?;
        query_array(|count, entries| unsafe { query(handle, count, entries) })
    }

    fn processes(&self, handle: Handle, timestamp: u64) -> Processes {
        let mut result = Processes {
            entries: HashMap::new(),
            samples: HashMap::new(),
            memory_partial: Default::default(),
            list_state: TelemetryState::Unsupported,
            usage_state: TelemetryState::Unsupported,
            timestamp,
        };
        let mut successes = 0;
        let mut errors = vec![];
        for index in 0..3 {
            // MPS is optional; absent symbols do not invalidate an otherwise
            // complete graphics/compute scan. An actual failed query does.
            if index == 2
                && self.processes_v1[index].is_none()
                && self.processes_v2[index].is_none()
            {
                continue;
            }
            match self.process_list(handle, index) {
                Ok(entries) => {
                    successes += 1;
                    for entry in entries {
                        if entry.pid == 0 {
                            continue;
                        }
                        let kind = if index == 0 {
                            ProcessKind::Graphical
                        } else {
                            ProcessKind::Compute
                        };
                        let memory = (entry.used_memory != u64::MAX).then_some(entry.used_memory);
                        let process = result.entries.entry(entry.pid).or_insert(Process {
                            kind: Some(kind as i32),
                            memory_usage_bytes: memory,
                            ..Default::default()
                        });
                        if process.kind != Some(kind as i32) {
                            process.kind = Some(ProcessKind::GraphicalCompute as i32);
                        }
                        if process.memory_usage_bytes != memory {
                            result.memory_partial.insert(entry.pid);
                        }
                        // Graphics/compute/MPS lists can repeat the same client.
                        // Never sum those buffers or fabricate unavailable bytes.
                        process.memory_usage_bytes = process.memory_usage_bytes.max(memory);
                    }
                }
                Err(error) => errors.push(error),
            }
        }
        result.list_state = if successes > 0 {
            if errors.is_empty() {
                TelemetryState::Available
            } else {
                TelemetryState::Partial
            }
        } else {
            errors
                .iter()
                .copied()
                .find(|state| *state != TelemetryState::Unsupported)
                .unwrap_or(TelemetryState::Unsupported)
        };
        if let Some(query) = self.process_utilization {
            let samples = query_array(|count, entries| {
                let status = unsafe { query(handle, entries, count, timestamp) };
                // Only NOT_FOUND means no buffered samples, not GPU_LOST or
                // DRIVER_NOT_LOADED (which must remain failures).
                if status == 6 {
                    unsafe {
                        *count = 0;
                    }
                    0
                } else {
                    status
                }
            });
            match samples {
                Ok(samples) => {
                    result.usage_state = TelemetryState::Available;
                    for sample in samples {
                        if sample.timestamp <= timestamp
                            || sample.sm > 100
                            || sample.encode > 100
                            || sample.decode > 100
                        {
                            continue;
                        }
                        result.timestamp = result.timestamp.max(sample.timestamp);
                        if !result.entries.contains_key(&sample.pid) {
                            continue;
                        }
                        let previous = result
                            .samples
                            .entry(sample.pid)
                            .or_insert_with(|| sample.clone());
                        if sample.timestamp > previous.timestamp {
                            *previous = sample;
                        }
                    }
                }
                Err(error) => result.usage_state = error,
            }
        }
        result
    }
}

fn percent(status: Status, value: u32) -> Result<f32, TelemetryState> {
    if status != 0 {
        return Err(state(status));
    }
    if value > 100 {
        return Err(TelemetryState::Stale);
    }
    Ok(value as f32)
}

fn optional_sensor(
    query: impl FnOnce(*mut u32) -> Option<Status>,
) -> Result<Option<u32>, TelemetryState> {
    let mut value = 0;
    match query(&mut value) {
        Some(0) => Ok(Some(value)),
        // Loss during any query invalidates the entire snapshot, including
        // native readings collected before the freshly resolved handle failed.
        Some(1 | 6 | 9 | 15) => Err(TelemetryState::DriverUnavailable),
        // The legacy sensor fields have no per-field state. Missing symbols,
        // permission failures and unsupported/error readings stay absent.
        _ => Ok(None),
    }
}

/// Bounded retries and allocations even if the driver's inventory changes while
/// it fills the array. A failed query is never converted into an empty/idle list.
fn query_array<T: Default + Clone>(
    mut query: impl FnMut(*mut u32, *mut T) -> Status,
) -> Result<Vec<T>, TelemetryState> {
    const MAX_RECORDS: u32 = 65_536;
    let mut count = 0;
    let status = query(&mut count, std::ptr::null_mut());
    if status == 0 && count == 0 {
        return Ok(vec![]);
    }
    if status != 7 {
        return Err(if status == 0 {
            TelemetryState::Stale
        } else {
            state(status)
        });
    }
    for _ in 0..3 {
        if count == 0 || count > MAX_RECORDS {
            return Err(TelemetryState::Stale);
        }
        let mut entries = vec![T::default(); count as usize];
        let status = query(&mut count, entries.as_mut_ptr());
        if status == 0 && count as usize <= entries.len() {
            entries.truncate(count as usize);
            return Ok(entries);
        }
        if status != 7 {
            return Err(if status == 0 {
                TelemetryState::Stale
            } else {
                state(status)
            });
        }
    }
    Err(TelemetryState::Stale)
}

fn validate(
    total: u64,
    used: u64,
    free: u64,
    reserved: Option<u64>,
) -> Result<Memory, TelemetryState> {
    // A zero framebuffer on UMA is not a license to substitute host MemTotal.
    if total == 0 {
        return Err(TelemetryState::Unsupported);
    }
    if used > total || free > total || reserved.is_some_and(|value| value > total) {
        return Err(TelemetryState::Stale);
    }
    Ok(Memory {
        total,
        used,
        reserved,
    })
}

impl Drop for Nvml {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Mock {
        present: bool,
        generation: usize,
        lookups: usize,
        queried_handles: Vec<usize>,
        utilization_status: Status,
        memory_status: Status,
        sample_status: Status,
        sensor_statuses: [Status; 12],
        sensor_calls: Vec<(usize, usize)>,
        samples: Vec<ProcessUtilization>,
        entries: Vec<ProcessV2>,
    }
    thread_local! {
        static MOCK: RefCell<Mock> = RefCell::new(Mock::default());
    }
    unsafe extern "C" fn handle(pci: *const c_char, out: *mut Handle) -> Status {
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.lookups += 1;
            if CStr::from_ptr(pci).to_bytes() != b"0000:01:00.0" {
                return 6;
            }
            if !mock.present {
                return 15;
            }
            *out = mock.generation as Handle;
            0
        })
    }
    unsafe extern "C" fn utilization(handle: Handle, out: *mut Utilization) -> Status {
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.queried_handles.push(handle as usize);
            (*out).gpu = handle as u32 * 10;
            mock.utilization_status
        })
    }
    unsafe extern "C" fn memory(_handle: Handle, out: *mut MemoryV2) -> Status {
        MOCK.with(|mock| {
            if (*out).version != memory_v2_version() {
                return 25;
            }
            if mock.borrow().memory_status != 0 {
                return mock.borrow().memory_status;
            }
            (*out).total = 100;
            (*out).free = 60;
            (*out).used = 40;
            (*out).reserved = 10;
            0
        })
    }
    unsafe extern "C" fn video(_handle: Handle, out: *mut u32, period: *mut u32) -> Status {
        *out = 0;
        *period = 1_000_000;
        0
    }
    unsafe fn sensor(handle: Handle, out: *mut u32, key: usize, value: u32) -> Status {
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.sensor_calls.push((handle as usize, key));
            // Even an error may have touched its output; never accept that value.
            *out = value;
            mock.sensor_statuses[key]
        })
    }
    unsafe extern "C" fn temperature(handle: Handle, kind: u32, out: *mut u32) -> Status {
        if kind != 0 {
            return 2;
        }
        sensor(handle, out, 0, 50 + handle as u32)
    }
    unsafe extern "C" fn power(handle: Handle, out: *mut u32) -> Status {
        sensor(handle, out, 1, 12_345 + handle as u32 * 1000)
    }
    unsafe extern "C" fn power_limit(handle: Handle, out: *mut u32) -> Status {
        sensor(handle, out, 2, 150_000)
    }
    unsafe extern "C" fn clock(handle: Handle, kind: u32, out: *mut u32) -> Status {
        match kind {
            0 => sensor(handle, out, 3, 500 + handle as u32),
            2 => sensor(handle, out, 4, 7000 + handle as u32),
            _ => 2,
        }
    }
    unsafe extern "C" fn max_clock(handle: Handle, kind: u32, out: *mut u32) -> Status {
        match kind {
            0 => sensor(handle, out, 5, 2000 + handle as u32),
            2 => sensor(handle, out, 6, 8000 + handle as u32),
            _ => 2,
        }
    }
    unsafe extern "C" fn fan(handle: Handle, out: *mut u32) -> Status {
        sensor(handle, out, 7, 0)
    }
    unsafe extern "C" fn pcie_gen(handle: Handle, out: *mut u32) -> Status {
        sensor(handle, out, 8, 3)
    }
    unsafe extern "C" fn pcie_lanes(handle: Handle, out: *mut u32) -> Status {
        sensor(handle, out, 9, 8)
    }
    unsafe extern "C" fn max_pcie_gen(handle: Handle, out: *mut u32) -> Status {
        sensor(handle, out, 10, 4)
    }
    unsafe extern "C" fn max_pcie_lanes(handle: Handle, out: *mut u32) -> Status {
        sensor(handle, out, 11, 16)
    }
    unsafe extern "C" fn list(_handle: Handle, count: *mut u32, out: *mut ProcessV2) -> Status {
        MOCK.with(|mock| write_array(&mock.borrow().entries, count, out))
    }
    unsafe extern "C" fn samples(
        _handle: Handle,
        out: *mut ProcessUtilization,
        count: *mut u32,
        timestamp: u64,
    ) -> Status {
        MOCK.with(|mock| {
            let mock = mock.borrow();
            if mock.sample_status != 0 {
                return mock.sample_status;
            }
            let samples = mock
                .samples
                .iter()
                .filter(|sample| sample.timestamp > timestamp)
                .cloned()
                .collect::<Vec<_>>();
            write_array(&samples, count, out)
        })
    }
    unsafe fn write_array<T: Clone>(values: &[T], count: *mut u32, out: *mut T) -> Status {
        if (*count as usize) < values.len() || (out.is_null() && !values.is_empty()) {
            *count = values.len() as u32;
            return 7;
        }
        *count = values.len() as u32;
        for (index, value) in values.iter().enumerate() {
            out.add(index).write(value.clone());
        }
        0
    }
    fn api() -> Api {
        Api {
            handle: Some(handle),
            utilization: Some(utilization),
            memory_v2: Some(memory),
            encode: Some(video),
            decode: Some(video),
            temperature: Some(temperature),
            power: Some(power),
            power_limit: Some(power_limit),
            clock: Some(clock),
            max_clock: Some(max_clock),
            fan: Some(fan),
            pcie_gen: Some(pcie_gen),
            pcie_lanes: Some(pcie_lanes),
            max_pcie_gen: Some(max_pcie_gen),
            max_pcie_lanes: Some(max_pcie_lanes),
            processes_v2: [Some(list), Some(list), None],
            process_utilization: Some(samples),
            ..Default::default()
        }
    }
    fn usage(timestamp: u64, sm: u32) -> ProcessUtilization {
        ProcessUtilization {
            pid: 11,
            timestamp,
            sm,
            encode: 0,
            decode: 10,
            ..Default::default()
        }
    }
    struct ProcFixture(std::path::PathBuf);
    impl ProcFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "magpie-nvml-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn process(&self, pid: u32, start: u64) {
            let path = self.0.join(pid.to_string());
            std::fs::create_dir_all(&path).unwrap();
            let mut fields = vec!["0".to_owned(); 20];
            fields[18] = start.to_string();
            std::fs::write(
                path.join("stat"),
                format!("{pid} (test (nested)) S {}", fields.join(" ")),
            )
            .unwrap();
        }
    }
    impl Drop for ProcFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn empty_startup_add_remove_reappear_resolves_fresh_pci_handles() {
        let api = api();
        assert!(matches!(
            api.sample("0000:01:00.0", 0),
            Err(TelemetryState::DriverUnavailable)
        ));
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.present = true;
            mock.generation = 1;
        });
        let first = api.sample("0000:01:00.0", 0).unwrap();
        assert_eq!(first.overall, Ok(10.));
        assert_eq!(first.encode, Ok(0.));
        assert_eq!(first.memory.unwrap().reserved, Some(10));
        let mut gpu = magpie_platform::gpus::Gpu::default();
        super::super::apply_nvml_sensors(&mut gpu, &first.sensors);
        assert_eq!(gpu.temperature_c, Some(51.));
        assert_eq!(gpu.power_draw_watts, Some(13.345));
        assert_eq!(gpu.max_power_draw_watts, Some(150.));
        assert_eq!(gpu.clock_speed_mhz, Some(501));
        assert_eq!(gpu.memory_speed_mhz, Some(7001));
        assert_eq!(gpu.max_clock_speed_mhz, Some(2001));
        assert_eq!(gpu.max_memory_speed_mhz, Some(8001));
        assert_eq!(gpu.fan_speed_percent, Some(0.)); // A real successful zero.
        assert_eq!((gpu.pcie_gen, gpu.pcie_lanes), (Some(3), Some(8)));
        assert_eq!((gpu.max_pcie_gen, gpu.max_pcie_lanes), (Some(4), Some(16)));
        MOCK.with(|mock| mock.borrow_mut().present = false);
        assert!(matches!(
            api.sample("0000:01:00.0", 0),
            Err(TelemetryState::DriverUnavailable)
        ));
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.present = true;
            mock.generation = 2;
        });
        let next = api.sample("0000:01:00.0", 0).unwrap();
        assert_eq!(next.overall, Ok(20.));
        super::super::clear_dynamic(&mut gpu);
        super::super::apply_nvml_sensors(&mut gpu, &next.sensors);
        assert_eq!(gpu.temperature_c, Some(52.));
        assert_eq!(gpu.power_draw_watts, Some(14.345));
        assert_eq!(gpu.clock_speed_mhz, Some(502));
        assert_eq!(gpu.memory_speed_mhz, Some(7002));
        MOCK.with(|mock| {
            assert_eq!(mock.borrow().lookups, 4);
            assert_eq!(mock.borrow().queried_handles, [1, 2]);
            assert_eq!(mock.borrow().sensor_calls.len(), 24);
            for (index, &(handle, key)) in mock.borrow().sensor_calls.iter().enumerate() {
                assert_eq!(handle, index / 12 + 1);
                assert_eq!(key, index % 12);
            }
        });
    }

    #[test]
    fn optional_sensor_errors_clear_old_values_and_loss_invalidates_snapshot() {
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.present = true;
            mock.generation = 1;
        });
        let api = api();
        let mut gpu = magpie_platform::gpus::Gpu::default();
        let first = api.sample("0000:01:00.0", 0).unwrap();
        super::super::apply_nvml_sensors(&mut gpu, &first.sensors);
        assert!(gpu.temperature_c.is_some());
        let missing = Api {
            handle: Some(handle),
            ..Default::default()
        }
        .sample("0000:01:00.0", 0)
        .unwrap();
        for status in [3, 4, 999] {
            MOCK.with(|mock| mock.borrow_mut().sensor_statuses = [status; 12]);
            let sample = api.sample("0000:01:00.0", 0).unwrap();
            super::super::clear_dynamic(&mut gpu);
            super::super::apply_nvml_sensors(&mut gpu, &sample.sensors);
            assert_eq!(gpu.temperature_c, None);
            assert_eq!(gpu.power_draw_watts, None);
            assert_eq!(gpu.clock_speed_mhz, None);
            assert_eq!(gpu.memory_speed_mhz, None);
            assert_eq!(gpu.max_power_draw_watts, None);
            assert_eq!(gpu.fan_speed_percent, None);
            assert_eq!(gpu.pcie_gen, None);
            assert_eq!(gpu.max_pcie_lanes, None);
        }
        super::super::apply_nvml_sensors(&mut gpu, &missing.sensors);
        assert_eq!(gpu.temperature_c, None);
        assert_eq!(gpu.clock_speed_mhz, None);
        for status in [1, 6, 9, 15] {
            MOCK.with(|mock| {
                let mut mock = mock.borrow_mut();
                mock.sensor_statuses = [0; 12];
                mock.sensor_statuses[1] = status;
                mock.sensor_calls.clear();
            });
            assert!(matches!(
                api.sample("0000:01:00.0", 0),
                Err(TelemetryState::DriverUnavailable)
            ));
            MOCK.with(|mock| assert_eq!(mock.borrow().sensor_calls, [(1, 0), (1, 1)]));
        }
    }

    #[test]
    fn absent_symbols_and_permission_failures_are_not_zero() {
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.present = true;
            mock.generation = 1;
        });
        assert!(matches!(
            Api::default().sample("0000:01:00.0", 0),
            Err(TelemetryState::Unsupported)
        ));
        let sample = Api {
            handle: Some(handle),
            ..Default::default()
        }
        .sample("0000:01:00.0", 0)
        .unwrap();
        assert_eq!(sample.overall, Err(TelemetryState::Unsupported));
        assert_eq!(sample.encode, Err(TelemetryState::Unsupported));
        assert!(matches!(sample.memory, Err(TelemetryState::Unsupported)));
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.utilization_status = 4;
            mock.memory_status = 4;
        });
        let sample = api().sample("0000:01:00.0", 0).unwrap();
        assert_eq!(sample.overall, Err(TelemetryState::PermissionDenied));
        assert!(matches!(
            sample.memory,
            Err(TelemetryState::PermissionDenied)
        ));
        assert_eq!(sample.encode, Ok(0.));
        assert_eq!(percent(0, 101), Err(TelemetryState::Stale));
    }

    #[test]
    fn process_samples_need_identity_warmup_and_missing_samples_are_not_idle() {
        let root = ProcFixture::new();
        root.process(11, 10);
        root.process(22, 20);
        let identities = HashMap::from([(11, 10), (22, 20)]);
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.present = true;
            mock.generation = 1;
            mock.entries = vec![ProcessV2 {
                pid: 11,
                used_memory: 64,
                ..Default::default()
            }];
            mock.samples = vec![usage(100, 20)];
        });
        let api = api();
        let mut cursor = ProcessCursor::default();
        let first = api.sample("0000:01:00.0", cursor.timestamp).unwrap();
        let clients = cursor.apply(&first.processes, &identities, &root.0);
        assert_eq!(clients.clients.len(), 1); // Graphics/compute duplicate deduped.
        assert_eq!(clients.clients[0].dedicated_memory_bytes, Some(64));
        assert_eq!(clients.clients[0].overall_usage_percent, None);
        assert_eq!(
            clients.clients[0].usage_state,
            TelemetryState::Warmup as i32
        );
        assert_eq!(
            clients.observations[1].state,
            TelemetryState::Available as i32
        );
        MOCK.with(|mock| mock.borrow_mut().samples = vec![usage(120, 50), usage(110, 30)]);
        let next = api.sample("0000:01:00.0", cursor.timestamp).unwrap();
        let clients = cursor.apply(&next.processes, &identities, &root.0);
        assert_eq!(clients.clients[0].overall_usage_percent, Some(50.));
        assert_eq!(
            clients.clients[0].usage_state,
            TelemetryState::Partial as i32
        );
        assert_eq!(clients.clients[0].engines.len(), 2);
        assert!(clients.clients[0]
            .engines
            .iter()
            .all(|engine| !matches!(engine.kind, 1..=3)));
        MOCK.with(|mock| mock.borrow_mut().sample_status = 6);
        let empty = api.sample("0000:01:00.0", cursor.timestamp).unwrap();
        let clients = cursor.apply(&empty.processes, &identities, &root.0);
        assert_eq!(clients.clients[0].overall_usage_percent, None);
        assert_eq!(
            clients.clients[0].usage_state,
            TelemetryState::Warmup as i32
        );
        MOCK.with(|mock| mock.borrow_mut().sample_status = 15);
        let failed = api.sample("0000:01:00.0", cursor.timestamp).unwrap();
        let clients = cursor.apply(&failed.processes, &identities, &root.0);
        assert_eq!(
            clients.clients[0].usage_state,
            TelemetryState::DriverUnavailable as i32
        );
        root.process(11, 99);
        MOCK.with(|mock| {
            let mut mock = mock.borrow_mut();
            mock.sample_status = 0;
            mock.samples = vec![usage(130, 90)];
        });
        let reused = api.sample("0000:01:00.0", cursor.timestamp).unwrap();
        let clients = cursor.apply(&reused.processes, &identities, &root.0);
        assert!(clients.clients.is_empty()); // PID changed during the collection.
        let identities = HashMap::from([(11, 99)]);
        let clients = cursor.apply(&reused.processes, &identities, &root.0);
        assert_eq!(clients.clients[0].owners[0].start_time_ticks, 99);
        assert_eq!(clients.clients[0].overall_usage_percent, None);
        assert_eq!(
            clients.clients[0].usage_state,
            TelemetryState::Warmup as i32
        );
        assert_eq!(cursor.owners.len(), 1);
        let clients = ProcessCursor::default().apply(&reused.processes, &identities, &root.0);
        assert_eq!(clients.clients[0].overall_usage_percent, None); // Device replacement reset.
    }

    #[test]
    fn arrays_are_bounded_and_failed_process_lists_are_not_empty_successes() {
        let failed = query_array::<ProcessV1>(|_, _| 4);
        assert!(matches!(failed, Err(TelemetryState::PermissionDenied)));
        let oversized = query_array::<ProcessV1>(|count, _| {
            unsafe {
                *count = 65_537;
            }
            7
        });
        assert!(matches!(oversized, Err(TelemetryState::Stale)));
        let mut calls = 0;
        let changing = query_array::<ProcessV1>(|count, _| {
            calls += 1;
            unsafe {
                *count += 1;
            }
            7
        });
        assert!(matches!(changing, Err(TelemetryState::Stale)));
        assert_eq!(calls, 4);
    }
    #[test]
    fn version_contains_structure_size_and_version_bits() {
        assert_eq!(
            memory_v2_version() & 0x00ff_ffff,
            std::mem::size_of::<MemoryV2>() as u32
        );
        assert_eq!(memory_v2_version() >> 24, 2);
        assert_ne!(memory_v2_version(), 2);
    }
    #[test]
    fn uma_and_failed_queries_do_not_become_zero() {
        assert_eq!(
            validate(0, 0, 0, None).unwrap_err(),
            TelemetryState::Unsupported
        );
        assert_eq!(
            validate(100, 101, 0, None).unwrap_err(),
            TelemetryState::Stale
        );
        let memory = validate(100, 60, 40, Some(10)).unwrap();
        assert_eq!(memory.reserved, Some(10));
        assert_eq!(state(4), TelemetryState::PermissionDenied);
    }
}
