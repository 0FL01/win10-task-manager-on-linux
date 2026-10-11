/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! DRM fdinfo accounting, following the kernel's drm-usage-stats contract.
//! No PID/FD heuristic is a substitute for a device and drm-client-id pair.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use magpie_platform::gpus::{
    ClientTelemetry, EngineKind, EngineTelemetry, ProcessIdentity, ProcessObservation,
    TelemetryState,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Counter {
    ns: Option<u64>,
    cycles: Option<u64>,
    total_cycles: Option<u64>,
    maxfreq: Option<u64>,
    capacity: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct Parsed {
    driver: String,
    pdev: Option<String>,
    client_id: Option<u64>,
    engines: BTreeMap<String, Counter>,
    resident: BTreeMap<String, u64>,
    allocated: BTreeMap<String, u64>,
    invalid: bool,
}

fn number(value: &str, units: &[(&str, u64)]) -> Option<u64> {
    let mut words = value.split_whitespace();
    let n: u64 = words.next()?.parse().ok()?;
    let factor = match words.next() {
        None => 1,
        Some(unit) => units.iter().find(|(name, _)| *name == unit)?.1,
    };
    if words.next().is_some() {
        return None;
    }
    n.checked_mul(factor)
}

fn parse(text: &str) -> Parsed {
    let mut result = Parsed::default();
    let mut keys = BTreeSet::new();
    let mut legacy_memory = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if !key.starts_with("drm-") {
            continue;
        }
        if !keys.insert(key) {
            result.invalid = true;
        }
        match key {
            "drm-driver" => result.driver = value.to_owned(),
            "drm-pdev" => {
                result.pdev = Some(
                    super::canonical_pci_address(value)
                        .unwrap_or_else(|| value.to_ascii_lowercase()),
                )
            }
            "drm-client-id" => {
                result.client_id = number(value, &[]);
                result.invalid |= result.client_id.is_none();
            }
            _ => {
                let engine_fields = [
                    ("drm-engine-capacity-", 0),
                    ("drm-engine-", 1),
                    ("drm-total-cycles-", 2),
                    ("drm-cycles-", 3),
                    ("drm-maxfreq-", 4),
                ];
                if let Some((id, field)) = engine_fields
                    .iter()
                    .find_map(|(prefix, field)| key.strip_prefix(prefix).map(|id| (id, field)))
                {
                    let value = match field {
                        1 => number(value, &[("ns", 1)]),
                        4 => number(
                            value,
                            &[("Hz", 1), ("KHz", 1000), ("kHz", 1000), ("MHz", 1_000_000)],
                        ),
                        _ => number(value, &[]),
                    };
                    result.invalid |=
                        value.is_none() || id.is_empty() || (*field == 0 && value == Some(0));
                    let engine = result.engines.entry(id.to_owned()).or_default();
                    match field {
                        0 => engine.capacity = value,
                        1 => engine.ns = value,
                        2 => engine.total_cycles = value,
                        3 => engine.cycles = value,
                        _ => engine.maxfreq = value,
                    }
                } else {
                    let memory_fields =
                        [("drm-resident-", 0), ("drm-total-", 1), ("drm-memory-", 2)];
                    if let Some((region, field)) =
                        memory_fields.iter().find_map(|(prefix, field)| {
                            key.strip_prefix(prefix).map(|region| (region, field))
                        })
                    {
                        if let Some(bytes) = number(
                            value,
                            &[("KiB", 1024), ("kB", 1024), ("MiB", 1024 * 1024), ("B", 1)],
                        ) {
                            match field {
                                0 => {
                                    result.resident.insert(region.to_owned(), bytes);
                                }
                                1 => {
                                    result.allocated.insert(region.to_owned(), bytes);
                                }
                                _ => {
                                    legacy_memory.insert(region.to_owned(), bytes);
                                }
                            }
                        } else {
                            result.invalid = true;
                        }
                    }
                }
            }
        }
    }
    // drm-memory is the deprecated resident alias on amdgpu. Other legacy
    // drivers do not promise residency, so preserve those as allocations.
    for (region, bytes) in legacy_memory {
        if result.driver == "amdgpu" {
            result.resident.entry(region).or_insert(bytes);
        } else {
            result.allocated.entry(region).or_insert(bytes);
        }
    }
    result
}

/// amdgpu engines present in the hardware, from the IP discovery table in sysfs, and whether video
/// encode and decode share one queue. amdgpu omits engines a client never used from fdinfo, so
/// these are listed before first use. From VCN 4.0 the kernel submits decode jobs to the unified
/// ring it reports as `enc`; older VCN and UVD/VCE report `dec` and `enc` separately. Without the
/// table nothing is guessed.
pub(super) fn amdgpu_hw_engines(sysfs_device: &Path) -> (Vec<String>, Option<bool>) {
    let major = |block: &Path| {
        std::fs::read_dir(block)
            .ok()?
            .flatten()
            .filter_map(|instance| {
                std::fs::read_to_string(instance.path().join("major"))
                    .ok()?
                    .trim()
                    .parse::<u32>()
                    .ok()
            })
            .max()
    };
    let Ok(dies) = std::fs::read_dir(sysfs_device.join("ip_discovery/die")) else {
        return (vec![], None);
    };
    let mut ids = BTreeSet::new();
    let (mut vcn, mut vce) = (None, false);
    for die in dies.flatten() {
        let Ok(blocks) = std::fs::read_dir(die.path()) else {
            continue;
        };
        for block in blocks.flatten() {
            match block.file_name().to_string_lossy().as_ref() {
                "GC" => ids.extend(["gfx", "compute"]),
                name if name.starts_with("SDMA") => {
                    ids.insert("dma");
                }
                "UVD" | "VCN" => vcn = vcn.max(major(&block.path())),
                "VCE" => vce = true,
                _ => {}
            }
        }
    }
    // UVD 5–7 (paired with VCE) predate VCN and never use the unified queue.
    let unified = vcn.map(|major| major >= 4 && !vce);
    match unified {
        Some(true) => ids.insert("enc"),
        Some(false) => ids.insert("dec") | ids.insert("enc"),
        None => false,
    };
    (ids.into_iter().map(String::from).collect(), unified)
}

