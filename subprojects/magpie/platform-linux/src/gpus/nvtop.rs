/* src/gpus/nvtop.rs
 *
 * Copyright 2025 Mission Center Developers
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

use nix::libc::{c_char, c_ulong, c_ulonglong};

#[macro_export]
macro_rules! gpu_info_valid {
    ($info: expr, $field: expr) => {{
        let field = $field as usize;
        ((($info).valid)[field / 8] & (1 << (field % 8))) != 0
    }};
}

const MAX_DEVICE_NAME: usize = 128;
const PDEV_LEN: usize = 16;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct ListHead {
    pub next: *mut ListHead,
    pub prev: *mut ListHead,
}

// TODO: This is a requirement for Arc<RwLock<ListHead>> to work
//       But I don't understand why it's needed since access to the ListHead always happens via
//       RwLock::write() or RwLock::read() which should ensure that the ListHead is only mutated
//       by one thread at a time.
unsafe impl Send for ListHead {}
unsafe impl Sync for ListHead {}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GpuVendor {
    pub list: ListHead,

    pub init: Option<unsafe extern "C" fn() -> u8>,
    pub shutdown: Option<unsafe extern "C" fn()>,

    pub last_error_string: Option<extern "C" fn() -> *const i8>,

    pub get_device_handles: Option<extern "C" fn(devices: *mut ListHead, count: *mut u32) -> u8>,

    pub populate_static_info: Option<extern "C" fn(gpu_info: *mut GpuInfo)>,
    pub refresh_dynamic_info: Option<extern "C" fn(gpu_info: *mut GpuInfo)>,
    pub refresh_utilisation_rate: Option<extern "C" fn(gpu_info: *mut GpuInfo)>,

    pub refresh_running_processes: Option<extern "C" fn(gpu_info: *mut GpuInfo)>,

    pub name: *mut i8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GpuInfoStaticInfoValid {
    DeviceNameValid = 0,
    MaxPcieGenValid,
    MaxPcieLinkWidthValid,
    TemperatureShutdownThresholdValid,
    TemperatureSlowdownThresholdValid,
    NumberSharedCoresValid,
    L2CacheSizeValid,
    NumberExecEnginesValid,
    NumberEnginesValid,
    StaticInfoCount,
}

const GPU_INFO_STATIC_INFO_COUNT: usize = GpuInfoStaticInfoValid::StaticInfoCount as usize;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GpuInfoStaticInfo {
    pub device_name: [c_char; MAX_DEVICE_NAME],
    pub max_pcie_gen: u32,
    pub max_pcie_link_width: u32,
    pub temperature_shutdown_threshold: u32,
    pub temperature_slowdown_threshold: u32,
    pub n_shared_cores: u32,
    pub l2cache_size: u32,
    pub n_exec_engines: u32,
    pub engine_count: u32,
    pub integrated_graphics: bool,
    pub encode_decode_shared: bool,
    pub valid: [u8; GPU_INFO_STATIC_INFO_COUNT.div_ceil(u8::BITS as _)],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GpuInfoDynamicInfoValid {
    GpuClockSpeedValid = 0,
    GpuClockSpeedMaxValid,
    MemClockSpeedValid,
    MemClockSpeedMaxValid,
    GpuUtilRateValid,
    MemUtilRateValid,
    EncoderRateValid,
    DecoderRateValid,
    TotalMemoryValid,
    FreeMemoryValid,
    UsedMemoryValid,
    PcieLinkGenValid,
    PcieLinkWidthValid,
    PcieRxValid,
    PcieTxValid,
    FanSpeedValid,
    FanRpmValid,
    GpuTempValid,
    PowerDrawValid,
    PowerDrawMaxValid,
    EffectiveLoadRateValid,
    MultiInstanceModeValid,
    DynamicInfoCount,
}

const GPU_INFO_DYNAMIC_INFO_COUNT: usize = GpuInfoDynamicInfoValid::DynamicInfoCount as usize;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GpuInfoDynamicInfo {
    pub gpu_clock_speed: u32,
    pub gpu_clock_speed_max: u32,
    pub mem_clock_speed: u32,
    pub mem_clock_speed_max: u32,
    pub gpu_util_rate: u32,
    pub mem_util_rate: u32,
    pub effective_load_rate: u32,
    pub encoder_rate: u32,
    pub decoder_rate: u32,
    pub total_memory: u64,
    pub free_memory: u64,
    pub used_memory: u64,
    pub pcie_link_gen: u32,
    pub pcie_link_width: u32,
    pub pcie_rx: u32,
    pub pcie_tx: u32,
    pub fan_speed: u32,
    pub fan_rpm: u32,
    pub gpu_temp: u32,
    pub power_draw: u32,
    pub power_draw_max: u32,
    pub multi_instance_mode: bool,
    pub valid: [u8; GPU_INFO_DYNAMIC_INFO_COUNT.div_ceil(u8::BITS as _)],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GpuProcessType {
    Unknown = 0,
    Graphical = 1,
    Compute = 2,
    GraphicalCompute = 3,
    Count,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum GpuInfoProcessInfoValid {
    CmdlineValid,
    UserNameValid,
    GfxEngineUsedValid,
    ComputeEngineUsedValid,
    EncEngineUsedValid,
    DecEngineUsedValid,
    GpuUsageValid,
    EncodeUsageValid,
    DecodeUsageValid,
    GpuMemoryUsageValid,
    GpuMemoryPercentageValid,
    CpuUsageValid,
    CpuMemoryVirtValid,
    CpuMemoryResValid,
    GpuCyclesValid,
    SampleDeltaValid,
    ProcessValidInfoCount,
}

const GPU_PROCESS_INFO_VALID_INFO_COUNT: usize =
    GpuInfoProcessInfoValid::ProcessValidInfoCount as usize;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GpuProcess {
    pub r#type: GpuProcessType,
    pub pid: i32,
    pub cmdline: *mut c_char,
    pub user_name: *mut c_char,
    pub sample_delta: u64, // Time spent between two successive samples
    pub gfx_engine_used: u64,
    pub compute_engine_used: u64,
    pub enc_engine_used: u64,
    pub dec_engine_used: u64,
    pub gpu_cycles: u64, // Number of GPU cycles spent in the GPU gfx engine
    pub gpu_usage: u32,
    pub encode_usage: u32,
    pub decode_usage: u32,
    pub gpu_memory_usage: c_ulonglong,
    pub gpu_memory_percentage: u32,

    pub cpu_usage: u32,
    pub cpu_memory_virt: c_ulong,
    pub cpu_memory_res: c_ulong,
    pub valid: [u8; GPU_PROCESS_INFO_VALID_INFO_COUNT.div_ceil(u8::BITS as _)],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct GpuInfo {
    pub list: ListHead,
    pub vendor: *mut GpuVendor,
    pub static_info: GpuInfoStaticInfo,
    pub dynamic_info: GpuInfoDynamicInfo,
    pub processes_count: u32,
    pub processes: *mut GpuProcess,
    pub processes_array_size: u32,
    pub pdev: [c_char; PDEV_LEN],
}

extern "C" {
    pub fn gpuinfo_init_info_extraction(
        monitored_dev_count: *mut u32,
        devices: *mut ListHead,
    ) -> u8;

    pub fn gpuinfo_shutdown_info_extraction(devices: *mut ListHead) -> u8;

    pub fn init_extract_gpuinfo_amdgpu();
    pub fn init_extract_gpuinfo_ascend();
    pub fn init_extract_gpuinfo_intel();
    pub fn init_extract_gpuinfo_ixml();
    pub fn init_extract_gpuinfo_metax();
    pub fn init_extract_gpuinfo_msm();
    pub fn init_extract_gpuinfo_nvidia();
    pub fn init_extract_gpuinfo_panfrost();
    pub fn init_extract_gpuinfo_panthor();
    pub fn init_extract_gpuinfo_radeon();
    pub fn init_extract_gpuinfo_tenstorrent();
    pub fn init_extract_gpuinfo_tpu();
    pub fn init_extract_gpuinfo_v3d();
    pub fn init_extract_gpuinfo_rknpu();

    pub fn gpuinfo_populate_static_infos(devices: *mut ListHead) -> u8;
    pub fn gpuinfo_refresh_dynamic_info(devices: *mut ListHead) -> u8;
    pub fn gpuinfo_refresh_processes(devices: *mut ListHead) -> u8;
    pub fn processinfo_enable_disable_callback_for(info: *const GpuInfo, enable: bool);
    pub fn gpuinfo_utilisation_rate(devices: *mut ListHead) -> u8;
    pub fn gpuinfo_fix_dynamic_info_from_process_info(devices: *mut ListHead) -> u8;
}
