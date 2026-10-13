/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! One snapshot for GPU tables and graphs. Percentages are max-engine values,
//! while application groups deduplicate clients before summing each engine.
use std::collections::{BTreeMap, HashMap, HashSet};

use mc_magpie::Readings;
use mc_magpie::magpie_types::gpus::{
    ClientTelemetry, DriverInfo, EngineTelemetry, MemoryTelemetry, TelemetryState,
};

use crate::i18n::{i18n, i18n_f, n_, pi18n};

pub fn engine_name(engine: &EngineTelemetry) -> String {
    match engine.name.as_str() {
        "3D" => i18n("3D"),
        "Copy" => pi18n("GPU engine", "Copy"),
        "Compute" => i18n("Compute"),
        "Video Encode" => i18n("Video Encode"),
        "Video Decode" => i18n("Video Decode"),
        "Video Encode/Decode" => i18n("Video Encode/Decode"),
        "Video" => i18n("Video"),
        "Video Enhance" => i18n("Video Enhance"),
        _ => engine.name.clone(),
    }
}

/// Extra explanation for engines whose meaning differs from Windows.
pub fn engine_note(engine: &EngineTelemetry) -> Option<String> {
    (engine.name == "Video Encode/Decode").then(|| {
        i18n("Encoding and decoding share one hardware queue; the driver does not count them separately")
    })
}

pub fn state_reason(state: TelemetryState) -> String {
    i18n(match state {
        TelemetryState::Available => n_("Measured by the GPU driver"),
        TelemetryState::Partial => {
            n_("Partial GPU data: some engines or processes are not observable")
        }
        TelemetryState::PermissionDenied => {
            n_("Permission denied while reading GPU process counters")
        }
        TelemetryState::Warmup => n_("Collecting GPU counters; waiting for the next sample"),
        TelemetryState::Stale => n_("The GPU driver has not supplied a fresh sample"),
        TelemetryState::DriverUnavailable => {
            n_("The GPU driver or its monitoring interface is unavailable")
        }
        TelemetryState::Unsupported => n_("The GPU driver does not provide this counter"),
        TelemetryState::Unknown => n_("GPU telemetry is not available for this process"),
    })
}

fn usable(state: TelemetryState) -> bool {
    matches!(state, TelemetryState::Available | TelemetryState::Partial)
}