/// Kind and display name of an engine. amdgpu names follow the Windows engine names; the unified
/// VCN ring (`enc` with `video_unified`) carries both encode and decode.
pub(super) fn engine_label(driver: &str, id: &str, video_unified: bool) -> (EngineKind, String) {
    let (kind, name) = match (driver, id) {
        ("amdgpu", "gfx") => (EngineKind::Render, "3D"),
        ("amdgpu", "dma" | "sdma") => (EngineKind::Copy, "Copy"),
        ("amdgpu", "compute") => (EngineKind::Compute, "Compute"),
        ("amdgpu", "dec") => (EngineKind::Decode, "Video Decode"),
        ("amdgpu", "enc") if video_unified => (EngineKind::Video, "Video Encode/Decode"),
        ("amdgpu", "enc") => (EngineKind::Encode, "Video Encode"),
        ("amdgpu", "enc_1") => (EngineKind::Encode, id),
        _ => (engine_kind(driver, id), id),
    };
    (kind, name.to_owned())
}

pub(super) fn engine_kind(driver: &str, id: &str) -> EngineKind {
    match driver {
        "amdgpu" => match id {
            "gfx" => EngineKind::Render,
            "dma" | "sdma" => EngineKind::Copy,
            "compute" => EngineKind::Compute,
            "enc" | "enc_1" => EngineKind::Encode,
            "dec" => EngineKind::Decode,
            "enc_dec" => EngineKind::Video,
            _ => EngineKind::Other,
        },
        "i915" | "xe" => match id {
            "render" | "rcs" => EngineKind::Render,
            "copy" | "bcs" => EngineKind::Copy,
            "compute" | "ccs" => EngineKind::Compute,
            "video" | "vcs" => EngineKind::Video,
            "video-enhance" | "vecs" => EngineKind::VideoEnhance,
            _ => EngineKind::Other,
        },
        _ => EngineKind::Other,
    }
}

fn engine(
    driver: &str,
    id: &str,
    video_unified: bool,
    usage: Option<f32>,
    state: TelemetryState,
) -> EngineTelemetry {
    let (kind, name) = engine_label(driver, id, video_unified);
    EngineTelemetry {
        id: id.to_owned(),
        name,
        kind: kind as i32,
        usage_percent: usage,
        state: state as i32,
        client_derived: Some(true),
    }
}

#[derive(Clone)]
pub(super) struct Device {
    pub id: String,
    pub driver: String,
    pub nodes: Vec<(u32, u32)>,
    pub generation: u64,
    pub covered: bool,
    /// PCI/platform device directory, read for the amdgpu IP discovery table
    pub sysfs_device: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct Sample {
    pub clients: Vec<ClientTelemetry>,
    pub engines: Vec<EngineTelemetry>,
    pub observations: Vec<ProcessObservation>,
    pub partial: bool,
    pub state: Option<TelemetryState>,
}

struct Group {
    parsed: Parsed,
    owners: BTreeSet<(u32, u64)>,
}

struct Baseline {
    owners: BTreeSet<(u32, u64)>,
    driver: String,
    counters: HashMap<String, (Counter, Duration)>,
    last_seen: Duration,
}

#[derive(Default)]
pub(super) struct Collector {
    baselines: HashMap<(String, u64), Baseline>,
    known_engines: HashMap<String, BTreeMap<String, String>>,
    devices: HashMap<String, DeviceToken>,
    /// Hardware engine ids and the unified-video flag per device (see [`amdgpu_hw_engines`]),
    /// read once per device generation.
    hardware: HashMap<String, (Vec<String>, Option<bool>)>,
}

type DeviceToken = (String, Vec<(u32, u32)>, u64);

pub(super) fn starttime_from_stat(text: &str) -> Option<u64> {
    // Field 22; comm is parenthesized and can itself contain spaces/parens.
    let end = text.rfind(')')?;
    text[end + 1..].split_whitespace().nth(19)?.parse().ok()
}

fn identity(root: &Path, pid: u32) -> std::io::Result<Option<u64>> {
    let text = std::fs::read_to_string(root.join(pid.to_string()).join("stat"))?;
    Ok(starttime_from_stat(&text))
}

pub(super) fn process_identities(root: &Path) -> HashMap<u32, u64> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse().ok()?;
            Some((pid, identity(root, pid).ok().flatten()?))
        })
        .collect()
}

fn denied(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied
}

fn device_for<'a>(parsed: &Parsed, fd: &Path, devices: &'a [Device]) -> Option<&'a Device> {
    let by_pdev = parsed.pdev.as_ref().and_then(|id| {
        let mut matches = devices.iter().filter(|d| d.id.eq_ignore_ascii_case(id));
        let found = matches.next()?;
        matches.next().is_none().then_some(found)
    });
    let by_node = std::fs::metadata(fd).ok().and_then(|metadata| {
        let dev = metadata.rdev();
        let node = (libc::major(dev), libc::minor(dev));
        let mut matches = devices.iter().filter(|d| d.nodes.contains(&node));
        let found = matches.next()?;
        matches.next().is_none().then_some(found)
    });
    match (by_pdev, by_node) {
        (Some(a), Some(b)) if a.id != b.id => None,
        // An explicitly supplied but unresolvable PCI address is never guessed.
        (None, _) if parsed.pdev.is_some() => None,
        (Some(a), _) => Some(a),
        (_, b) => b,
    }
}

