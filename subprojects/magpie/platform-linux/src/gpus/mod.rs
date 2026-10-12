/* src/gpus/mod.rs
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

use std::collections::HashMap;
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use magpie_platform::gpus::{
    ClientTelemetry, DriverInfo, EngineKind, EngineTelemetry, Gpu, GpuTelemetry, MemoryTelemetry,
    Process, ProcessIdentity, ProcessKind, ProcessObservation, TelemetryState,
};

use crate::gpu_info_valid;

use drm::DrmDevice;
use nvtop::{
    GpuInfoDynamicInfoValid, GpuInfoProcessInfoValid, GpuInfoStaticInfoValid, GpuProcessType,
};
use run_forked::run_forked;
use vulkan_info::VulkanDevice;

mod drm;
mod fdinfo;
mod gl_info;
mod nvml;
#[allow(unused)]
mod nvtop;
mod run_forked;
mod util;
mod vulkan_info;

#[derive(Clone)]
struct GpuDevice {
    id: String,
    sysfs_device: Option<PathBuf>,
    is_pci: bool,
    drm: Option<DrmDevice>,
}

impl GpuDevice {
    /// Nodes to try when probing OpenGL support, least privileged first.
    fn render_nodes(&self) -> Vec<PathBuf> {
        match &self.drm {
            // Driver ioctls on the card node require DRM authentication, the render node does not
            Some(drm) => [drm.render_node.clone(), Some(drm.card_node.clone())]
                .into_iter()
                .flatten()
                .collect(),
            // No DRM device was resolved, so fall back to the udev by-path symlink
            None if self.is_pci => vec![PathBuf::from(format!(
                "/dev/dri/by-path/pci-{}-card",
                self.id
            ))],
            None => vec![],
        }
    }

    fn is_vulkan_device(&self, vulkan: &VulkanDevice) -> bool {
        let Some(drm) = self.drm.as_ref() else {
            return false;
        };

        vulkan.primary_node.is_some_and(|node| node == drm.card_dev)
            || (vulkan.render_node.is_some() && vulkan.render_node == drm.render_dev)
    }
}

fn match_device(drm_devices: &mut [DrmDevice], pdev: &str, backend: &str) -> Option<usize> {
    if !pdev.is_empty() {
        let pdev = canonical_pci_address(pdev).unwrap_or_else(|| pdev.to_ascii_lowercase());
        return drm_devices
            .iter()
            .position(|d| d.sysname.eq_ignore_ascii_case(&pdev));
    }

    // nvtop identifies SoC GPUs by the bus driver name (v3d) or by the DRM driver name (msm,
    // panfrost, panthor), so try the free comparison before opening any device node
    if let Some(index) = drm_devices.iter().position(|d| d.driver == backend) {
        return Some(index);
    }

    (0..drm_devices.len()).find(|&i| drm_devices[i].drm_driver() == Some(backend))
}

fn is_pci_address(address: &str) -> bool {
    fn hex_digits(part: &str, lengths: std::ops::RangeInclusive<usize>) -> bool {
        lengths.contains(&part.len()) && part.bytes().all(|b| b.is_ascii_hexdigit())
    }

    let Some((domain, rest)) = address.split_once(':') else {
        return false;
    };
    let Some((bus, rest)) = rest.split_once(':') else {
        return false;
    };
    let Some((device, function)) = rest.split_once('.') else {
        return false;
    };

    // `dddd:bb:dd.f`
    hex_digits(domain, 4..=8)
        && hex_digits(bus, 2..=2)
        && hex_digits(device, 2..=2)
        && hex_digits(function, 1..=1)
}

fn canonical_pci_address(address: &str) -> Option<String> {
    if !is_pci_address(address) {
        return None;
    }
    let (domain, rest) = address.split_once(':')?;
    let domain = u32::from_str_radix(domain, 16).ok()?;
    Some(format!("{domain:04x}:{}", rest.to_ascii_lowercase()))
}

fn resolve_devices(gpu_list: &mut nvtop::ListHead) -> HashMap<usize, GpuDevice> {
    let mut drm_devices = drm::enumerate();
    let mut resolved = HashMap::new();
    let mut unidentified = 0;

    let mut device = gpu_list.next;
    while device != gpu_list {
        let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
        let entry = device as usize;
        device = unsafe { (*device).next };

        let pdev = c_str(&dev.pdev);
        let backend = vendor_name(dev);

        let gpu_device = match match_device(&mut drm_devices, pdev.as_ref(), backend)
            .map(|i| drm_devices.remove(i))
        {
            Some(drm) => GpuDevice {
                id: drm.sysname.clone(),
                sysfs_device: Some(drm.sysfs_device.clone()),
                is_pci: drm.is_pci(),
                drm: Some(drm),
            },
            // A PCI GPU with no DRM card, an NVIDIA card without `nvidia-drm` loaded
            // for instance
            None if is_pci_address(&pdev) => {
                let pdev = canonical_pci_address(&pdev).unwrap();
                GpuDevice {
                    sysfs_device: Some(PathBuf::from(format!("/sys/bus/pci/devices/{pdev}"))),
                    id: pdev,
                    is_pci: true,
                    drm: None,
                }
            }
            None => {
                unidentified += 1;
                log::warn!(
                    "No DRM device matches the `{backend}` GPU reported by nvtop, pdev `{pdev}`"
                );
                GpuDevice {
                    id: format!("{backend}-{unidentified}"),
                    sysfs_device: None,
                    is_pci: false,
                    drm: None,
                }
            }
        };

        log::debug!(
            "GPU `{}`: nvtop backend `{}`, driver `{}`, bus `{}`, node `{}`",
            gpu_device.id,
            backend,
            gpu_device.drm.as_ref().map_or("", |d| d.driver.as_str()),
            gpu_device
                .drm
                .as_ref()
                .map_or("none".into(), |d| d.bus.to_string()),
            gpu_device
                .drm
                .as_ref()
                .map_or_else(|| "none".into(), |d| d.card_node.display().to_string()),
        );

        resolved.insert(entry, gpu_device);
    }

    resolved
}

fn c_str(field: &[std::ffi::c_char]) -> std::borrow::Cow<'_, str> {
    let bytes = unsafe { &*(field as *const [std::ffi::c_char] as *const [u8]) };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());

    String::from_utf8_lossy(&bytes[..end])
}

fn vendor_name(dev: &nvtop::GpuInfo) -> &str {
    if dev.vendor.is_null() || unsafe { (*dev.vendor).name.is_null() } {
        return "";
    }

    unsafe { std::ffi::CStr::from_ptr((*dev.vendor).name as *const std::ffi::c_char) }
        .to_str()
        .unwrap_or_default()
}

fn collection_epoch() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ ((std::process::id() as u64) << 32)
        ^ serial
}

fn union_drm_devices(inventory: &mut HashMap<String, GpuDevice>) {
    for drm in drm::enumerate() {
        inventory
            .entry(drm.sysname.clone())
            .or_insert_with(|| GpuDevice {
                id: drm.sysname.clone(),
                sysfs_device: Some(drm.sysfs_device.clone()),
                is_pci: drm.is_pci(),
                drm: Some(drm),
            });
    }
}

/// NVIDIA need not expose a DRM card (nvidia-drm may be absent). Sysfs PCI
/// inventory also works when NVTOP/NVML saw no GPU during startup.
fn union_nvidia_pci_devices(inventory: &mut HashMap<String, GpuDevice>, pci_root: &Path) {
    let Ok(entries) = std::fs::read_dir(pci_root) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(id) = entry.file_name().to_str().and_then(canonical_pci_address) else {
            continue;
        };
        let hex_attribute = |name: &str| {
            let value = std::fs::read_to_string(entry.path().join(name)).ok()?;
            u32::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok()
        };
        if hex_attribute("vendor") != Some(0x10de)
            || hex_attribute("class").is_none_or(|class| class >> 16 != 0x03)
        {
            continue;
        }
        inventory.entry(id.clone()).or_insert_with(|| GpuDevice {
            id,
            sysfs_device: Some(entry.path()),
            is_pci: true,
            drm: None,
        });
    }
}

fn inventory_gpu(device: &GpuDevice) -> Gpu {
    let (vendor_id, device_id) = device
        .sysfs_device
        .as_deref()
        .and_then(|path| util::ven_dev_ids(path, &mut String::new()))
        .unwrap_or_default();
    Gpu {
        id: device.id.clone(),
        vendor_id,
        device_id,
        ..Default::default()
    }
}

fn fdinfo_device(device: &GpuDevice) -> fdinfo::Device {
    let drm = device.drm.as_ref().unwrap();
    fdinfo::Device {
        id: device.id.clone(),
        // A platform bus driver (for example `mali`) need not be the DRM
        // driver's name. Unknown bus names are not an fdinfo identity check.
        driver: drm
            .drm_driver
            .as_ref()
            .filter(|driver| !driver.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if matches!(
                    drm.driver.as_str(),
                    "amdgpu" | "i915" | "xe" | "nvidia" | "v3d" | "msm" | "panfrost" | "panthor"
                ) {
                    drm.driver.clone()
                } else {
                    String::new()
                }
            }),
        nodes: std::iter::once(drm.card_dev)
            .chain(drm.render_dev)
            .collect(),
        generation: device_token(device).0,
        covered: drm.driver != "nvidia",
        sysfs_device: device.sysfs_device.clone(),
    }
}

fn device_token(device: &GpuDevice) -> (u64, Option<PathBuf>) {
    use std::os::unix::fs::MetadataExt;
    let Some(path) = device.sysfs_device.as_ref() else {
        return (0, None);
    };
    (
        std::fs::metadata(path)
            .map(|metadata| metadata.ino())
            .unwrap_or(0),
        std::fs::canonicalize(path.join("driver")).ok(),
    )
}

fn retire_native_devices(
    devices: &mut HashMap<usize, GpuDevice>,
    tokens: &mut HashMap<usize, (u64, Option<PathBuf>)>,
) {
    tokens.retain(|node, token| {
        devices.get(node).is_some_and(|device| {
            device
                .sysfs_device
                .as_ref()
                .is_none_or(|path| path.exists())
                && *token == device_token(device)
        })
    });
    devices.retain(|node, _| tokens.contains_key(node));
}

fn driver_info(device: &GpuDevice) -> DriverInfo {
    let kernel_driver = device
        .drm
        .as_ref()
        .map(|drm| drm.driver.clone())
        .filter(|driver| !driver.is_empty())
        .or_else(|| {
            device
                .sysfs_device
                .as_ref()
                .and_then(|path| std::fs::canonicalize(path.join("driver")).ok())
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
        });
    let version = kernel_driver.as_ref().and_then(|driver| {
        // driver came from a sysfs basename, not arbitrary fdinfo contents.
        let value =
            std::fs::read_to_string(Path::new("/sys/module").join(driver).join("version")).ok()?;
        (!value.trim().is_empty()).then(|| value.trim().to_owned())
    });
    let mut info = device
        .drm
        .as_ref()
        .and_then(drm::driver_metadata)
        .unwrap_or_default();
    info.kernel_driver = kernel_driver;
    info.version = version.or(info.version);
    info
}

fn memory_info(device: &GpuDevice, gpu: &Gpu, driver: &DriverInfo) -> MemoryTelemetry {
    let attribute = |name: &str| {
        device
            .sysfs_device
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path.join(name)).ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    // Several native UMA backends export host MemTotal as GPU memory. Intel's
    // query also chooses the system region on integrated devices. Only known
    // framebuffer/local-memory sources may be a dedicated-memory fallback.
    let native_local = match driver.kernel_driver.as_deref() {
        Some("amdgpu") => true,
        Some("i915" | "xe") => driver.integrated == Some(false),
        _ => false,
    };
    let dedicated_total = attribute("mem_info_vram_total")
        .or_else(|| native_local.then_some(gpu.total_memory).flatten());
    let dedicated_used = attribute("mem_info_vram_used")
        .or_else(|| native_local.then_some(gpu.used_memory).flatten());
    let shared_total = attribute("mem_info_gtt_total");
    let shared_used = attribute("mem_info_gtt_used");
    let status = |total: Option<u64>, used: Option<u64>| {
        (if total.is_some() && used.is_some() {
            TelemetryState::Available
        } else if total.is_some() || used.is_some() {
            TelemetryState::Partial
        } else {
            TelemetryState::Unsupported
        }) as i32
    };
    MemoryTelemetry {
        dedicated_total_bytes: dedicated_total,
        dedicated_used_bytes: dedicated_used,
        shared_total_bytes: shared_total,
        shared_used_bytes: shared_used,
        dedicated_state: status(dedicated_total, dedicated_used),
        shared_state: status(shared_total, shared_used),
        ..Default::default()
    }
}

fn optional_max(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn kind_usage(engines: &[EngineTelemetry], kind: EngineKind) -> Option<f32> {
    engines
        .iter()
        .filter(|engine| engine.kind == kind as i32)
        .filter_map(|engine| engine.usage_percent)
        .reduce(f32::max)
}

fn native_engine(id: &str, kind: EngineKind, usage: Option<f32>) -> EngineTelemetry {
    EngineTelemetry {
        id: id.into(),
        name: id.into(),
        kind: kind as i32,
        usage_percent: usage,
        state: if usage.is_some() {
            TelemetryState::Available
        } else {
            TelemetryState::Unsupported
        } as i32,
        client_derived: Some(false),
    }
}

fn native_engines(gpu: &Gpu) -> Vec<EngineTelemetry> {
    // Device-wide/SM total is not an engine and cannot compete in max(engine).
    let mut engines = vec![];
    if gpu.encode_decode_shared {
        engines.push(native_engine(
            "video",
            EngineKind::Video,
            optional_max(gpu.encoder_percent, gpu.decoder_percent),
        ));
    } else {
        engines.push(native_engine(
            "encode",
            EngineKind::Encode,
            gpu.encoder_percent,
        ));
        engines.push(native_engine(
            "decode",
            EngineKind::Decode,
            gpu.decoder_percent,
        ));
    }
    engines
}

fn native_telemetry_state(gpu: &Gpu, fallback: TelemetryState) -> TelemetryState {
    if gpu.utilization_percent.is_some()
        || gpu.encoder_percent.is_some()
        || gpu.decoder_percent.is_some()
        || gpu.total_memory.is_some()
        || gpu.used_memory.is_some()
    {
        // NVIDIA's total/SM, encode and decode never supply 3D/compute/copy.
        TelemetryState::Partial
    } else {
        fallback
    }
}

fn native_clients(
    gpu: &mut Gpu,
    telemetry: &mut GpuTelemetry,
    processes_ok: bool,
    identities: &HashMap<u32, u64>,
) {
    if !processes_ok {
        return;
    }
    for (pid, process) in std::mem::take(&mut gpu.processes) {
        let identity = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|text| fdinfo::starttime_from_stat(&text));
        let Some(start_time_ticks) = identity.filter(|start| identities.get(&pid) == Some(start))
        else {
            telemetry.observations.push(ProcessObservation {
                pid,
                start_time_ticks: identity,
                state: TelemetryState::Stale as i32,
            });
            continue;
        };
        gpu.processes.insert(pid, process);
        let mut engines = vec![];
        // NVML's graphical/compute process-list membership is not engine usage.
        if process.encode_usage_percent.is_some() {
            engines.push(native_engine(
                "encode",
                EngineKind::Encode,
                process.encode_usage_percent,
            ));
        }
        if process.decode_usage_percent.is_some() {
            engines.push(native_engine(
                "decode",
                EngineKind::Decode,
                process.decode_usage_percent,
            ));
        }
        let usage_state = if process.gpu_usage_percent.is_some() {
            TelemetryState::Available
        } else if !engines.is_empty() {
            TelemetryState::Partial
        } else {
            TelemetryState::Unsupported
        };
        telemetry.clients.push(ClientTelemetry {
            id: format!("nvml:{pid}:{start_time_ticks}"),
            owners: vec![ProcessIdentity {
                pid,
                start_time_ticks,
            }],
            engines,
            overall_usage_percent: process.gpu_usage_percent,
            dedicated_memory_bytes: process.memory_usage_bytes,
            usage_state: usage_state as i32,
            memory_state: if process.memory_usage_bytes.is_some() {
                TelemetryState::Available
            } else {
                TelemetryState::Unsupported
            } as i32,
            memory_is_resident: process.memory_usage_bytes.map(|_| true),
            ..Default::default()
        });
        telemetry.observations.push(ProcessObservation {
            pid,
            start_time_ticks: Some(start_time_ticks),
            state: usage_state as i32,
        });
    }
    telemetry.clients.sort_by(|a, b| a.id.cmp(&b.id));
    telemetry
        .observations
        .sort_by_key(|observation| observation.pid);
}

/// Temporarily omit removed native devices while preserving C's allocation and
/// callback ownership. All pointer traversal and restoration is under GPU_LIST.
fn with_live_native<T>(
    list: &mut nvtop::ListHead,
    devices: &HashMap<usize, GpuDevice>,
    inventory: &HashMap<String, GpuDevice>,
    collect: impl FnOnce(&mut nvtop::ListHead) -> T,
) -> T {
    struct Restore {
        links: Vec<(*mut nvtop::ListHead, nvtop::ListHead)>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            for (node, value) in &self.links {
                unsafe {
                    **node = *value;
                }
            }
        }
    }
    let head: *mut nvtop::ListHead = list;
    let mut restore = Restore {
        links: vec![(head, *list)],
    };
    let mut node = list.next;
    while node != head {
        let current = unsafe { *node };
        restore.links.push((node, current));
        node = current.next;
    }
    let live: Vec<_> = restore
        .links
        .iter()
        .skip(1)
        .filter(|(node, _)| {
            devices
                .get(&(*node as usize))
                .is_some_and(|device| inventory.contains_key(&device.id))
        })
        .map(|(node, _)| *node)
        .collect();
    let mut previous = head;
    for node in live {
        unsafe {
            (*previous).next = node;
            (*node).prev = previous;
        }
        previous = node;
    }
    unsafe {
        (*previous).next = head;
        (*head).prev = previous;
    }
    collect(list)
}

fn clear_dynamic(gpu: &mut Gpu) {
    let static_gpu = Gpu {
        id: gpu.id.clone(),
        device_name: gpu.device_name.clone(),
        vendor_id: gpu.vendor_id,
        device_id: gpu.device_id,
        opengl_variant: gpu.opengl_variant,
        opengl_version: gpu.opengl_version,
        vulkan_version: gpu.vulkan_version,
        opencl_version: gpu.opencl_version,
        cuda_version: gpu.cuda_version,
        encode_decode_shared: gpu.encode_decode_shared,
        telemetry: gpu.telemetry.take(),
        ..Default::default()
    };
    *gpu = static_gpu;
}

fn apply_nvml_sensors(gpu: &mut Gpu, sensors: &nvml::Sensors) {
    // Native fields were validity-gated in this collection. Supplement absent
    // fields without carrying measurements across inventory/driver generations.
    gpu.temperature_c = gpu.temperature_c.or(sensors.temperature_c);
    gpu.power_draw_watts = gpu.power_draw_watts.or(sensors.power_draw_watts);
    gpu.max_power_draw_watts = gpu.max_power_draw_watts.or(sensors.max_power_draw_watts);
    gpu.clock_speed_mhz = gpu.clock_speed_mhz.or(sensors.clock_speed_mhz);
    gpu.memory_speed_mhz = gpu.memory_speed_mhz.or(sensors.memory_speed_mhz);
    gpu.max_clock_speed_mhz = gpu.max_clock_speed_mhz.or(sensors.max_clock_speed_mhz);
    gpu.max_memory_speed_mhz = gpu.max_memory_speed_mhz.or(sensors.max_memory_speed_mhz);
    gpu.fan_speed_percent = gpu.fan_speed_percent.or(sensors.fan_speed_percent);
    gpu.pcie_gen = gpu.pcie_gen.or(sensors.pcie_gen);
    gpu.pcie_lanes = gpu.pcie_lanes.or(sensors.pcie_lanes);
    gpu.max_pcie_gen = gpu.max_pcie_gen.or(sensors.max_pcie_gen);
    gpu.max_pcie_lanes = gpu.max_pcie_lanes.or(sensors.max_pcie_lanes);
    if gpu
        .pcie_gen
        .zip(gpu.max_pcie_gen)
        .is_some_and(|(current, max)| current > max)
    {
        gpu.max_pcie_gen = None;
    }
    if gpu
        .pcie_lanes
        .zip(gpu.max_pcie_lanes)
        .is_some_and(|(current, max)| current > max)
    {
        gpu.max_pcie_lanes = None;
    }
}

pub struct GpuCache {
    gpu_list: Arc<RwLock<nvtop::ListHead>>,
    devices: HashMap<usize, GpuDevice>,
    native_tokens: HashMap<usize, (u64, Option<PathBuf>)>,
    inventory_tokens: HashMap<String, (u64, Option<PathBuf>)>,
    gpus: HashMap<String, Gpu>,
    metadata: HashMap<String, DriverInfo>,
    fdinfo: fdinfo::Collector,
    nvml: Option<nvml::Nvml>,
    nvml_cursors: HashMap<String, nvml::ProcessCursor>,
    started: Instant,
    epoch: u64,
    sequence: u64,
}

impl magpie_platform::gpus::GpuCache for GpuCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        static GPU_LIST: OnceLock<(usize, Arc<RwLock<nvtop::ListHead>>)> = OnceLock::new();

        let (gpu_count, gpu_list) = GPU_LIST
            .get_or_init(|| {
                unsafe {
                    nvtop::init_extract_gpuinfo_intel();
                    nvtop::init_extract_gpuinfo_amdgpu();
                    nvtop::init_extract_gpuinfo_nvidia();
                    nvtop::init_extract_gpuinfo_v3d();
                    nvtop::init_extract_gpuinfo_msm();
                    nvtop::init_extract_gpuinfo_panfrost();
                    nvtop::init_extract_gpuinfo_panthor();
                }

                let gpu_list = Arc::new(RwLock::new(nvtop::ListHead {
                    next: std::ptr::null_mut(),
                    prev: std::ptr::null_mut(),
                }));
                {
                    let mut gl = gpu_list.write().unwrap();
                    gl.next = gl.deref_mut();
                    gl.prev = gl.deref_mut();
                }

                let gpu_count = {
                    let mut gpu_list = gpu_list.write().unwrap();
                    let gpu_list = gpu_list.deref_mut();

                    let mut gpu_count: u32 = 0;
                    let nvt_result =
                        unsafe { nvtop::gpuinfo_init_info_extraction(&mut gpu_count, gpu_list) };
                    if nvt_result == 0 {
                        log::error!("Unable to initialize GPU info extraction");
                        gpu_count = 0;
                    }

                    let nvt_result = unsafe { nvtop::gpuinfo_populate_static_infos(gpu_list) };
                    if nvt_result == 0 {
                        log::error!("Unable to populate static GPU info");
                    }

                    gpu_count
                };

                (gpu_count as usize, gpu_list)
            })
            .clone();

        let mut gpus = HashMap::with_capacity(gpu_count);
        let mut buffer = String::with_capacity(1024);

        let devices = {
            let mut gpu_list = gpu_list.write().unwrap();
            let gpu_list = gpu_list.deref_mut();

            let devices = resolve_devices(gpu_list);

            let mut device = gpu_list.next;
            while device != gpu_list {
                let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
                let entry = device as usize;
                device = unsafe { (*device).next };

                let Some(gpu_device) = devices.get(&entry) else {
                    continue;
                };

                let (vendor_id, device_id) = gpu_device
                    .sysfs_device
                    .as_deref()
                    .and_then(|device| util::ven_dev_ids(device, &mut buffer))
                    .unwrap_or_else(|| {
                        if gpu_device.is_pci {
                            log::warn!(
                                "Unable to read the vendor and device IDs of GPU `{}`",
                                gpu_device.id
                            );
                        }
                        (0, 0)
                    });

                let device_name =
                    if gpu_info_valid!(dev.static_info, GpuInfoStaticInfoValid::DeviceNameValid) {
                        Some(c_str(&dev.static_info.device_name).into_owned())
                    } else {
                        None
                    };

                let gpu = Gpu {
                    id: gpu_device.id.clone(),
                    vendor_id,
                    device_id,
                    device_name,
                    encode_decode_shared: dev.static_info.encode_decode_shared,
                    ..Default::default()
                };
                gpus.insert(gpu_device.id.clone(), gpu);
            }

            devices
        };

        let mut inventory = devices
            .values()
            .cloned()
            .map(|device| (device.id.clone(), device))
            .collect::<HashMap<_, _>>();
        union_drm_devices(&mut inventory);
        union_nvidia_pci_devices(&mut inventory, Path::new("/sys/bus/pci/devices"));
        let mut metadata = inventory
            .values()
            .map(|device| (device.id.clone(), driver_info(device)))
            .collect::<HashMap<_, _>>();
        for device in inventory.values() {
            gpus.entry(device.id.clone())
                .or_insert_with(|| inventory_gpu(device));
        }

        let vulkan_devices = unsafe {
            run_forked(|| {
                Ok(vulkan_info::VulkanInfo::new()
                    .map(|vulkan_info| vulkan_info.devices())
                    .unwrap_or_default())
            })
        };
        let vulkan_devices = vulkan_devices.unwrap_or_else(|e| {
            log::warn!("Failed to get Vulkan information: {}", e);
            vec![]
        });

        let mut ids_counts = HashMap::<u32, usize>::new();
        for gpu in gpus.values() {
            *ids_counts
                .entry((gpu.vendor_id << 16) | gpu.device_id)
                .or_default() += 1;
        }
        for gpu_device in inventory.values() {
            let Some(gpu) = gpus.get_mut(&gpu_device.id) else {
                continue;
            };

            let nodes = gpu_device.render_nodes();
            let nodes = nodes.iter().map(PathBuf::as_path).collect::<Vec<_>>();
            if let Some((variant, version)) =
                unsafe { run_forked(|| Ok(gl_info::supported_opengl_version(&nodes))) }
                    .unwrap_or_else(|e| {
                        log::warn!("Failed to get OpenGL information: {}", e);
                        None
                    })
            {
                gpu.opengl_variant = Some(variant as i32);
                gpu.opengl_version = Some(version);
            }

            let ids_key = (gpu.vendor_id << 16) | (gpu.device_id & 0x0000FFFF);
            let vulkan = vulkan_devices
                .iter()
                .find(|vulkan| gpu_device.is_vulkan_device(vulkan))
                .or_else(|| {
                    let matches = vulkan_devices
                        .iter()
                        .filter(|vulkan| ids_key != 0 && vulkan.ids_key == ids_key)
                        .collect::<Vec<_>>();
                    let same_ids = ids_counts.get(&ids_key).copied().unwrap_or_default();
                    (matches.len() == 1 && same_ids == 1).then(|| matches[0])
                });
            if let Some(vulkan) = vulkan {
                gpu.vulkan_version = Some(vulkan.version);
                if gpu.device_name.is_none() && !vulkan.name.is_empty() {
                    gpu.device_name = Some(vulkan.name.clone());
                }
                if let Some(driver) = metadata.get_mut(&gpu_device.id) {
                    driver.vulkan_driver = vulkan.driver.clone();
                    driver.integrated = vulkan.integrated;
                }
            }
        }

        let native_tokens = devices
            .iter()
            .map(|(node, device)| (*node, device_token(device)))
            .collect();
        let inventory_tokens = inventory
            .values()
            .map(|device| (device.id.clone(), device_token(device)))
            .collect();
        Self {
            gpu_list,
            devices,
            native_tokens,
            inventory_tokens,
            gpus,
            metadata,
            fdinfo: fdinfo::Collector::default(),
            nvml: nvml::Nvml::new(),
            nvml_cursors: HashMap::new(),
            started: Instant::now(),
            epoch: collection_epoch(),
            sequence: 0,
        }
    }

    fn refresh(&mut self) {
        let mut gpu_list = self.gpu_list.write().unwrap();
        let gpu_list = gpu_list.deref_mut();

        // The pinned C backend owns its device allocations and callback registry
        // for the process lifetime. Reinitialization would leave stale callbacks.
        // Reconcile live DRM/PCI inventory and supplement NVIDIA through fresh
        // NVML handles instead, without freeing those C allocations.
        self.devices = resolve_devices(gpu_list);
        // Permanently retire a native allocation once its identity disappeared
        // or changed, even if the same PCI address later returns. Never reuse an
        // old NVML/DRM handle or its C-side process-utilization baseline.
        retire_native_devices(&mut self.devices, &mut self.native_tokens);
        let mut inventory = self
            .devices
            .values()
            .filter(|device| {
                device
                    .sysfs_device
                    .as_ref()
                    .is_none_or(|path| path.exists())
            })
            .cloned()
            .map(|device| (device.id.clone(), device))
            .collect::<HashMap<_, _>>();
        union_drm_devices(&mut inventory);
        union_nvidia_pci_devices(&mut inventory, Path::new("/sys/bus/pci/devices"));
        self.gpus.retain(|id, _| inventory.contains_key(id));
        self.metadata.retain(|id, _| inventory.contains_key(id));
        self.nvml_cursors.retain(|id, _| inventory.contains_key(id));
        self.inventory_tokens
            .retain(|id, _| inventory.contains_key(id));
        for device in inventory.values() {
            let token = device_token(device);
            if self.inventory_tokens.get(&device.id) != Some(&token) {
                self.gpus.remove(&device.id);
                self.metadata.remove(&device.id);
                self.nvml_cursors.remove(&device.id);
                self.inventory_tokens.insert(device.id.clone(), token);
            }
            self.gpus
                .entry(device.id.clone())
                .or_insert_with(|| inventory_gpu(device));
            self.metadata
                .entry(device.id.clone())
                .or_insert_with(|| driver_info(device));
        }
        for gpu in self.gpus.values_mut() {
            clear_dynamic(gpu);
        }
        let fd_devices: Vec<_> = inventory
            .values()
            .filter(|device| device.drm.is_some())
            .map(fdinfo_device)
            .collect();
        let covered_ids: std::collections::HashSet<_> = fd_devices
            .iter()
            .filter(|device| device.covered)
            .map(|device| device.id.as_str())
            .collect();
        let mut node = gpu_list.next;
        while node != gpu_list {
            let dev = unsafe { &*(node as *const nvtop::GpuInfo) };
            let entry = node as usize;
            node = unsafe { (*node).next };
            let enable = self.devices.get(&entry).is_some_and(|device| {
                !covered_ids.contains(device.id.as_str()) && inventory.contains_key(&device.id)
            });
            unsafe {
                nvtop::processinfo_enable_disable_callback_for(dev, enable);
            }
        }

        self.sequence = self.sequence.saturating_add(1);
        let sample_start = Instant::now();
        let mut fd_samples =
            self.fdinfo
                .collect(Path::new("/proc"), &fd_devices, self.started.elapsed());
        let native_identities = fdinfo::process_identities(Path::new("/proc"));
        // A machine with no GPU/driver at startup may acquire one later. Retry
        // the optional library on the existing collection path, without workers.
        if self.nvml.is_none() && self.gpus.values().any(|gpu| gpu.vendor_id == 0x10de) {
            self.nvml = nvml::Nvml::new();
        }

        let (dynamic_ok, processes_ok) =
            with_live_native(gpu_list, &self.devices, &inventory, |list| unsafe {
                // Reset flags before collection: failed queries may not retain an
                // earlier successful sample as if it had just been collected.
                let mut node = list.next;
                while node != list {
                    let dev = &mut *(node as *mut nvtop::GpuInfo);
                    dev.dynamic_info.valid.fill(0);
                    for i in 0..dev.processes_count as usize {
                        (*dev.processes.add(i)).valid.fill(0);
                    }
                    node = (*node).next;
                }
                let dynamic_ok = nvtop::gpuinfo_refresh_dynamic_info(list) != 0;
                let processes_ok = nvtop::gpuinfo_refresh_processes(list) != 0;
                nvtop::gpuinfo_utilisation_rate(list);
                // Do not run gpuinfo_fix_dynamic_info_from_process_info: it turns
                // partial process sums into apparently device-wide measurements.
                (dynamic_ok, processes_ok)
            });

        let mut buffer = String::with_capacity(1024);

        let mut device: *mut nvtop::ListHead = gpu_list.next;
        while device != gpu_list {
            let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
            let entry = device as usize;
            device = unsafe { (*device).next };

            let Some(gpu_device) = self.devices.get(&entry) else {
                continue;
            };
            if !inventory.contains_key(&gpu_device.id) {
                continue;
            }
            let sysfs_device: Option<&Path> = gpu_device.sysfs_device.as_deref();

            let gpu = match self.gpus.get_mut(gpu_device.id.as_str()) {
                Some(gpu) => gpu,
                None => {
                    log::warn!("Unable to find gpu `{}` in cache", gpu_device.id);
                    continue;
                }
            };

            gpu.temperature_c =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::GpuTempValid) {
                    Some(dev.dynamic_info.gpu_temp as f32)
                } else {
                    None
                };
            gpu.fan_speed_percent =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::FanSpeedValid) {
                    Some(dev.dynamic_info.fan_speed as f32)
                } else {
                    None
                };
            gpu.utilization_percent =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::GpuUtilRateValid) {
                    Some(dev.dynamic_info.gpu_util_rate as f32)
                } else {
                    None
                };
            gpu.power_draw_watts =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::PowerDrawValid) {
                    Some(dev.dynamic_info.power_draw as f32 / 1000.)
                } else {
                    None
                };
            gpu.max_power_draw_watts =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::PowerDrawMaxValid) {
                    Some(dev.dynamic_info.power_draw_max as f32 / 1000.)
                } else {
                    None
                };
            gpu.clock_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::GpuClockSpeedValid
            ) {
                Some(dev.dynamic_info.gpu_clock_speed)
            } else {
                None
            };
            gpu.max_clock_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::GpuClockSpeedMaxValid
            ) {
                Some(dev.dynamic_info.gpu_clock_speed_max)
            } else {
                None
            };
            gpu.memory_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::MemClockSpeedValid
            ) {
                Some(dev.dynamic_info.mem_clock_speed)
            } else {
                None
            };
            gpu.max_memory_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::MemClockSpeedMaxValid
            ) {
                Some(dev.dynamic_info.mem_clock_speed_max)
            } else {
                None
            };
            gpu.total_memory =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::TotalMemoryValid) {
                    Some(dev.dynamic_info.total_memory)
                } else {
                    None
                };
            gpu.used_memory =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::UsedMemoryValid) {
                    Some(dev.dynamic_info.used_memory)
                } else {
                    None
                };
            gpu.total_shared_memory =
                sysfs_device.and_then(|device| util::shared_mem_total(device, &mut buffer));
            gpu.used_shared_memory =
                sysfs_device.and_then(|device| util::shared_mem_used(device, &mut buffer));
            gpu.encoder_percent =
                gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::EncoderRateValid)
                    .then_some(dev.dynamic_info.encoder_rate as f32);
            gpu.decoder_percent =
                gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::DecoderRateValid)
                    .then_some(dev.dynamic_info.decoder_rate as f32);
            gpu.pcie_gen =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::PcieLinkGenValid) {
                    Some(dev.dynamic_info.pcie_link_gen)
                } else {
                    sysfs_device.and_then(|device| util::pcie_speed(device, &mut buffer))
                };

            gpu.pcie_lanes = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::PcieLinkWidthValid
            ) {
                Some(dev.dynamic_info.pcie_link_width)
            } else {
                sysfs_device.and_then(|device| util::pcie_width(device, &mut buffer))
            };

            gpu.max_pcie_gen =
                sysfs_device.and_then(|device| util::max_pcie_speed(device, &mut buffer));
            gpu.max_pcie_lanes =
                sysfs_device.and_then(|device| util::max_pcie_width(device, &mut buffer));

            if let Some(max_pcie_gen) = gpu.max_pcie_gen {
                if let Some(pcie_gen) = gpu.pcie_gen {
                    if let Some(max_pcie_lanes) = gpu.max_pcie_lanes {
                        if let Some(pcie_lanes) = gpu.pcie_lanes {
                            if max_pcie_gen < pcie_gen || max_pcie_lanes < pcie_lanes {
                                gpu.max_pcie_gen = None;
                                gpu.max_pcie_lanes = None;
                            }
                        }
                    }
                }
            }

            gpu.processes.clear();
            if !processes_ok || covered_ids.contains(gpu_device.id.as_str()) {
                continue;
            }
            for i in 0..dev.processes_count as usize {
                let nvtop_proc = unsafe { &*dev.processes.add(i) };

                let process = Process {
                    kind: match nvtop_proc.r#type {
                        GpuProcessType::Graphical => Some(ProcessKind::Graphical as i32),
                        GpuProcessType::Compute => Some(ProcessKind::Compute as i32),
                        GpuProcessType::GraphicalCompute => {
                            Some(ProcessKind::GraphicalCompute as i32)
                        }
                        _ => None,
                    },
                    memory_usage_bytes: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::GpuMemoryUsageValid
                    ) {
                        Some(nvtop_proc.gpu_memory_usage)
                    } else {
                        None
                    },
                    gpu_usage_percent: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::GpuUsageValid
                    ) {
                        Some(nvtop_proc.gpu_usage as f32)
                    } else {
                        None
                    },
                    encode_usage_percent: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::EncodeUsageValid
                    ) {
                        Some(nvtop_proc.encode_usage as f32)
                    } else {
                        None
                    },
                    decode_usage_percent: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::DecodeUsageValid
                    ) {
                        Some(nvtop_proc.decode_usage as f32)
                    } else {
                        None
                    },
                };
                gpu.processes.insert(nvtop_proc.pid as u32, process);
            }
        }

        let mut retry_nvml = false;
        for device in inventory.values() {
            let gpu = self.gpus.get_mut(&device.id).unwrap();
            let native = self.devices.values().any(|native| native.id == device.id);
            let mut driver = self.metadata.get(&device.id).cloned().unwrap_or_default();
            let mut memory = memory_info(device, gpu, &driver);
            let mut nvml_clients = None;
            let mut nvml_engine_states = None;
            let mut native_fallback = if native {
                if dynamic_ok && processes_ok {
                    TelemetryState::Unsupported
                } else {
                    TelemetryState::Stale
                }
            } else {
                TelemetryState::DriverUnavailable
            };
            if gpu.vendor_id == 0x10de || driver.kernel_driver.as_deref() == Some("nvidia") {
                // NVTOP's UMA fallback uses host MemTotal, and its v2 ABI version
                // is incorrect in this pin. Only real NVML framebuffer data is
                // accepted; a missing symbol or unsupported UMA stays absent.
                let cursor = self.nvml_cursors.entry(device.id.clone()).or_default();
                let sample = self
                    .nvml
                    .as_ref()
                    .ok_or(TelemetryState::DriverUnavailable)
                    .and_then(|nvml| nvml.sample(&device.id, cursor.timestamp));
                match sample {
                    Ok(sample) => {
                        if gpu.device_name.is_none() {
                            gpu.device_name = sample.name;
                        }
                        apply_nvml_sensors(gpu, &sample.sensors);
                        // Keep valid native readings; absent/new devices get
                        // these same direct queries without any C pointer.
                        gpu.utilization_percent = gpu.utilization_percent.or(sample.overall.ok());
                        gpu.encoder_percent = gpu.encoder_percent.or(sample.encode.ok());
                        gpu.decoder_percent = gpu.decoder_percent.or(sample.decode.ok());
                        native_fallback = sample
                            .overall
                            .err()
                            .or(sample.encode.err())
                            .or(sample.decode.err())
                            .unwrap_or(TelemetryState::Unsupported);
                        nvml_engine_states = Some((sample.encode.err(), sample.decode.err()));
                        match sample.memory {
                            Ok(value) => {
                                memory.dedicated_total_bytes = Some(value.total);
                                memory.dedicated_used_bytes = Some(value.used);
                                memory.reserved_bytes = value.reserved;
                                memory.dedicated_state = TelemetryState::Available as i32;
                            }
                            Err(error) => {
                                memory.dedicated_total_bytes = None;
                                memory.dedicated_used_bytes = None;
                                memory.dedicated_state = error as i32;
                            }
                        }
                        // The pinned C process collector fabricates valid zeros
                        // when NVML supplies no fresh utilization sample. Read
                        // these optional APIs directly so missing stays absent.
                        let mut clients =
                            cursor.apply(&sample.processes, &native_identities, Path::new("/proc"));
                        gpu.processes = std::mem::take(&mut clients.processes);
                        nvml_clients = Some(clients);
                    }
                    Err(error) => {
                        memory.dedicated_total_bytes = None;
                        memory.dedicated_used_bytes = None;
                        memory.dedicated_state = error as i32;
                        // A failed fresh PCI lookup invalidates native results
                        // too; never publish a stale C handle's measurements.
                        clear_dynamic(gpu);
                        native_fallback = error;
                        nvml_engine_states = Some((Some(error), Some(error)));
                        self.nvml_cursors.remove(&device.id);
                        if error == TelemetryState::DriverUnavailable {
                            // Loss can precede sysfs removal. Retire the old C
                            // allocation now, even if PCI/driver paths survive.
                            self.devices.retain(|node, native| {
                                if native.id == device.id {
                                    self.native_tokens.remove(node);
                                    false
                                } else {
                                    true
                                }
                            });
                            retry_nvml = true;
                        }
                    }
                }
                driver.version = self
                    .nvml
                    .as_ref()
                    .and_then(nvml::Nvml::driver_version)
                    .or(driver.version);
            }
            gpu.total_memory = memory.dedicated_total_bytes;
            gpu.used_memory = memory.dedicated_used_bytes;
            gpu.total_shared_memory = memory.shared_total_bytes;
            gpu.used_shared_memory = memory.shared_used_bytes;

            let mut telemetry = GpuTelemetry {
                epoch: self.epoch,
                sequence: self.sequence,
                sample_age_ms: 0,
                memory: Some(memory),
                driver: Some(driver),
                ..Default::default()
            };
            if let Some(sample) = fd_samples.remove(&device.id) {
                telemetry.engines = sample.engines;
                telemetry.clients = sample.clients;
                telemetry.observations = sample.observations;
                telemetry.state = sample.state.unwrap_or(TelemetryState::Unsupported) as i32;
                let derived = telemetry
                    .engines
                    .iter()
                    .filter_map(|engine| engine.usage_percent)
                    .reduce(f32::max);
                telemetry.overall_usage_percent = gpu.utilization_percent.or(derived);
                telemetry.overall_is_partial =
                    Some(gpu.utilization_percent.is_none() || sample.partial);
                if gpu.utilization_percent.is_none() {
                    gpu.utilization_percent = derived;
                }
                // Preserve raw engine IDs; a combined video engine is one series.
                // A shared video engine (amdgpu VCN 4+, Intel VCS) serves both legacy fields.
                let video = kind_usage(&telemetry.engines, EngineKind::Video);
                if gpu.encoder_percent.is_none() {
                    gpu.encoder_percent =
                        kind_usage(&telemetry.engines, EngineKind::Encode).or(video);
                }
                if gpu.decoder_percent.is_none() {
                    gpu.decoder_percent =
                        kind_usage(&telemetry.engines, EngineKind::Decode).or(video);
                }
                gpu.encode_decode_shared = telemetry
                    .engines
                    .iter()
                    .any(|engine| engine.kind == EngineKind::Video as i32);
                // Legacy per-PID data cannot express shared clients. Supply only
                // singly-owned client groups, avoiding duplicate attribution.
                for client in &telemetry.clients {
                    if let [owner] = client.owners.as_slice() {
                        let process = gpu.processes.entry(owner.pid).or_default();
                        process.gpu_usage_percent =
                            optional_max(process.gpu_usage_percent, client.overall_usage_percent);
                        process.encode_usage_percent = optional_max(
                            process.encode_usage_percent,
                            kind_usage(&client.engines, EngineKind::Encode),
                        );
                        process.decode_usage_percent = optional_max(
                            process.decode_usage_percent,
                            kind_usage(&client.engines, EngineKind::Decode),
                        );
                        if let Some(bytes) = client.dedicated_memory_bytes {
                            process.memory_usage_bytes = Some(
                                process
                                    .memory_usage_bytes
                                    .unwrap_or(0)
                                    .saturating_add(bytes),
                            );
                        }
                    }
                }
            } else {
                telemetry.engines = native_engines(gpu);
                if let Some((encode, decode)) = nvml_engine_states {
                    for engine in &mut telemetry.engines {
                        if engine.usage_percent.is_none() {
                            let error = if engine.kind == EngineKind::Encode as i32 {
                                encode
                            } else {
                                decode
                            };
                            if let Some(error) = error {
                                engine.state = error as i32;
                            }
                        }
                    }
                }
                telemetry.overall_usage_percent = gpu.utilization_percent;
                telemetry.overall_is_partial = Some(true);
                telemetry.state = native_telemetry_state(gpu, native_fallback) as i32;
                if let Some(clients) = nvml_clients {
                    telemetry.clients = clients.clients;
                    telemetry.observations = clients.observations;
                } else {
                    native_clients(gpu, &mut telemetry, processes_ok, &native_identities);
                }
            }
            gpu.telemetry = Some(telemetry);
        }
        if retry_nvml {
            // Our NVML reference is safely reference-counted independently of
            // NVTOP; a later sample can initialize again after a driver reload.
            self.nvml = None;
        }
        let age = sample_start.elapsed().as_millis().min(u64::MAX as u128) as u64;
        for gpu in self.gpus.values_mut() {
            if let Some(telemetry) = gpu.telemetry.as_mut() {
                telemetry.sample_age_ms = age;
            }
        }
    }

    fn cached_entries(&self) -> &HashMap<String, Gpu> {
        &self.gpus
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::drm::{Bus, DrmDevice};
    use super::*;

    fn drm_device(sysname: &str, bus: Bus, driver: &str, card_index: u32) -> DrmDevice {
        DrmDevice {
            card_index,
            sysname: sysname.to_owned(),
            bus,
            driver: driver.to_owned(),
            drm_driver: Some(String::new()),
            sysfs_device: PathBuf::from(format!("/sys/class/drm/card{card_index}/device")),
            card_node: PathBuf::from(format!("/dev/dri/card{card_index}")),
            card_dev: (226, card_index),
            render_node: Some(PathBuf::from(format!(
                "/dev/dri/renderD{}",
                128 + card_index
            ))),
            render_dev: Some((226, 128 + card_index)),
        }
    }

    fn devices() -> Vec<DrmDevice> {
        vec![
            drm_device("0000:0a:00.0", Bus::Pci, "nvidia", 0),
            drm_device("1002000000.v3d", Bus::Platform, "v3d", 1),
        ]
    }

    #[test]
    fn matches_a_soc_gpu_by_driver_name() {
        // nvtop leaves `pdev` empty for v3d, msm, panfrost and panthor
        assert_eq!(match_device(&mut devices(), "", "v3d"), Some(1));
        assert_eq!(match_device(&mut devices(), "", "panthor"), None);
    }

    #[test]
    fn matches_a_soc_gpu_whose_drm_driver_differs_from_its_bus_driver() {
        let mut devices = devices();
        devices[1].driver = "mali".to_owned();
        devices[1].drm_driver = Some("panthor".to_owned());

        assert_eq!(match_device(&mut devices, "", "panthor"), Some(1));
        assert_eq!(match_device(&mut devices, "", "mali"), Some(1));
    }

    #[test]
    fn recognizes_pci_addresses() {
        assert!(is_pci_address("0000:0a:00.0"));
        assert!(is_pci_address("0000:0A:00.0"));
        assert!(is_pci_address("00000000:65:00.0"));
    }

    #[test]
    fn rejects_anything_that_is_not_a_pci_address() {
        for address in ["", "RK-NPU0", "TPU0", "1-2"] {
            assert!(!is_pci_address(address), "{address}");
        }

        for address in [
            "../../etc/passwd",
            "0000:0a:00.0/uevent",
            "0000:0a:00",
            "0000:0a:00.",
            "0:0a:00.0",
            "0000:g0:00.0",
            "0000.0a:00:0",
        ] {
            assert!(!is_pci_address(address), "{address}");
        }
    }

    #[test]
    fn matches_a_pci_gpu_regardless_of_case() {
        // NVML reports the bus ID with uppercase hex, sysfs uses lowercase
        assert_eq!(
            match_device(&mut devices(), "0000:0A:00.0", "NVIDIA"),
            Some(0)
        );
        assert_eq!(
            match_device(&mut devices(), "0000:0a:00.0", "NVIDIA"),
            Some(0)
        );
        assert_eq!(
            match_device(&mut devices(), "00000000:0A:00.0", "NVIDIA"),
            Some(0)
        );
    }

    #[test]
    fn a_pci_gpu_is_never_matched_by_backend_name() {
        assert_eq!(match_device(&mut devices(), "0000:99:00.0", "nvidia"), None);
    }

    #[test]
    fn soc_gpus_probe_the_render_node_before_the_card_node() {
        let device = GpuDevice {
            id: "1002000000.v3d".to_owned(),
            sysfs_device: None,
            is_pci: false,
            drm: Some(drm_device("1002000000.v3d", Bus::Platform, "v3d", 1)),
        };

        assert_eq!(
            device.render_nodes(),
            vec![
                PathBuf::from("/dev/dri/renderD129"),
                PathBuf::from("/dev/dri/card1")
            ]
        );
    }

    #[test]
    fn a_gpu_without_a_drm_card_falls_back_to_the_by_path_symlink() {
        let device = GpuDevice {
            id: "0000:01:00.0".to_owned(),
            sysfs_device: None,
            is_pci: true,
            drm: None,
        };

        assert_eq!(
            device.render_nodes(),
            vec![PathBuf::from("/dev/dri/by-path/pci-0000:01:00.0-card")]
        );
    }

    #[test]
    fn matches_vulkan_devices_by_drm_node() {
        let device = GpuDevice {
            id: "1002000000.v3d".to_owned(),
            sysfs_device: None,
            is_pci: false,
            drm: Some(drm_device("1002000000.v3d", Bus::Platform, "v3d", 1)),
        };

        let version = magpie_platform::gpus::ApiVersion {
            major: 1,
            minor: 3,
            patch: Some(0),
        };

        assert!(device.is_vulkan_device(&VulkanDevice {
            ids_key: 0,
            primary_node: Some((226, 1)),
            render_node: None,
            version,
            ..Default::default()
        }));
        assert!(device.is_vulkan_device(&VulkanDevice {
            ids_key: 0,
            primary_node: None,
            render_node: Some((226, 129)),
            version,
            ..Default::default()
        }));
        // A software renderer reports no DRM node at all, it must not match
        assert!(!device.is_vulkan_device(&VulkanDevice {
            ids_key: 0,
            primary_node: None,
            render_node: None,
            version,
            ..Default::default()
        }));
    }

    #[test]
    fn native_hotplug_filter_restores_every_link() {
        let empty = || nvtop::ListHead {
            next: std::ptr::null_mut(),
            prev: std::ptr::null_mut(),
        };
        let mut head = empty();
        let mut a = Box::new(empty());
        let mut b = Box::new(empty());
        let h: *mut nvtop::ListHead = &mut head;
        let ap: *mut nvtop::ListHead = a.as_mut();
        let bp: *mut nvtop::ListHead = b.as_mut();
        head.next = ap;
        head.prev = bp;
        a.prev = h;
        a.next = bp;
        b.prev = ap;
        b.next = h;
        let device = |id: &str| GpuDevice {
            id: id.into(),
            sysfs_device: None,
            is_pci: false,
            drm: None,
        };
        let devices = HashMap::from([(ap as usize, device("a")), (bp as usize, device("b"))]);
        let inventory = HashMap::from([("b".into(), device("b"))]);
        with_live_native(&mut head, &devices, &inventory, |list| {
            assert_eq!(list.next, bp);
            assert_eq!(list.prev, bp);
            assert_eq!(b.next, h);
            assert_eq!(b.prev, h);
        });
        assert_eq!(head.next, ap);
        assert_eq!(head.prev, bp);
        assert_eq!(a.prev, h);
        assert_eq!(a.next, bp);
        assert_eq!(b.prev, ap);
        assert_eq!(b.next, h);
    }

    #[test]
    fn native_overall_is_never_labeled_render_or_compute() {
        let gpu = Gpu {
            utilization_percent: Some(25.),
            encoder_percent: None,
            decoder_percent: Some(0.),
            ..Default::default()
        };
        let engines = native_engines(&gpu);
        assert_eq!(engines.len(), 2);
        assert!(engines.iter().all(|engine| engine.id != "overall"));
        assert_eq!(engines[0].kind, EngineKind::Encode as i32);
        assert_eq!(engines[0].usage_percent, None);
        assert_eq!(engines[0].state, TelemetryState::Unsupported as i32);
        assert_eq!(engines[1].usage_percent, Some(0.));
        assert_eq!(
            native_telemetry_state(&gpu, TelemetryState::Unsupported),
            TelemetryState::Partial
        );
        let missing = Gpu::default();
        for state in [
            TelemetryState::Unsupported,
            TelemetryState::DriverUnavailable,
            TelemetryState::PermissionDenied,
        ] {
            assert_eq!(native_telemetry_state(&missing, state), state);
        }
    }

    #[test]
    fn pci_inventory_handles_no_startup_gpu_and_nvidia_without_drm() {
        let root = GpuFixture::new();
        let mut inventory = HashMap::new();
        union_nvidia_pci_devices(&mut inventory, &root.0);
        assert!(inventory.is_empty());
        let path = root.0.join("0000:01:00.0");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("vendor"), "0x10de\n").unwrap();
        std::fs::write(path.join("class"), "0x030200\n").unwrap();
        union_nvidia_pci_devices(&mut inventory, &root.0);
        assert!(inventory["0000:01:00.0"].drm.is_none());
        // An existing DRM entry stays authoritative; NVIDIA audio/USB functions
        // sharing the vendor ID must not become phantom GPUs.
        let drm = devices().remove(0);
        inventory.insert(
            "0000:01:00.0".into(),
            GpuDevice {
                id: "0000:01:00.0".into(),
                sysfs_device: Some(path.clone()),
                is_pci: true,
                drm: Some(drm),
            },
        );
        std::fs::write(path.join("class"), "0x040300\n").unwrap();
        union_nvidia_pci_devices(&mut inventory, &root.0);
        assert!(inventory["0000:01:00.0"].drm.is_some());
        inventory.clear();
        union_nvidia_pci_devices(&mut inventory, &root.0);
        assert!(inventory.is_empty());
        std::fs::write(path.join("class"), "0x030000\n").unwrap();
        union_nvidia_pci_devices(&mut inventory, &root.0);
        assert_eq!(inventory.len(), 1);
        std::fs::remove_dir_all(&path).unwrap();
        inventory.clear();
        union_nvidia_pci_devices(&mut inventory, &root.0);
        assert!(inventory.is_empty());
    }

    struct GpuFixture(PathBuf);
    impl GpuFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "magpie-gpu-hotplug-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for GpuFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_native_pointer_is_retired_even_when_the_same_sysfs_device_rebinds() {
        use std::os::unix::fs::symlink;
        let root = GpuFixture::new();
        let path = root.0.join("gpu");
        let driver = root.0.join("nvidia");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(&driver).unwrap();
        symlink(&driver, path.join("driver")).unwrap();
        let device = GpuDevice {
            id: "0000:01:00.0".into(),
            sysfs_device: Some(path.clone()),
            is_pci: true,
            drm: None,
        };
        let original = device_token(&device);
        let mut tokens = HashMap::from([(123, original.clone())]);
        let mut devices = HashMap::from([(123, device.clone())]);
        retire_native_devices(&mut devices, &mut tokens);
        assert_eq!(devices.len(), 1);
        std::fs::remove_file(path.join("driver")).unwrap();
        retire_native_devices(&mut devices, &mut tokens);
        assert!(devices.is_empty());
        assert!(tokens.is_empty());
        symlink(&driver, path.join("driver")).unwrap();
        assert_eq!(device_token(&device), original);
        devices.insert(123, device);
        retire_native_devices(&mut devices, &mut tokens);
        assert!(devices.is_empty());
    }

    #[test]
    fn native_uma_host_memory_is_not_a_gpu_budget() {
        let device = GpuDevice {
            id: "gpu".into(),
            sysfs_device: None,
            is_pci: false,
            drm: None,
        };
        let gpu = Gpu {
            total_memory: Some(1000),
            used_memory: Some(100),
            ..Default::default()
        };
        for name in ["msm", "panfrost", "panthor", "nvidia", "i915", "xe"] {
            let driver = DriverInfo {
                kernel_driver: Some(name.into()),
                ..Default::default()
            };
            let memory = memory_info(&device, &gpu, &driver);
            assert_eq!(memory.dedicated_total_bytes, None, "{name}");
            assert_eq!(memory.shared_total_bytes, None, "{name}");
        }
        let driver = DriverInfo {
            kernel_driver: Some("i915".into()),
            integrated: Some(false),
            ..Default::default()
        };
        assert_eq!(
            memory_info(&device, &gpu, &driver).dedicated_total_bytes,
            Some(1000)
        );
    }
}