fn percent(value: Option<f32>, state: TelemetryState) -> Option<f32> {
    value.filter(|value| usable(state) && value.is_finite() && (0. ..=100.).contains(value))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GpuEngine {
    pub gpu_index: usize,
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct GpuUsage {
    pub percent: Option<f32>,
    pub engine: Option<GpuEngine>,
    pub state: TelemetryState,
}

impl GpuUsage {
    pub fn text(&self) -> String {
        self.percent
            .map_or_else(|| "—".into(), |value| format!("{value:.0}%"))
    }

    pub fn engine_text(&self) -> String {
        self.engine.as_ref().map_or_else(
            || {
                if self.percent == Some(0.) {
                    String::new()
                } else {
                    "—".into()
                }
            },
            |engine| {
                i18n_f(
                    "GPU {} - {}",
                    &[&engine.gpu_index.to_string(), &engine.name],
                )
            },
        )
    }

    pub fn tooltip(&self) -> String {
        let mut text = state_reason(self.state);
        if self.percent.is_some_and(|value| value > 0.) && self.engine.is_none() {
            text.push('\n');
            text.push_str(&i18n(
                "Only overall utilization is available; the active engine is unknown",
            ));
        }
        text
    }
}

#[derive(Clone, Debug, Default)]
pub struct GpuStats {
    pub usage: GpuUsage,
    pub dedicated: Option<u64>,
    pub shared: Option<u64>,
    pub memory: Option<u64>,
    pub dedicated_state: TelemetryState,
    pub shared_state: TelemetryState,
    pub memory_state: TelemetryState,
    pub memory_is_resident: bool,
}

impl GpuStats {
    pub fn memory_tooltip(&self, state: TelemetryState) -> String {
        let mut text = state_reason(state);
        if self.memory.is_some() && !self.memory_is_resident {
            text.push('\n');
            text.push_str(&i18n(
                "Driver-reported allocation; residency is not available",
            ));
        }
        text
    }
}

#[derive(Clone, Debug, Default)]
pub struct GpuDevice {
    pub index: usize,
    pub usage: GpuUsage,
    pub engines: Vec<EngineTelemetry>,
    pub memory: MemoryTelemetry,
    pub driver: DriverInfo,
    pub epoch: u64,
    pub sequence: u64,
    pub age_ms: u64,
}

/// Keep adapter numbers stable for the application session, including unplug/replug.
#[derive(Default)]
pub struct GpuIndices {
    adapters: HashMap<String, usize>,
    samples: HashMap<String, (u64, u64)>,
}

impl GpuIndices {
    fn update(&mut self, readings: &Readings) {
        let mut ids: Vec<_> = readings.gpus.keys().collect();
        ids.sort_unstable();
        for id in ids {
            let next = self.adapters.len();
            self.adapters.entry(id.clone()).or_insert(next);
        }
        self.samples.retain(|id, _| readings.gpus.contains_key(id));
    }
}

#[derive(Clone)]
struct Client {
    gpu_index: usize,
    sample: ClientTelemetry,
    // Retain validated membership only for the independent full-scan oracle.
    #[cfg(test)]
    owners: HashSet<u32>,
}

#[derive(Default)]
pub struct GpuSnapshot {
    pub devices: HashMap<String, GpuDevice>,
    pub processes: HashMap<u32, GpuStats>,
    pub usage: GpuUsage,
    clients: Vec<Client>,
    clients_by_pid: HashMap<u32, Vec<usize>>,
    observations: HashMap<(usize, u32), TelemetryState>,
}

impl GpuSnapshot {
    pub fn from_readings(readings: &Readings, indices: &mut GpuIndices, interval_ms: u64) -> Self {
        indices.update(readings);
        let mut snapshot = Self::default();
        let mut ids: Vec<_> = readings.gpus.keys().collect();
        ids.sort_unstable();
        for id in ids {
            let gpu = &readings.gpus[id];
            let index = indices.adapters[id];
            let mut device = GpuDevice {
                index,
                ..Default::default()
            };
            if let Some(telemetry) = &gpu.telemetry {
                device.epoch = telemetry.epoch;
                device.sequence = telemetry.sequence;
                device.age_ms = telemetry.sample_age_ms;
                device.memory = telemetry.memory.unwrap_or_default();
                device.driver = telemetry.driver.clone().unwrap_or_default();
                let sample = (telemetry.epoch, telemetry.sequence);
                let repeated = indices.samples.insert(id.clone(), sample) == Some(sample);
                let stale =
                    repeated || telemetry.sample_age_ms > interval_ms.saturating_mul(2).max(1);
                let failed = matches!(
                    telemetry.state(),
                    TelemetryState::Stale | TelemetryState::DriverUnavailable
                );
                device.engines = telemetry.engines.clone();
                for engine in &mut device.engines {
                    engine.usage_percent = percent(engine.usage_percent, engine.state());
                    if stale || failed {
                        engine.state = if stale {
                            TelemetryState::Stale
                        } else {
                            telemetry.state()
                        } as i32;
                        engine.usage_percent = None;
                    }
                }
                if stale || failed {
                    let state = if stale {
                        TelemetryState::Stale
                    } else {
                        telemetry.state()
                    };
                    device.usage.state = state;
                    device.memory.dedicated_state = state as i32;
                    device.memory.shared_state = state as i32;
                    device.memory.dedicated_used_bytes = None;
                    device.memory.shared_used_bytes = None;
                } else {
                    device.usage = device_usage(
                        &device.engines,
                        telemetry.overall_usage_percent,
                        telemetry.overall_is_partial.unwrap_or(true),
                        telemetry.state(),
                        index,
                    );
                }
                let mut seen = HashSet::new();
                for sample in &telemetry.clients {
                    if !seen.insert(&sample.id) {
                        continue;
                    }
                    let owners = sample
                        .owners
                        .iter()
                        .filter_map(|owner| {
                            readings
                                .running_processes
                                .get(&owner.pid)
                                .filter(|process| {
                                    process.start_time_ticks == Some(owner.start_time_ticks)
                                })
                                .map(|_| owner.pid)
                        })
                        .collect::<HashSet<_>>();
                    if owners.is_empty() {
                        continue;
                    }
                    let mut sample = sample.clone();
                    if stale || failed {
                        sample.usage_state = device.usage.state as i32;
                        sample.memory_state = device.usage.state as i32;
                    }
                    let client_index = snapshot.clients.len();
                    for &pid in &owners {
                        snapshot
                            .clients_by_pid
                            .entry(pid)
                            .or_default()
                            .push(client_index);
                    }
                    snapshot.clients.push(Client {
                        gpu_index: index,
                        sample,
                        #[cfg(test)]
                        owners,
                    });
                }
                for observation in &telemetry.observations {
                    let identity_matches = readings
                        .running_processes
                        .get(&observation.pid)
                        .is_some_and(|process| {
                            process.start_time_ticks.is_some()
                                && process.start_time_ticks == observation.start_time_ticks
                        });
                    let state = if stale || failed {
                        device.usage.state
                    } else if identity_matches {
                        observation.state()
                    } else {
                        TelemetryState::Unknown
                    };
                    snapshot
                        .observations
                        .insert((index, observation.pid), state);
                }
            } else {
                device.usage.percent = percent(gpu.utilization_percent, TelemetryState::Partial);
                device.usage.state = if device.usage.percent.is_some() {
                    TelemetryState::Partial
                } else {
                    TelemetryState::Unsupported
                };
                device.memory.dedicated_state = TelemetryState::Unsupported as i32;
                device.memory.shared_state = TelemetryState::Unsupported as i32;
                // Legacy fields lack memory-region and engine provenance; do not relabel them.
            }
            snapshot.devices.insert(id.clone(), device);
        }
        snapshot.usage = snapshot
            .devices
            .values()
            .filter(|d| d.usage.percent.is_some())
            .max_by(|a, b| {
                a.usage
                    .percent
                    .unwrap()
                    .total_cmp(&b.usage.percent.unwrap())
                    .then_with(|| b.index.cmp(&a.index))
            })
            .map(|device| device.usage.clone())
            .unwrap_or_else(|| GpuUsage {
                state: if snapshot.devices.is_empty() {
                    TelemetryState::Unsupported
                } else {
                    snapshot
                        .devices
                        .values()
                        .min_by_key(|d| d.index)
                        .unwrap()
                        .usage
                        .state
                },
                ..Default::default()
            });
        if snapshot.usage.percent.is_some()
            && snapshot.devices.values().any(|d| d.usage.percent.is_none())
        {
            snapshot.usage.state = TelemetryState::Partial;
        }
        for &pid in readings.running_processes.keys() {
            snapshot.processes.insert(pid, snapshot.aggregate([pid]));
        }
        snapshot
    }

    pub fn aggregate(&self, pids: impl IntoIterator<Item = u32>) -> GpuStats {
        let pids: HashSet<_> = pids.into_iter().collect();
        let clients = self.clients_for_pids(&pids);
        self.aggregate_clients(pids, clients)
    }

    fn clients_for_pids(&self, pids: &HashSet<u32>) -> Vec<&Client> {
        let mut indices = Vec::new();
        for pid in pids {
            if let Some(client_indices) = self.clients_by_pid.get(pid) {
                indices.extend_from_slice(client_indices);
            }
        }
        // Preserve the full scan's client order for sums and state precedence.
        indices.sort_unstable();
        indices.dedup();
        indices
            .into_iter()
            .map(|index| &self.clients[index])
            .collect()
    }

    fn aggregate_clients(&self, pids: HashSet<u32>, clients: Vec<&Client>) -> GpuStats {
        let mut loads: BTreeMap<(usize, String), (f32, Option<GpuEngine>, bool)> = BTreeMap::new();
        let mut incomplete = false;
        for client in &clients {
            let sample = &client.sample;
            for engine in &sample.engines {
                let Some(value) = percent(engine.usage_percent, engine.state())
                    .filter(|_| usable(sample.usage_state()))
                else {
                    incomplete = true;
                    continue;
                };
                let entry = loads
                    .entry((client.gpu_index, engine.id.clone()))
                    .or_insert((
                        0.,
                        Some(GpuEngine {
                            gpu_index: client.gpu_index,
                            id: engine.id.clone(),
                            name: engine_name(engine),
                        }),
                        false,
                    ));
                entry.0 += value;
                entry.2 |= engine.state() == TelemetryState::Partial
                    || sample.usage_state() == TelemetryState::Partial;
            }
            // fdinfo's overall value is max(client engines), not a separate engine.
            // Summing it across clients would produce SUM(MAX), instead of MAX(SUM).
            if !sample
                .engines
                .iter()
                .any(|engine| engine.client_derived == Some(true))
                && let Some(value) = percent(sample.overall_usage_percent, sample.usage_state())
            {
                let entry = loads
                    .entry((client.gpu_index, String::new()))
                    .or_insert((0., None, true));
                entry.0 += value;
            } else if sample.engines.is_empty() {
                incomplete = true;
            }
        }
        let mut idle = !pids.is_empty() && !self.devices.is_empty();
        let mut missing = TelemetryState::Unknown;
        let mut memory_incomplete = false;
        for device in self.devices.values() {
            for pid in &pids {
                let state = self
                    .observations
                    .get(&(device.index, *pid))
                    .copied()
                    .unwrap_or(if usable(device.usage.state) {
                        TelemetryState::Unknown
                    } else {
                        device.usage.state
                    });
                if state != TelemetryState::Available {
                    idle = false;
                    incomplete = true;
                    memory_incomplete = true;
                    if missing == TelemetryState::Unknown {
                        missing = state;
                    }
                }
            }
        }
        let usage = loads
            .into_values()
            .map(|(value, engine, partial)| (value.min(100.), engine, partial))
            .max_by(|a, b| a.0.total_cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
            .map(|(value, engine, partial)| GpuUsage {
                percent: Some(value),
                engine,
                state: if partial || incomplete {
                    TelemetryState::Partial
                } else {
                    TelemetryState::Available
                },
            })
            .unwrap_or_else(|| GpuUsage {
                percent: (idle && clients.is_empty()).then_some(0.),
                state: if idle && clients.is_empty() {
                    TelemetryState::Available
                } else if self.devices.is_empty() {
                    TelemetryState::Unsupported
                } else if let Some(client) = clients.first() {
                    client.sample.usage_state()
                } else {
                    missing
                },
                engine: None,
            });
        if clients.is_empty() {
            let zero = (idle && !self.devices.is_empty()).then_some(0);
            return GpuStats {
                dedicated: zero,
                shared: zero,
                memory: zero,
                dedicated_state: usage.state,
                shared_state: usage.state,
                memory_state: usage.state,
                memory_is_resident: zero.is_some(),
                usage,
            };
        }
        let (dedicated, mut dedicated_state) = sum_memory(&clients, |c| c.dedicated_memory_bytes);
        let (shared, mut shared_state) = sum_memory(&clients, |c| c.shared_memory_bytes);
        let (memory, mut memory_state) = sum_memory(&clients, |c| {
            c.allocated_memory_bytes.or_else(|| {
                c.dedicated_memory_bytes?
                    .checked_add(c.shared_memory_bytes?)
            })
        });
        if memory_incomplete {
            for state in [&mut dedicated_state, &mut shared_state, &mut memory_state] {
                if *state == TelemetryState::Available {
                    *state = TelemetryState::Partial;
                }
            }
        }
        GpuStats {
            usage,
            dedicated,
            shared,
            memory,
            dedicated_state,
            shared_state,
            memory_state,
            memory_is_resident: clients
                .iter()
                .all(|c| c.sample.memory_is_resident == Some(true)),
        }
    }

    pub fn dedicated_capacity(&self) -> Option<u64> {
        if self.devices.is_empty() {
            return None;
        }
        self.devices.values().try_fold(0u64, |sum, d| {
            sum.checked_add(d.memory.dedicated_total_bytes?)
        })
    }
}

fn sum_memory(
    clients: &[&Client],
    value: fn(&ClientTelemetry) -> Option<u64>,
) -> (Option<u64>, TelemetryState) {
    let mut sum = None::<u64>;
    let mut partial = false;
    for client in clients {
        let Some(bytes) = value(&client.sample).filter(|_| usable(client.sample.memory_state()))
        else {
            partial = true;
            continue;
        };
        let Some(total) = sum.unwrap_or(0).checked_add(bytes) else {
            return (None, TelemetryState::Unknown);
        };
        sum = Some(total);
        partial |= client.sample.memory_state() == TelemetryState::Partial;
    }
    (
        sum,
        if sum.is_none() {
            clients
                .iter()
                .map(|c| c.sample.memory_state())
                .find(|state| !usable(*state))
                .unwrap_or(TelemetryState::Unsupported)
        } else if partial {
            TelemetryState::Partial
        } else {
            TelemetryState::Available
        },
    )
}

fn device_usage(
    engines: &[EngineTelemetry],
    overall: Option<f32>,
    overall_partial: bool,
    state: TelemetryState,
    index: usize,
) -> GpuUsage {
    let derived = engines.iter().any(|engine| {
        engine.client_derived == Some(true)
            && percent(engine.usage_percent, engine.state()).is_some()
    });
    let mut usage = GpuUsage {
        percent: if derived {
            None
        } else {
            percent(overall, TelemetryState::Available)
        },
        state: if overall_partial {
            TelemetryState::Partial
        } else {
            TelemetryState::Available
        },
        engine: None,
    };
    for engine in engines {
        if let Some(value) = percent(engine.usage_percent, engine.state())
            && usage.percent.is_none_or(|old| value > old)
        {
            usage.percent = Some(value);
            usage.engine = Some(GpuEngine {
                gpu_index: index,
                id: engine.id.clone(),
                name: engine_name(engine),
            });
            usage.state = engine.state();
        }
    }
    if usage.percent.is_none() || state == TelemetryState::Partial {
        usage.state = state;
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_magpie::magpie_types::gpus::{
        EngineKind, GpuTelemetry, ProcessIdentity, ProcessObservation,
    };
    use mc_magpie::{Gpu, Process};

    fn readings() -> Readings {
        let mut r = Readings::new();
        for pid in [10, 20] {
            r.running_processes.insert(
                pid,
                Process {
                    pid,
                    start_time_ticks: Some(100 + u64::from(pid)),
                    ..Default::default()
                },
            );
        }
        let engine = |id: &str, value| EngineTelemetry {
            id: id.into(),
            name: id.into(),
            kind: EngineKind::Render as i32,
            usage_percent: Some(value),
            state: TelemetryState::Available as i32,
            client_derived: Some(true),
        };
        let client = |id: &str, owners: &[u32], engines: Vec<EngineTelemetry>| ClientTelemetry {
            overall_usage_percent: engines
                .iter()
                .filter_map(|e| e.usage_percent)
                .reduce(f32::max),
            id: id.into(),
            owners: owners
                .iter()
                .map(|&pid| ProcessIdentity {
                    pid,
                    start_time_ticks: 100 + u64::from(pid),
                })
                .collect(),
            engines,
            dedicated_memory_bytes: Some(1024),
            shared_memory_bytes: Some(2048),
            allocated_memory_bytes: Some(3072),
            usage_state: TelemetryState::Available as i32,
            memory_state: TelemetryState::Available as i32,
            memory_is_resident: Some(true),
        };
        r.gpus.insert(
            "a".into(),
            Gpu {
                id: "a".into(),
                telemetry: Some(GpuTelemetry {
                    epoch: 1,
                    sequence: 1,
                    state: TelemetryState::Available as i32,
                    engines: vec![engine("render", 75.), engine("copy", 10.)],
                    clients: vec![
                        client("shared", &[10, 20], vec![engine("render", 25.)]),
                        client(
                            "own",
                            &[20],
                            vec![engine("render", 30.), engine("copy", 50.)],
                        ),
                    ],
                    observations: [10, 20]
                        .into_iter()
                        .map(|pid| ProcessObservation {
                            pid,
                            start_time_ticks: Some(100 + u64::from(pid)),
                            state: TelemetryState::Available as i32,
                        })
                        .collect(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        r
    }

    fn assert_stats_eq(actual: &GpuStats, expected: &GpuStats) {
        let fields = |stats: &GpuStats| {
            (
                stats.usage.percent.map(f32::to_bits),
                stats.usage.engine.clone(),
                stats.usage.state,
                stats.dedicated,
                stats.shared,
                stats.memory,
                stats.dedicated_state,
                stats.shared_state,
                stats.memory_state,
                stats.memory_is_resident,
            )
        };
        assert_eq!(fields(actual), fields(expected));
    }

    fn assert_full_scan_parity(snapshot: &GpuSnapshot, pids: impl IntoIterator<Item = u32>) {
        let pids: HashSet<_> = pids.into_iter().collect();
        let indexed = snapshot.clients_for_pids(&pids);
        // Original selection rule, independent of the candidate index.
        let full_scan: Vec<_> = snapshot
            .clients
            .iter()
            .filter(|client| !client.owners.is_disjoint(&pids))
            .collect();
        let membership = |clients: &[&Client]| {
            clients
                .iter()
                .map(|client| (client.gpu_index, client.sample.id.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(membership(&indexed), membership(&full_scan));
        // Keep PID iteration order equal, including missing-state precedence.
        let actual = snapshot.aggregate_clients(pids.clone(), indexed);
        let expected = snapshot.aggregate_clients(pids, full_scan);
        assert_stats_eq(&actual, &expected);
    }

    fn checked_snapshot(readings: &Readings, indices: &mut GpuIndices) -> GpuSnapshot {
        let snapshot = GpuSnapshot::from_readings(readings, indices, 1000);
        for &pid in readings.running_processes.keys() {
            let clients = snapshot
                .clients
                .iter()
                .filter(|client| client.owners.contains(&pid))
                .collect();
            let expected = snapshot.aggregate_clients(HashSet::from([pid]), clients);
            assert_stats_eq(&snapshot.processes[&pid], &expected);
            assert_stats_eq(&snapshot.aggregate([pid, pid]), &expected);
            assert_full_scan_parity(&snapshot, [pid, pid]);
        }
        assert_full_scan_parity(&snapshot, []);
        assert_full_scan_parity(&snapshot, [10, 20, 10]);
        assert_full_scan_parity(&snapshot, [20, 10, 20]);
        assert_full_scan_parity(&snapshot, [u32::MAX]);
        let mut pids: Vec<_> = readings.running_processes.keys().copied().collect();
        pids.extend([10, 20, 10, u32::MAX]);
        assert_full_scan_parity(&snapshot, pids);
        snapshot
    }

    #[test]
    fn groups_deduplicate_shared_clients_before_max_engine() {
        let snapshot = checked_snapshot(&readings(), &mut GpuIndices::default());
        let group = snapshot.aggregate([10, 20, 10]);
        assert_eq!(group.usage.percent, Some(55.));
        assert_eq!(group.usage.engine.unwrap().id, "render");
        assert_eq!(group.memory, Some(6144));
        assert_eq!(snapshot.usage.percent, Some(75.));
        assert_eq!(snapshot.processes[&20].usage.percent, Some(55.));
    }

    #[test]
    fn stale_and_recycled_processes_never_receive_old_values() {
        let mut r = readings();
        r.running_processes.get_mut(&10).unwrap().start_time_ticks = Some(999);
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        assert_eq!(snapshot.processes[&10].usage.percent, None);
        r.gpus
            .get_mut("a")
            .unwrap()
            .telemetry
            .as_mut()
            .unwrap()
            .sample_age_ms = 2001;
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        assert_eq!(snapshot.usage.percent, None);
        assert_eq!(snapshot.devices["a"].usage.state, TelemetryState::Stale);
        assert_eq!(snapshot.processes[&20].usage.percent, None);
        assert_eq!(snapshot.processes[&20].memory, None);
        let r = readings();
        let mut indices = GpuIndices::default();
        checked_snapshot(&r, &mut indices);
        let repeated = checked_snapshot(&r, &mut indices);
        assert_eq!(repeated.processes[&20].usage.state, TelemetryState::Stale);
        assert_eq!(repeated.devices["a"].usage.percent, None);
    }

    #[test]
    fn indices_survive_unplug_and_multiple_gpu_usage_is_max_not_mean() {
        let mut r = readings();
        r.gpus.insert(
            "b".into(),
            Gpu {
                id: "b".into(),
                utilization_percent: Some(90.),
                ..Default::default()
            },
        );
        let mut indices = GpuIndices::default();
        let snapshot = checked_snapshot(&r, &mut indices);
        assert_eq!(snapshot.devices["a"].index, 0);
        assert_eq!(snapshot.devices["b"].index, 1);
        assert_eq!(snapshot.usage.percent, Some(90.));
        r.gpus.remove("a");
        assert_eq!(checked_snapshot(&r, &mut indices).devices["b"].index, 1);
    }

    #[test]
    fn observed_idle_is_distinct_from_missing_or_unspecified_engines() {
        let mut r = readings();
        r.gpus
            .get_mut("a")
            .unwrap()
            .telemetry
            .as_mut()
            .unwrap()
            .observations
            .clear();
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        assert_eq!(snapshot.processes[&20].usage.state, TelemetryState::Partial);
        assert_eq!(
            snapshot.processes[&20].dedicated_state,
            TelemetryState::Partial
        );
        assert_eq!(
            snapshot.processes[&20].shared_state,
            TelemetryState::Partial
        );
        assert_eq!(
            snapshot.processes[&20].memory_state,
            TelemetryState::Partial
        );
        let mut r = readings();
        r.gpus
            .get_mut("a")
            .unwrap()
            .telemetry
            .as_mut()
            .unwrap()
            .clients
            .clear();
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        assert_eq!(snapshot.processes[&10].usage.percent, Some(0.));
        assert_eq!(snapshot.processes[&10].memory, Some(0));
        r.gpus
            .get_mut("a")
            .unwrap()
            .telemetry
            .as_mut()
            .unwrap()
            .observations
            .clear();
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        assert_eq!(snapshot.processes[&10].usage.percent, None);
        assert_eq!(snapshot.processes[&10].memory, None);
    }

    #[test]
    fn indexed_aggregation_keeps_shared_clients_and_device_local_ids() {
        let mut r = readings();
        {
            let telemetry = r.gpus.get_mut("a").unwrap().telemetry.as_mut().unwrap();
            let repeated_owner = telemetry.clients[0].owners[0];
            telemetry.clients[0].owners.extend([
                repeated_owner,
                ProcessIdentity {
                    pid: 10,
                    start_time_ticks: 999,
                },
                ProcessIdentity {
                    pid: 30,
                    start_time_ticks: 130,
                },
            ]);
            let mut duplicate = telemetry.clients[0].clone();
            duplicate.dedicated_memory_bytes = Some(u64::MAX);
            telemetry.clients.push(duplicate);
        }
        let mut second_gpu = r.gpus["a"].clone();
        second_gpu.id = "b".into();
        second_gpu.telemetry.as_mut().unwrap().clients.truncate(1);
        r.gpus.insert("b".into(), second_gpu);
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        assert_eq!(snapshot.clients.len(), 3);
        let group = snapshot.aggregate([10, 20, 10, 20]);
        assert_eq!(group.usage.percent, Some(55.));
        assert_eq!(group.usage.engine.as_ref().unwrap().gpu_index, 0);
        assert_eq!(group.dedicated, Some(3072));
        assert_eq!(group.shared, Some(6144));
        assert_eq!(group.memory, Some(9216));
        assert_eq!(snapshot.processes[&10].memory, Some(6144));
    }

    #[test]
    fn indexed_aggregation_does_not_keep_missing_or_unidentified_owners() {
        let mut r = readings();
        let mut indices = GpuIndices::default();
        let first = checked_snapshot(&r, &mut indices);
        assert!(!first.clients_by_pid.is_empty());
        r.running_processes.get_mut(&10).unwrap().start_time_ticks = None;
        r.running_processes.remove(&20);
        r.gpus
            .get_mut("a")
            .unwrap()
            .telemetry
            .as_mut()
            .unwrap()
            .sequence += 1;
        let snapshot = checked_snapshot(&r, &mut indices);
        assert!(snapshot.clients.is_empty());
        assert!(snapshot.clients_by_pid.is_empty());
        assert_eq!(snapshot.processes[&10].usage.state, TelemetryState::Unknown);
        assert_eq!(snapshot.processes[&10].memory, None);
        assert_eq!(
            snapshot.aggregate([20]).usage.state,
            TelemetryState::Unknown
        );
        assert_eq!(first.processes[&20].usage.percent, Some(55.));
    }

    #[test]
    fn indexed_aggregation_preserves_rounding_and_client_state_precedence() {
        let mut r = readings();
        {
            let telemetry = r.gpus.get_mut("a").unwrap().telemetry.as_mut().unwrap();
            telemetry.clients[0].engines[0].usage_percent = Some(64.);
            let mut small_engine = telemetry.clients[0].engines[0].clone();
            // Half an ULP at 64: summing these first would change the result.
            small_engine.usage_percent = Some(f32::EPSILON * 32.);
            telemetry.clients[1].engines = vec![small_engine];
            let mut third = telemetry.clients[1].clone();
            third.id = "third".into();
            telemetry.clients.push(third);
        }
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        assert_eq!(
            snapshot
                .aggregate([20, 10, 20])
                .usage
                .percent
                .map(f32::to_bits),
            Some(64_f32.to_bits())
        );
        {
            let telemetry = r.gpus.get_mut("a").unwrap().telemetry.as_mut().unwrap();
            for (client, state) in telemetry.clients.iter_mut().zip([
                TelemetryState::Warmup,
                TelemetryState::PermissionDenied,
                TelemetryState::Unsupported,
            ]) {
                client.usage_state = state as i32;
                client.memory_state = state as i32;
            }
            telemetry.clients[1].memory_is_resident = Some(false);
        }
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        let group = snapshot.aggregate([10, 20]);
        assert_eq!(group.usage.percent, None);
        assert_eq!(group.usage.state, TelemetryState::Warmup);
        assert_eq!(group.dedicated_state, TelemetryState::Warmup);
        assert_eq!(group.shared_state, TelemetryState::Warmup);
        assert_eq!(group.memory_state, TelemetryState::Warmup);
        assert!(!group.memory_is_resident);
        r.gpus
            .get_mut("a")
            .unwrap()
            .telemetry
            .as_mut()
            .unwrap()
            .clients
            .swap(0, 1);
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        let group = snapshot.aggregate([10, 20]);
        assert_eq!(group.usage.state, TelemetryState::PermissionDenied);
        assert_eq!(group.dedicated_state, TelemetryState::PermissionDenied);
        assert_eq!(group.shared_state, TelemetryState::PermissionDenied);
        assert_eq!(group.memory_state, TelemetryState::PermissionDenied);
    }

    #[test]
    fn indexed_aggregation_matches_partial_and_overflow_memory() {
        let mut r = readings();
        {
            let telemetry = r.gpus.get_mut("a").unwrap().telemetry.as_mut().unwrap();
            telemetry.clients[0].usage_state = TelemetryState::Partial as i32;
            telemetry.clients[0].memory_state = TelemetryState::Partial as i32;
            telemetry.clients[1].dedicated_memory_bytes = None;
            telemetry.clients[1].allocated_memory_bytes = None;
            telemetry.clients[1].memory_is_resident = None;
        }
        let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
        let group = snapshot.aggregate([10, 20, 10]);
        assert_eq!(
            (group.dedicated, group.shared, group.memory),
            (Some(1024), Some(4096), Some(3072))
        );
        assert_eq!(group.usage.state, TelemetryState::Partial);
        assert_eq!(group.dedicated_state, TelemetryState::Partial);
        assert_eq!(group.shared_state, TelemetryState::Partial);
        assert_eq!(group.memory_state, TelemetryState::Partial);
        assert!(!group.memory_is_resident);

        let mut r = readings();
        for allocated in [true, false] {
            {
                let telemetry = r.gpus.get_mut("a").unwrap().telemetry.as_mut().unwrap();
                for (index, client) in telemetry.clients.iter_mut().enumerate() {
                    let dedicated = if index == 0 { u64::MAX } else { 1 };
                    client.dedicated_memory_bytes = Some(dedicated);
                    client.shared_memory_bytes = Some(if index == 0 { 1 } else { 2 });
                    client.allocated_memory_bytes = allocated.then_some(dedicated);
                }
            }
            let snapshot = checked_snapshot(&r, &mut GpuIndices::default());
            let group = snapshot.aggregate([10, 20]);
            assert_eq!(group.dedicated, None);
            assert_eq!(group.dedicated_state, TelemetryState::Unknown);
            assert_eq!(group.shared, Some(3));
            assert_eq!(group.shared_state, TelemetryState::Available);
            if allocated {
                assert_eq!(group.memory, None);
                assert_eq!(group.memory_state, TelemetryState::Unknown);
            } else {
                assert_eq!(group.memory, Some(3));
                assert_eq!(group.memory_state, TelemetryState::Partial);
            }
            assert!(group.memory_is_resident);
        }
    }

    #[test]
    fn indexed_aggregation_matches_synthetic_series_and_empty_devices() {
        let mut indices = GpuIndices::default();
        for r in mc_magpie::synthetic_series(4, 32) {
            checked_snapshot(&r, &mut indices);
        }
        let snapshot = checked_snapshot(&Readings::new(), &mut indices);
        assert_eq!(
            snapshot.aggregate([u32::MAX]).usage.state,
            TelemetryState::Unsupported
        );
        assert!(snapshot.clients_by_pid.is_empty());
    }
}