fn merge(target: &mut Parsed, other: Parsed) {
    if target.driver != other.driver {
        target.invalid = true;
    }
    target.invalid |= other.invalid;
    // Duplicate fd reads happen at slightly different instants. Take maxima,
    // never sums; capacities and clock parameters must agree.
    for (id, counter) in other.engines {
        let old = target.engines.entry(id).or_default();
        if old.capacity.is_some() && counter.capacity.is_some() && old.capacity != counter.capacity
        {
            target.invalid = true;
        }
        if old.maxfreq.is_some() && counter.maxfreq.is_some() && old.maxfreq != counter.maxfreq {
            target.invalid = true;
        }
        old.ns = old.ns.max(counter.ns);
        old.cycles = old.cycles.max(counter.cycles);
        old.total_cycles = old.total_cycles.max(counter.total_cycles);
        old.capacity = old.capacity.or(counter.capacity);
        old.maxfreq = old.maxfreq.or(counter.maxfreq);
    }
    for (region, value) in other.resident {
        target
            .resident
            .entry(region)
            .and_modify(|old| *old = (*old).max(value))
            .or_insert(value);
    }
    for (region, value) in other.allocated {
        target
            .allocated
            .entry(region)
            .and_modify(|old| *old = (*old).max(value))
            .or_insert(value);
    }
}

fn usage(old: &Counter, new: &Counter, elapsed: Duration) -> (Option<f32>, TelemetryState) {
    if new.capacity != old.capacity || new.maxfreq != old.maxfreq {
        return (None, TelemetryState::Warmup);
    }
    let capacity = new.capacity.unwrap_or(1) as f64;
    let ratio = if let (Some(previous), Some(current)) = (old.ns, new.ns) {
        if current < previous {
            return (None, TelemetryState::Stale);
        }
        if elapsed.is_zero() {
            return (None, TelemetryState::Warmup);
        }
        (current - previous) as f64 / elapsed.as_nanos() as f64 / capacity
    } else if let (Some(previous), Some(current)) = (old.cycles, new.cycles) {
        if current < previous {
            return (None, TelemetryState::Stale);
        }
        let delta = (current - previous) as f64;
        if let (Some(before), Some(after)) = (old.total_cycles, new.total_cycles) {
            if after < before {
                return (None, TelemetryState::Stale);
            }
            if after == before {
                return (None, TelemetryState::Warmup);
            }
            delta / (after - before) as f64 / capacity
        } else if let Some(hz) = new.maxfreq.filter(|hz| *hz > 0) {
            if elapsed.is_zero() {
                return (None, TelemetryState::Warmup);
            }
            delta / (elapsed.as_secs_f64() * hz as f64) / capacity
        } else {
            return (None, TelemetryState::Unsupported);
        }
    } else {
        return (None, TelemetryState::Unsupported);
    };
    (
        Some((ratio * 100.).clamp(0., 100.) as f32),
        TelemetryState::Available,
    )
}

fn known_region(region: &str, dedicated: bool) -> bool {
    let prefixes: &[&str] = if dedicated {
        &["vram", "local", "lmem"]
    } else {
        &["gtt", "system", "memory", "smem"]
    };
    prefixes.iter().any(|prefix| {
        region
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

fn sum_regions(regions: &BTreeMap<String, u64>, dedicated: bool) -> Option<u64> {
    let mut result: Option<u64> = None;
    for (region, bytes) in regions {
        if known_region(region, dedicated) {
            result = Some(result.unwrap_or(0).checked_add(*bytes)?);
        }
    }
    result
}

impl Collector {
    pub fn collect(
        &mut self,
        proc_root: &Path,
        devices: &[Device],
        now: Duration,
    ) -> HashMap<String, Sample> {
        let mut samples: HashMap<_, _> = devices
            .iter()
            .filter(|device| device.covered)
            .map(|d| (d.id.clone(), Sample::default()))
            .collect();
        self.devices.retain(|id, _| samples.contains_key(id));
        for device in devices.iter().filter(|device| device.covered) {
            let token = (
                device.driver.clone(),
                device.nodes.clone(),
                device.generation,
            );
            if self.devices.get(&device.id) != Some(&token) {
                self.baselines.retain(|(id, _), _| id != &device.id);
                self.known_engines.remove(&device.id);
                self.devices.insert(device.id.clone(), token);
                let hardware = match (device.driver.as_str(), &device.sysfs_device) {
                    ("amdgpu", Some(path)) => amdgpu_hw_engines(path),
                    _ => (vec![], None),
                };
                self.hardware.insert(device.id.clone(), hardware);
            }
            if let Some((ids, _)) = self.hardware.get(&device.id) {
                let known = self.known_engines.entry(device.id.clone()).or_default();
                for id in ids {
                    known
                        .entry(id.clone())
                        .or_insert_with(|| device.driver.clone());
                }
            }
        }
        self.hardware.retain(|id, _| samples.contains_key(id));
        self.baselines.retain(|(device, _), baseline| {
            samples.contains_key(device)
                && now.saturating_sub(baseline.last_seen) < Duration::from_secs(60)
        });
        self.known_engines.retain(|id, _| samples.contains_key(id));
        if samples.is_empty() {
            return samples;
        }
        let mut groups: HashMap<(String, u64), Group> = HashMap::new();
        let entries = match std::fs::read_dir(proc_root) {
            Ok(entries) => entries,
            Err(error) => {
                for sample in samples.values_mut() {
                    sample.partial = true;
                    sample.state = Some(if denied(&error) {
                        TelemetryState::PermissionDenied
                    } else {
                        TelemetryState::Stale
                    });
                }
                return samples;
            }
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let start = match identity(proc_root, pid) {
                Ok(Some(start)) => start,
                Ok(None) => continue,
                Err(error) => {
                    if denied(&error) {
                        for sample in samples.values_mut() {
                            sample.partial = true;
                            sample.observations.push(ProcessObservation {
                                pid,
                                start_time_ticks: None,
                                state: TelemetryState::PermissionDenied as i32,
                            });
                        }
                    }
                    continue;
                }
            };
            let mut states: HashMap<String, TelemetryState> = HashMap::new();
            let mut failed = None;
            match std::fs::read_dir(entry.path().join("fdinfo")) {
                Ok(fds) => {
                    for fd in fds {
                        let fd = match fd {
                            Ok(fd) => fd,
                            Err(error) => {
                                failed = Some(if denied(&error) {
                                    TelemetryState::PermissionDenied
                                } else {
                                    TelemetryState::Stale
                                });
                                continue;
                            }
                        };
                        let text = match std::fs::read_to_string(fd.path()) {
                            Ok(text) => text,
                            Err(error) => {
                                failed = Some(if denied(&error) {
                                    TelemetryState::PermissionDenied
                                } else {
                                    TelemetryState::Stale
                                });
                                continue;
                            }
                        };
                        let parsed = parse(&text);
                        if parsed.driver.is_empty()
                            && parsed.client_id.is_none()
                            && parsed.pdev.is_none()
                        {
                            continue;
                        }
                        let Some(device) = device_for(
                            &parsed,
                            &entry.path().join("fd").join(fd.file_name()),
                            devices,
                        ) else {
                            failed = Some(TelemetryState::Unsupported);
                            continue;
                        };
                        if !device.covered {
                            continue;
                        }
                        if parsed.driver.is_empty() {
                            states.insert(device.id.clone(), TelemetryState::Unsupported);
                            continue;
                        }
                        let Some(client_id) = parsed.client_id.filter(|_| !parsed.invalid) else {
                            states.insert(device.id.clone(), TelemetryState::Unsupported);
                            continue;
                        };
                        if parsed.driver != device.driver && !device.driver.is_empty() {
                            states.insert(device.id.clone(), TelemetryState::Unsupported);
                            continue;
                        }
                        let group =
                            groups
                                .entry((device.id.clone(), client_id))
                                .or_insert_with(|| Group {
                                    parsed: parsed.clone(),
                                    owners: BTreeSet::new(),
                                });
                        merge(&mut group.parsed, parsed);
                        group.owners.insert((pid, start));
                    }
                }
                Err(error) => {
                    failed = Some(if denied(&error) {
                        TelemetryState::PermissionDenied
                    } else {
                        TelemetryState::Stale
                    })
                }
            }
            // Processes can exit or PIDs can be recycled during the scan.
            if identity(proc_root, pid).ok().flatten() != Some(start) {
                for group in groups.values_mut() {
                    group.owners.remove(&(pid, start));
                }
                failed = Some(TelemetryState::Stale);
            }
            for (id, sample) in &mut samples {
                let state = failed
                    .or_else(|| states.get(id).copied())
                    .unwrap_or(TelemetryState::Available);
                sample.partial |= state != TelemetryState::Available;
                sample.observations.push(ProcessObservation {
                    pid,
                    start_time_ticks: Some(start),
                    state: state as i32,
                });
            }
        }
        for ((device, client_id), group) in groups {
            if group.owners.is_empty() {
                continue;
            }
            let sample = samples.get_mut(&device).unwrap();
            if group.parsed.invalid {
                sample.partial = true;
                for observation in &mut sample.observations {
                    if group.owners.contains(&(
                        observation.pid,
                        observation.start_time_ticks.unwrap_or_default(),
                    )) {
                        observation.state = TelemetryState::Unsupported as i32;
                    }
                }
                continue;
            }
            let key = (device.clone(), client_id);
            let baseline = self.baselines.entry(key).or_insert_with(|| Baseline {
                owners: group.owners.clone(),
                driver: group.parsed.driver.clone(),
                counters: HashMap::new(),
                last_seen: now,
            });
            if baseline.owners.is_disjoint(&group.owners) || baseline.driver != group.parsed.driver
            {
                baseline.counters.clear();
            }
            baseline.owners = group.owners.clone();
            baseline.driver = group.parsed.driver.clone();
            baseline.last_seen = now;
            let unified = self.hardware.get(&device).and_then(|h| h.1) == Some(true);
            // amdgpu omits engines whose cumulative counter is zero, so a missing key means the
            // client never used that engine. A counter that vanished is not a zero.
            let vanished = baseline
                .counters
                .keys()
                .any(|id| !group.parsed.engines.contains_key(id));
            let absent_is_zero = group.parsed.driver == "amdgpu" && !vanished;
            // amdgpu keeps a vanished counter so the client stays unknown until it is re-identified
            if group.parsed.driver != "amdgpu" {
                baseline
                    .counters
                    .retain(|id, _| group.parsed.engines.contains_key(id));
            }
            let mut engines = vec![];
            for (id, counter) in &group.parsed.engines {
                let (percent, state) = baseline
                    .counters
                    .get(id)
                    .map(|(old, time)| usage(old, counter, now.saturating_sub(*time)))
                    .unwrap_or((None, TelemetryState::Warmup));
                // Preserve BOTH the high value and its timestamp on a temporary
                // decrease; otherwise the recovery sample invents a spike.
                if state != TelemetryState::Stale {
                    baseline.counters.insert(id.clone(), (counter.clone(), now));
                }
                engines.push(engine(&group.parsed.driver, id, unified, percent, state));
                self.known_engines
                    .entry(device.clone())
                    .or_default()
                    .insert(id.clone(), group.parsed.driver.clone());
            }
            let usage_state = if engines.is_empty() && absent_is_zero {
                TelemetryState::Available
            } else if engines.is_empty() {
                TelemetryState::Unsupported
            } else if engines
                .iter()
                .all(|e| e.state == TelemetryState::Available as i32)
            {
                TelemetryState::Available
            } else if engines.iter().any(|e| e.usage_percent.is_some()) {
                TelemetryState::Partial
            } else if engines
                .iter()
                .any(|e| e.state == TelemetryState::Stale as i32)
            {
                TelemetryState::Stale
            } else if engines
                .iter()
                .any(|e| e.state == TelemetryState::Unsupported as i32)
            {
                TelemetryState::Unsupported
            } else {
                TelemetryState::Warmup
            };
            sample.partial |= usage_state == TelemetryState::Unsupported;
            let dedicated = sum_regions(&group.parsed.resident, true);
            let shared = sum_regions(&group.parsed.resident, false);
            let allocated = (!group.parsed.allocated.is_empty())
                .then(|| {
                    group
                        .parsed
                        .allocated
                        .values()
                        .try_fold(0u64, |sum, bytes| sum.checked_add(*bytes))
                })
                .flatten();
            let resident = dedicated.is_some() || shared.is_some();
            let memory_state = if resident
                && group
                    .parsed
                    .resident
                    .keys()
                    .all(|region| known_region(region, true) || known_region(region, false))
            {
                TelemetryState::Available
            } else if resident || allocated.is_some() {
                TelemetryState::Partial
            } else {
                TelemetryState::Unsupported
            };
            let overall = engines
                .iter()
                .filter_map(|e| e.usage_percent)
                .reduce(f32::max)
                .or((engines.is_empty() && absent_is_zero).then_some(0.));
            sample.clients.push(ClientTelemetry {
                id: format!("drm:{client_id}"),
                owners: group
                    .owners
                    .into_iter()
                    .map(|(pid, start_time_ticks)| ProcessIdentity {
                        pid,
                        start_time_ticks,
                    })
                    .collect(),
                engines,
                overall_usage_percent: overall,
                dedicated_memory_bytes: dedicated,
                shared_memory_bytes: shared,
                allocated_memory_bytes: allocated,
                usage_state: usage_state as i32,
                memory_state: memory_state as i32,
                memory_is_resident: if resident {
                    Some(true)
                } else {
                    allocated.map(|_| false)
                },
            });
        }
        self.baselines.retain(|(device, _), baseline| {
            samples.contains_key(device)
                && now.saturating_sub(baseline.last_seen) < Duration::from_secs(60)
        });
        self.known_engines.retain(|id, _| samples.contains_key(id));
        for (id, sample) in &mut samples {
            sample.clients.sort_by(|a, b| a.id.cmp(&b.id));
            for observation in &mut sample.observations {
                if observation.state != TelemetryState::Available as i32 {
                    continue;
                }
                let clients = sample
                    .clients
                    .iter()
                    .filter(|client| {
                        client.owners.iter().any(|owner| {
                            owner.pid == observation.pid
                                && Some(owner.start_time_ticks) == observation.start_time_ticks
                        })
                    })
                    .collect::<Vec<_>>();
                observation.state = if clients
                    .iter()
                    .all(|client| client.usage_state == TelemetryState::Available as i32)
                {
                    TelemetryState::Available
                } else if clients
                    .iter()
                    .any(|client| client.overall_usage_percent.is_some())
                {
                    TelemetryState::Partial
                } else if clients
                    .iter()
                    .any(|client| client.usage_state == TelemetryState::Stale as i32)
                {
                    TelemetryState::Stale
                } else if clients
                    .iter()
                    .any(|client| client.usage_state == TelemetryState::Warmup as i32)
                {
                    TelemetryState::Warmup
                } else {
                    TelemetryState::Unsupported
                } as i32;
            }
            sample.observations.sort_by_key(|o| o.pid);
            let unified = self.hardware.get(id).and_then(|h| h.1) == Some(true);
            if let Some(known) = self.known_engines.get(id) {
                for (engine_id, driver) in known {
                    // amdgpu omits engines with a zero counter: a client without the key is idle
                    let absent_is_zero = driver == "amdgpu";
                    let values: Vec<_> = sample
                        .clients
                        .iter()
                        .filter_map(|client| client.engines.iter().find(|e| &e.id == engine_id))
                        .collect();
                    // amdgpu: clients without the key count as idle unless their own counters
                    // are not fully known.
                    let unknown = |client: &&ClientTelemetry| {
                        client.usage_state == TelemetryState::Unsupported as i32
                            || client.usage_state == TelemetryState::Partial as i32
                    };
                    let missing_active = values.is_empty()
                        && !sample.clients.is_empty()
                        && (!absent_is_zero || sample.clients.iter().all(|c| unknown(&c)));
                    let incomplete = if absent_is_zero {
                        sample.clients.iter().any(|c| unknown(&c))
                    } else {
                        values.len() < sample.clients.len()
                    };
                    let valid = !missing_active && values.iter().all(|e| e.usage_percent.is_some());
                    let value = valid.then(|| {
                        values
                            .iter()
                            .filter_map(|e| e.usage_percent)
                            .fold(0., |sum, value| sum + value)
                            .min(100.)
                    });
                    let state = if missing_active {
                        TelemetryState::Unsupported
                    } else if sample.partial || incomplete {
                        TelemetryState::Partial
                    } else if valid {
                        TelemetryState::Available
                    } else if values
                        .iter()
                        .any(|e| e.state == TelemetryState::Stale as i32)
                    {
                        TelemetryState::Stale
                    } else if values
                        .iter()
                        .any(|e| e.state == TelemetryState::Unsupported as i32)
                    {
                        TelemetryState::Unsupported
                    } else {
                        TelemetryState::Warmup
                    };
                    // A denied scan with no clients must not invent an idle value.
                    sample.engines.push(engine(
                        driver,
                        engine_id,
                        unified,
                        if sample.partial && sample.clients.is_empty() {
                            None
                        } else {
                            value
                        },
                        state,
                    ));
                }
            }
            sample.state = Some(if sample.partial {
                if sample.clients.is_empty()
                    && sample.observations.iter().any(|observation| {
                        observation.state == TelemetryState::PermissionDenied as i32
                    })
                {
                    TelemetryState::PermissionDenied
                } else {
                    TelemetryState::Partial
                }
            } else if sample
                .clients
                .iter()
                .any(|c| c.usage_state == TelemetryState::Stale as i32)
            {
                TelemetryState::Stale
            } else if sample
                .clients
                .iter()
                .any(|c| c.usage_state == TelemetryState::Warmup as i32)
            {
                TelemetryState::Warmup
            } else if sample.engines.is_empty() {
                TelemetryState::Unsupported
            } else {
                TelemetryState::Available
            });
        }
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "magpie-fdinfo-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn process(&self, pid: u32, start: u64, fds: &[&str]) {
            let path = self.0.join(pid.to_string());
            std::fs::create_dir_all(path.join("fdinfo")).unwrap();
            let mut fields = vec!["0".to_owned(); 20];
            fields[18] = start.to_string();
            std::fs::write(
                path.join("stat"),
                format!("{pid} (name (nested)) S {}", fields.join(" ")),
            )
            .unwrap();
            for (fd, text) in fds.iter().enumerate() {
                std::fs::write(path.join("fdinfo").join(fd.to_string()), text).unwrap();
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn devices() -> Vec<Device> {
        vec![Device {
            id: "0000:01:00.0".into(),
            driver: "amdgpu".into(),
            nodes: vec![],
            generation: 1,
            covered: true,
            sysfs_device: None,
        }]
    }
    fn fd(ns: u64) -> String {
        format!("drm-driver: amdgpu\ndrm-pdev: 0000:01:00.0\ndrm-client-id: 42\ndrm-engine-gfx: {ns} ns\ndrm-engine-capacity-gfx: 2\ndrm-memory-vram: 4 KiB\ndrm-total-gtt: 8 KiB\ndrm-shared-vram: 3 KiB\n")
    }
    #[test]
    fn deduplicates_across_fds_and_pids_and_uses_elapsed_capacity() {
        let root = Fixture::new();
        let mut collector = Collector::default();
        let first = fd(1_000_000_000);
        root.process(1, 10, &[&first, &first]);
        root.process(2, 20, &[&first]);
        let sample = collector.collect(&root.0, &devices(), Duration::from_secs(1));
        assert_eq!(
            sample[&devices()[0].id].clients[0].usage_state,
            TelemetryState::Warmup as i32
        );
        let second = fd(3_000_000_000);
        root.process(1, 10, &[&second, &second]);
        root.process(2, 20, &[&second]);
        let sample = collector.collect(&root.0, &devices(), Duration::from_secs(3));
        let sample = &sample[&devices()[0].id];
        assert_eq!(sample.clients.len(), 1);
        assert_eq!(sample.clients[0].owners.len(), 2);
        assert_eq!(sample.clients[0].engines[0].usage_percent, Some(50.));
        assert_eq!(sample.engines[0].usage_percent, Some(50.));
        assert_eq!(sample.clients[0].dedicated_memory_bytes, Some(4096));
        assert_eq!(sample.clients[0].shared_memory_bytes, None);
        assert_eq!(sample.clients[0].allocated_memory_bytes, Some(8192));
    }
    #[test]
    fn decreases_keep_high_counter_and_timestamp_then_pid_reuse_warms_up() {
        let root = Fixture::new();
        let mut collector = Collector::default();
        for (time, counter, expected) in [
            (1, 1_000_000_000, None),
            (2, 500_000_000, None),
            (3, 3_000_000_000, Some(50.)),
        ] {
            root.process(1, 10, &[&fd(counter)]);
            let sample = collector.collect(&root.0, &devices(), Duration::from_secs(time));
            assert_eq!(
                sample[&devices()[0].id].clients[0].engines[0].usage_percent,
                expected
            );
        }
        root.process(1, 99, &[&fd(4_000_000_000)]);
        let sample = collector.collect(&root.0, &devices(), Duration::from_secs(4));
        assert_eq!(
            sample[&devices()[0].id].clients[0].usage_state,
            TelemetryState::Warmup as i32
        );
        assert_eq!(
            sample[&devices()[0].id].clients[0].owners[0].start_time_ticks,
            99
        );
        collector.collect(&root.0, &[], Duration::from_secs(5));
        assert!(collector.baselines.is_empty());
    }
    #[test]
    fn cycle_domains_maxfreq_and_bad_units_are_not_guessed() {
        let a = Counter {
            cycles: Some(10),
            total_cycles: Some(100),
            ..Default::default()
        };
        let b = Counter {
            cycles: Some(60),
            total_cycles: Some(200),
            ..Default::default()
        };
        assert_eq!(usage(&a, &b, Duration::from_secs(17)).0, Some(50.));
        let a = Counter {
            cycles: Some(0),
            maxfreq: Some(100),
            ..Default::default()
        };
        let b = Counter {
            cycles: Some(100),
            ..a.clone()
        };
        assert_eq!(usage(&a, &b, Duration::from_secs(2)).0, Some(50.));
        assert!(parse("drm-engine-gfx: 4 bananas").invalid);
        assert!(parse("drm-client-id: 1\ndrm-client-id: 2").invalid);
        assert!(parse("drm-engine-capacity-gfx: 0").invalid);
        assert_eq!(engine_kind("i915", "video"), EngineKind::Video);
        assert_eq!(engine_kind("xe", "ccs"), EngineKind::Compute);
        assert_eq!(engine_kind("amdgpu", "unknown7"), EngineKind::Other);
    }
    #[test]
    fn missing_client_id_is_unsupported_and_successful_empty_scan_is_known_idle() {
        let root = Fixture::new();
        let mut collector = Collector::default();
        root.process(1, 10, &[&fd(0)]);
        collector.collect(&root.0, &devices(), Duration::from_secs(1));
        std::fs::remove_file(root.0.join("1/fdinfo/0")).unwrap();
        let sample = collector.collect(&root.0, &devices(), Duration::from_secs(2));
        assert_eq!(sample[&devices()[0].id].engines[0].usage_percent, Some(0.));
        assert_eq!(
            sample[&devices()[0].id].observations[0].state,
            TelemetryState::Available as i32
        );
        root.process(
            1,
            10,
            &["drm-driver: amdgpu\ndrm-pdev: 0000:01:00.0\ndrm-engine-gfx: 3 ns"],
        );
        let sample = collector.collect(&root.0, &devices(), Duration::from_secs(3));
        assert!(sample[&devices()[0].id].clients.is_empty());
        assert_eq!(
            sample[&devices()[0].id].observations[0].state,
            TelemetryState::Unsupported as i32
        );
        assert_eq!(sample[&devices()[0].id].engines[0].usage_percent, None);
        assert!(denied(&std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )));
    }
    #[test]
    fn allocated_memory_is_never_promoted_to_resident() {
        let parsed = parse("drm-driver: i915\ndrm-total-system: 2 MiB\ndrm-shared-system: 1 MiB\n");
        assert!(parsed.resident.is_empty());
        assert_eq!(parsed.allocated["system"], 2 * 1024 * 1024);
        assert_eq!(
            parse("drm-driver: amdgpu\ndrm-memory-vram: 1 KiB\ndrm-resident-vram: 2 KiB").resident
                ["vram"],
            2048
        );
        assert_eq!(
            sum_regions(&BTreeMap::from([("local0".into(), 4096)]), true),
            Some(4096)
        );
        assert_eq!(
            sum_regions(&BTreeMap::from([("system0".into(), 8192)]), false),
            Some(8192)
        );
    }

    #[test]
    fn access_denied_and_failed_scans_do_not_invent_idle() {
        use std::os::unix::fs::PermissionsExt;
        let root = Fixture::new();
        let mut collector = Collector::default();
        root.process(1, 10, &[&fd(0)]);
        collector.collect(&root.0, &devices(), Duration::from_secs(1));
        if unsafe { libc::geteuid() } != 0 {
            let path = root.0.join("1/fdinfo");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
            let sample = collector.collect(&root.0, &devices(), Duration::from_secs(2));
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let sample = &sample[&devices()[0].id];
            assert_eq!(
                sample.observations[0].state,
                TelemetryState::PermissionDenied as i32
            );
            assert_eq!(sample.state, Some(TelemetryState::PermissionDenied));
            assert_eq!(sample.engines[0].usage_percent, None);
        }
        let sample = collector.collect(
            &root.0.join("missing"),
            &devices(),
            Duration::from_secs(100),
        );
        assert_eq!(sample[&devices()[0].id].state, Some(TelemetryState::Stale));
        assert!(collector.baselines.is_empty());
    }

    #[test]
    fn changing_device_identity_warms_up_and_uncovered_devices_are_ignored() {
        let root = Fixture::new();
        let mut collector = Collector::default();
        root.process(1, 10, &[&fd(1_000_000_000)]);
        collector.collect(&root.0, &devices(), Duration::from_secs(1));
        let mut changed = devices();
        changed[0].generation = 2;
        root.process(1, 10, &[&fd(2_000_000_000)]);
        let sample = collector.collect(&root.0, &changed, Duration::from_secs(2));
        assert_eq!(
            sample[&changed[0].id].clients[0].usage_state,
            TelemetryState::Warmup as i32
        );
        changed.push(Device {
            id: "0000:02:00.0".into(),
            driver: "nvidia".into(),
            nodes: vec![],
            generation: 3,
            covered: false,
            sysfs_device: None,
        });
        root.process(
            2,
            20,
            &["drm-driver: nvidia\ndrm-pdev: 0000:02:00.0\ndrm-client-id: 7\n"],
        );
        let sample = collector.collect(&root.0, &changed, Duration::from_secs(3));
        assert_eq!(sample.len(), 1);
        assert!(!sample[&changed[0].id].partial);
        assert_eq!(
            process_identities(&root.0),
            HashMap::from([(1, 10), (2, 20)])
        );
    }

    #[test]
    fn ambiguous_identity_and_unaccounted_client_engines_are_not_zero() {
        let parsed = parse(&fd(0));
        let root = Fixture::new();
        let mut duplicates = devices();
        duplicates.push(devices().remove(0));
        assert!(device_for(&parsed, &root.0.join("absent"), &duplicates).is_none());
        assert!(device_for(
            &parse("drm-driver: amdgpu\ndrm-client-id: 9"),
            &root.0.join("absent"),
            &devices()
        )
        .is_none());
        let mut collector = Collector::default();
        root.process(1, 10, &[&fd(0)]);
        collector.collect(&root.0, &devices(), Duration::from_secs(1));
        root.process(
            1,
            10,
            &["drm-driver: amdgpu\ndrm-pdev: 0000:01:00.0\ndrm-client-id: 42\n"],
        );
        let sample = collector.collect(&root.0, &devices(), Duration::from_secs(2));
        assert_eq!(sample[&devices()[0].id].engines[0].usage_percent, None);
        assert_eq!(
            sample[&devices()[0].id].clients[0].usage_state,
            TelemetryState::Unsupported as i32
        );
    }

    fn ip_discovery(blocks: &[(&str, u32)]) -> Fixture {
        let sysfs = Fixture::new();
        for (block, major) in blocks {
            let dir = sysfs.0.join("ip_discovery/die/0").join(block).join("0");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("major"), format!("{major}\n")).unwrap();
        }
        sysfs
    }

    #[test]
    fn amdgpu_hardware_engines_come_from_ip_discovery() {
        let rdna3 = ip_discovery(&[("GC", 11), ("SDMA0", 6), ("UVD", 4), ("MP1", 13)]);
        assert_eq!(
            amdgpu_hw_engines(&rdna3.0),
            (
                vec!["compute".into(), "dma".into(), "enc".into(), "gfx".into()],
                Some(true)
            )
        );
        let rdna2 = ip_discovery(&[("GC", 10), ("SDMA1", 5), ("UVD", 3)]);
        assert_eq!(
            amdgpu_hw_engines(&rdna2.0).0,
            vec!["compute", "dec", "dma", "enc", "gfx"]
        );
        assert_eq!(amdgpu_hw_engines(&rdna2.0).1, Some(false));
        // UVD 7 + VCE predates VCN: separate decode and encode
        let vega = ip_discovery(&[("GC", 9), ("UVD", 7), ("VCE", 4)]);
        assert_eq!(amdgpu_hw_engines(&vega.0).1, Some(false));
        let none = Fixture::new();
        assert_eq!(amdgpu_hw_engines(&none.0), (vec![], None));

        assert_eq!(
            engine_label("amdgpu", "enc", true),
            (EngineKind::Video, "Video Encode/Decode".into())
        );
        assert_eq!(
            engine_label("amdgpu", "enc", false),
            (EngineKind::Encode, "Video Encode".into())
        );
        assert_eq!(
            engine_label("amdgpu", "dec", false),
            (EngineKind::Decode, "Video Decode".into())
        );
        assert_eq!(engine_label("amdgpu", "jpeg", true).0, EngineKind::Other);
        assert_eq!(
            engine_label("i915", "video", false),
            (EngineKind::Video, "video".into())
        );
    }

    #[test]
    fn unused_amdgpu_engines_read_zero_and_absent_keys_are_idle() {
        let sysfs = ip_discovery(&[("GC", 11), ("SDMA0", 6), ("UVD", 4)]);
        let mut devices = devices();
        devices[0].sysfs_device = Some(sysfs.0.clone());
        let client = |id: u32, gfx: u64, compute: Option<u64>| {
            let compute = compute
                .map(|ns| format!("drm-engine-compute: {ns} ns\n"))
                .unwrap_or_default();
            format!(
                "drm-driver: amdgpu\ndrm-pdev: 0000:01:00.0\ndrm-client-id: {id}\n\
                 drm-engine-gfx: {gfx} ns\n{compute}"
            )
        };
        let root = Fixture::new();
        let mut collector = Collector::default();
        root.process(1, 10, &[&client(42, 0, Some(0)), &client(43, 0, None)]);
        collector.collect(&root.0, &devices, Duration::from_secs(1));
        root.process(
            1,
            10,
            &[
                &client(42, 500_000_000, Some(250_000_000)),
                &client(43, 0, None),
            ],
        );
        let sample = collector.collect(&root.0, &devices, Duration::from_secs(2));
        let sample = &sample[&devices[0].id];
        let engine = |id: &str| {
            sample
                .engines
                .iter()
                .find(|engine| engine.id == id)
                .unwrap_or_else(|| panic!("{id} listed"))
        };
        assert_eq!(engine("gfx").name, "3D");
        assert_eq!(engine("gfx").usage_percent, Some(50.));
        // Client 43 never used compute: its absent key is idle, not partial
        assert_eq!(engine("compute").usage_percent, Some(25.));
        assert_eq!(engine("compute").state, TelemetryState::Available as i32);
        // Never used by any client, known from the hardware
        assert_eq!(engine("dma").name, "Copy");
        assert_eq!(engine("dma").usage_percent, Some(0.));
        assert_eq!(engine("dma").state, TelemetryState::Available as i32);
        assert_eq!(engine("enc").name, "Video Encode/Decode");
        assert_eq!(engine("enc").kind, EngineKind::Video as i32);
        assert_eq!(engine("enc").usage_percent, Some(0.));
        assert!(!sample.partial);
    }
}
