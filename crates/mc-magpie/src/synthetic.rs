/* crates/mc-magpie/src/synthetic.rs
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

//! Deterministic synthetic readings (no data from the host), used by snapshot tests and
//! benchmarks.

use std::collections::HashMap;

use magpie_types::battery::{Battery, HistoryPoint};
use magpie_types::disks::{Disk, DiskKind, PartitionInfo, SmartInterface};
use magpie_types::fan::Fan;
use magpie_types::gpus::{ApiVersion, Gpu};
use magpie_types::memory::{Memory, MemoryDevice};
use magpie_types::network::{Connection, ConnectionKind, ConnectionState, WirelessConnection};
use magpie_types::processes::{Process, ProcessUsageStats};

use crate::{App, Readings, Service};

fn wave(step: usize, period: f32, phase: f32, lo: f32, hi: f32) -> f32 {
    let t = step as f32 / period * std::f32::consts::TAU + phase;
    lo + (hi - lo) * (0.5 + 0.5 * t.sin())
}

/// `process_count` controls the size of the process tree (for table benchmarks).
pub fn synthetic_readings(step: usize, process_count: usize) -> Readings {
    let mut r = Readings::new();
    let cores = 8;

    r.cpu.name = Some("Synthetic CPU 8-Core".into());
    r.cpu.socket_count = Some(1);
    r.cpu.base_freq_khz = Some(3_200_000);
    r.cpu.virtualization_technology = Some("AMD-V".into());
    r.cpu.is_virtual_machine = Some(false);
    r.cpu.l1_combined_cache_bytes = Some(512 * 1024);
    r.cpu.l2_cache_bytes = Some(8 * 1024 * 1024);
    r.cpu.l3_cache_bytes = Some(32 * 1024 * 1024);
    r.cpu.core_usage_percent = (0..cores)
        .map(|c| wave(step, 17., c as f32, 5., 90.))
        .collect();
    r.cpu.core_kernel_usage_percent = r.cpu.core_usage_percent.iter().map(|v| v / 5.).collect();
    r.cpu.total_usage_percent = r.cpu.core_usage_percent.iter().sum::<f32>() / cores as f32;
    r.cpu.kernel_usage_percent = r.cpu.total_usage_percent / 5.;
    r.cpu.current_frequency_mhz = wave(step, 11., 0., 1400., 4800.) as u64;
    r.cpu.temperature_celsius = Some(wave(step, 23., 1., 40., 75.));
    r.cpu.total_process_count = process_count as u64;
    r.cpu.total_thread_count = process_count as u64 * 4;
    r.cpu.total_handle_count = process_count as u64 * 40;
    r.cpu.uptime_seconds = 3600 * 5 + step as u64;
    r.cpu.frequency_driver = Some("amd-pstate-epp".into());
    r.cpu.frequency_governor = Some("powersave".into());
    r.cpu.power_draw_w = Some(wave(step, 13., 2., 5., 35.));

    let gib = 1024u64 * 1024 * 1024;
    let used = (wave(step, 29., 0., 0.35, 0.6) * 32. * gib as f32) as u64;
    let cached = 8 * gib;
    let shared = gib;
    let buffers = gib / 2;
    let cache = cached - shared;
    r.mem_info = Memory {
        mem_total: 32 * gib,
        mem_free: 32 * gib - used - buffers - cache,
        mem_available: 32 * gib - used,
        cached,
        sh_mem: shared,
        buffers,
        committed: used + 2 * gib,
        dirty: 64 * 1024 * 1024,
        swap_total: 8 * gib,
        swap_free: 7 * gib,
        max_devices: 4,
        ..Default::default()
    };
    r.mem_devices = vec![
        MemoryDevice {
            size: 16 * gib,
            form_factor: "SODIMM".into(),
            locator: "DIMM 0".into(),
            bank_locator: "BANK 0".into(),
            ram_type: "DDR5".into(),
            speed: 5600,
            rank: 1,
            ..Default::default()
        };
        2
    ];

    let mut partitions = HashMap::new();
    partitions.insert(
        "nvme0n1p2".to_string(),
        PartitionInfo {
            devname: "nvme0n1p2".into(),
            size: Some(900 * gib),
            used: Some(400 * gib),
            filesystem: Some("btrfs".into()),
            mountpoints: vec!["/".into(), "/home".into()],
        },
    );
    r.disks_info = vec![Disk {
        id: "nvme0n1".into(),
        model: Some("Synthetic NVMe 1TB".into()),
        kind: Some(DiskKind::NvMe as i32),
        smart_interface: Some(magpie_types::disks::smart_interface::Kind::NvMe as i32),
        capacity_bytes: 1000 * gib,
        formatted_bytes: Some(950 * gib),
        is_system: true,
        busy_percent: wave(step, 7., 0., 0., 40.),
        response_time_ms: 0.4,
        rx_speed_bytes_ps: wave(step, 9., 0., 0., 200e6) as u64,
        rx_bytes_total: 50 * gib,
        tx_speed_bytes_ps: wave(step, 9., 1., 0., 80e6) as u64,
        tx_bytes_total: 20 * gib,
        ejectable: false,
        temperature_milli_k: Some(318_150),
        serial_number: Some("SYN-0001".into()),
        world_wide_name: None,
        rotation_rate: None,
        sector_size: 512,
        partitions,
    }];
    let _ = SmartInterface::default();

    let mut connections = HashMap::new();
    connections.insert(
        "wlan0".to_string(),
        Connection {
            id: "wlan0".into(),
            device_name: Some("Synthetic Wi-Fi".into()),
            kind: ConnectionKind::Wireless as i32,
            wireless_connection: Some(WirelessConnection {
                ssid: Some("synthetic".into()),
                frequency_mhz: Some(5180),
                bitrate_kbps: Some(866_000),
                signal_strength_percent: Some(70),
            }),
            hw_address: "02:00:00:00:00:01".into(),
            tx_rate_bytes_ps: wave(step, 5., 0., 0., 2e6),
            tx_total_bytes: 3 * gib,
            rx_rate_bytes_ps: wave(step, 6., 1., 0., 9e6),
            rx_total_bytes: 12 * gib,
            max_speed_bytes_ps: Some(108_250_000),
            ipv4_address: Some("192.0.2.10".into()),
            ipv6_address: None,
            state: ConnectionState::Connected as i32,
        },
    );
    r.network_connections = connections;

    let mut gpus = HashMap::new();
    gpus.insert(
        "0000:01:00.0".to_string(),
        Gpu {
            id: "0000:01:00.0".into(),
            device_name: Some("Synthetic GPU".into()),
            vendor_id: 0x1002,
            device_id: 0x1900,
            temperature_c: Some(wave(step, 19., 0., 38., 70.)),
            utilization_percent: Some(wave(step, 8., 2., 0., 100.)),
            power_draw_watts: Some(wave(step, 8., 2., 3., 40.)),
            max_power_draw_watts: Some(54.),
            clock_speed_mhz: Some(wave(step, 8., 2., 400., 2700.) as u32),
            max_clock_speed_mhz: Some(2700),
            memory_speed_mhz: Some(2800),
            max_memory_speed_mhz: Some(2800),
            total_memory: Some(4 * gib),
            used_memory: Some((wave(step, 31., 0., 0.2, 0.7) * 4. * gib as f32) as u64),
            encoder_percent: Some(0.),
            decoder_percent: Some(wave(step, 9., 0., 0., 30.)),
            vulkan_version: Some(ApiVersion {
                major: 1,
                minor: 4,
                patch: Some(0),
            }),
            opengl_version: Some(ApiVersion {
                major: 4,
                minor: 6,
                patch: None,
            }),
            pcie_gen: Some(4),
            pcie_lanes: Some(16),
            ..Default::default()
        },
    );
    r.gpus = gpus;

    r.fans = vec![Fan {
        fan_label: Some("cpu_fan".into()),
        temp_name: Some("k10temp".into()),
        fan_index: 1,
        hwmon_index: 2,
        rpm: wave(step, 21., 0., 1200., 3200.) as u32,
        temp_amount: Some(323_150),
        pwm_percent: Some(wave(step, 21., 0., 0.3, 0.8)),
        max_rpm: None,
    }];

    r.batteries = vec![Battery {
        name: "BAT0".into(),
        vendor: Some("Synthetic".into()),
        model: Some("Cell 60Wh".into()),
        kind: Some(2),
        technology: Some(1),
        power_supply: Some(true),
        energy_full: Some(60_000),
        energy_full_design: Some(64_000),
        capacity: Some(0.93),
        charge_cycles: Some(120),
        percentage: 0.8,
        energy: Some(48_000),
        voltage: Some(15.6),
        power: Some(9.5),
        time_to_empty: Some(18_000),
        state: Some(2),
        charge_threshold_enabled: false,
        charge_threshold_supported: 0,
        history: (0..30)
            .map(|i| HistoryPoint {
                x: 600_000. - i as f32 * 20_000.,
                y: 0.4 + i as f32 / 60.,
            })
            .collect(),
        history_changed: step == 0,
        ..Default::default()
    }];

    // Process tree: systemd -> N children, a few belonging to apps
    let mut init = Process {
        name: "systemd".into(),
        cmd: vec!["/usr/lib/systemd/systemd".into()],
        exe: "/usr/lib/systemd/systemd".into(),
        pid: 1,
        start_time_ticks: Some(2),
        parent: 0,
        task_count: 1,
        ..Default::default()
    };
    for i in 0..process_count.saturating_sub(1) {
        let pid = 100 + i as u32;
        init.children.push(pid);
        let name = format!("worker-{i:04}");
        r.running_processes.insert(
            pid,
            Process {
                name: name.clone(),
                cmd: vec![format!("/usr/bin/{name}"), "--serve".into()],
                exe: format!("/usr/bin/{name}"),
                pid,
                start_time_ticks: Some(u64::from(pid) + 1),
                parent: 1,
                task_count: 2,
                usage_stats: ProcessUsageStats {
                    cpu_usage: wave(step + i, 13., i as f32, 0., 12.),
                    memory_usage: 10_000_000 + (i as u64 % 97) * 3_000_000,
                    swap_usage: 0,
                    disk_usage: wave(step + i, 7., 0., 0., 1e6),
                    network_usage: 0.,
                    gpu_usage: 0.,
                    gpu_memory_usage: 0,
                },
                ..Default::default()
            },
        );
    }
    r.running_processes.insert(1, init);
    for &pid in r.running_processes.keys() {
        let user = if pid == 1 { "root" } else { "user" };
        r.process_users.insert(pid, user.into());
    }

    for (i, (id, name)) in [
        ("org.example.Editor", "Text Editor"),
        ("org.example.Browser", "Web Browser"),
        ("org.example.Terminal", "Terminal"),
    ]
    .into_iter()
    .enumerate()
    {
        let pid = 100 + i as u32;
        if r.running_processes.contains_key(&pid) {
            r.running_apps.insert(
                id.into(),
                App {
                    id: id.into(),
                    name: name.into(),
                    command: name.to_lowercase(),
                    pids: vec![pid],
                },
            );
        }
    }

    for (i, (name, running, failed, enabled)) in [
        ("sshd.service", true, false, true),
        ("cups.service", false, true, true),
        ("bluetooth.service", false, false, true),
        ("httpd.service", false, false, false),
    ]
    .into_iter()
    .enumerate()
    {
        r.system_services.insert(
            i as u64 + 1,
            Service {
                id: i as u64 + 1,
                name: name.into(),
                description: Some(format!("Synthetic {name}")),
                enabled,
                running,
                failed,
                pid: running.then_some(100),
                user: Some("root".into()),
                group: Some("root".into()),
                file_path: Some(format!("/usr/lib/systemd/system/{name}")),
            },
        );
    }
    r.user_services.insert(
        100,
        Service {
            id: 100,
            name: "pipewire.service".into(),
            description: Some("Multimedia Service".into()),
            enabled: true,
            running: true,
            failed: false,
            pid: Some(101),
            user: Some("user".into()),
            group: Some("user".into()),
            file_path: Some("/usr/lib/systemd/user/pipewire.service".into()),
        },
    );

    // Both a discrete and an integrated adapter, without any host GPU I/O.
    // Samples model actual engine/client identities, not ProcessUsageStats sums.
    use magpie_types::gpus::{
        ClientTelemetry, DriverInfo, EngineKind, EngineTelemetry, GpuTelemetry, MemoryTelemetry,
        ProcessIdentity, ProcessObservation, TelemetryState,
    };
    let engine = |id: &str, name: &str, kind: EngineKind, load: f32| EngineTelemetry {
        id: id.into(),
        name: name.into(),
        kind: kind as i32,
        usage_percent: Some(load),
        state: TelemetryState::Available as i32,
        client_derived: Some(true),
    };
    r.gpus.insert(
        "0000:03:00.0".into(),
        Gpu {
            id: "0000:03:00.0".into(),
            device_name: Some("Synthetic integrated GPU".into()),
            vendor_id: 0x8086,
            utilization_percent: Some(wave(step, 11., 1., 0., 75.)),
            total_memory: Some(0),
            used_memory: Some(0),
            total_shared_memory: Some(8 * gib),
            used_shared_memory: Some(gib),
            ..Default::default()
        },
    );
    for (id, gpu) in &mut r.gpus {
        let integrated = id == "0000:03:00.0";
        let load = gpu.utilization_percent.unwrap();
        let observations = r
            .running_processes
            .values()
            .map(|p| ProcessObservation {
                pid: p.pid,
                start_time_ticks: p.start_time_ticks,
                state: TelemetryState::Available as i32,
            })
            .collect();
        let clients = r
            .running_processes
            .values()
            .filter(|p| p.pid != 1 && p.pid.is_multiple_of(if integrated { 5 } else { 3 }))
            .map(|p| {
                let render = wave(step + p.pid as usize, 9., 0., 0., 25.);
                ClientTelemetry {
                    id: format!("synthetic-{}", p.pid),
                    owners: vec![ProcessIdentity {
                        pid: p.pid,
                        start_time_ticks: p.start_time_ticks.unwrap(),
                    }],
                    engines: vec![
                        engine(
                            if integrated { "gfx" } else { "render" },
                            "3D",
                            EngineKind::Render,
                            render,
                        ),
                        engine(
                            if integrated { "dma" } else { "copy" },
                            "Copy",
                            EngineKind::Copy,
                            render * 0.3,
                        ),
                    ],
                    overall_usage_percent: Some(render),
                    dedicated_memory_bytes: Some(if integrated { 0 } else { 64 * 1024 * 1024 }),
                    shared_memory_bytes: Some(8 * 1024 * 1024),
                    allocated_memory_bytes: Some(if integrated { 8 } else { 72 } * 1024 * 1024),
                    memory_is_resident: Some(true),
                    usage_state: TelemetryState::Available as i32,
                    memory_state: TelemetryState::Available as i32,
                }
            })
            .collect();
        gpu.telemetry = Some(GpuTelemetry {
            epoch: 1,
            sequence: step as u64 + 1,
            state: TelemetryState::Available as i32,
            // The integrated adapter mirrors amdgpu with a unified VCN 4 video queue
            engines: if integrated {
                vec![
                    engine("gfx", "3D", EngineKind::Render, load),
                    engine("dma", "Copy", EngineKind::Copy, 0.),
                    engine("enc", "Video Encode/Decode", EngineKind::Video, load * 0.4),
                    engine("compute", "Compute", EngineKind::Compute, load * 0.3),
                ]
            } else {
                vec![
                    engine("render", "3D", EngineKind::Render, load),
                    engine("copy", "Copy", EngineKind::Copy, load * 0.2),
                    engine(
                        "video-encode",
                        "Video Encode",
                        EngineKind::Encode,
                        load * 0.1,
                    ),
                    engine(
                        "video-decode",
                        "Video Decode",
                        EngineKind::Decode,
                        load * 0.5,
                    ),
                    engine("compute", "Compute", EngineKind::Compute, load * 0.3),
                ]
            },
            clients,
            observations,
            overall_usage_percent: Some(load),
            overall_is_partial: Some(false),
            memory: Some(MemoryTelemetry {
                dedicated_total_bytes: gpu.total_memory,
                dedicated_used_bytes: gpu.used_memory,
                shared_total_bytes: Some(8 * gib),
                shared_used_bytes: Some(gib),
                reserved_bytes: Some(128 * 1024 * 1024),
                dedicated_state: TelemetryState::Available as i32,
                shared_state: TelemetryState::Available as i32,
            }),
            driver: Some(DriverInfo {
                kernel_driver: Some("synthetic".into()),
                version: Some("1.0".into()),
                integrated: Some(integrated),
                ..Default::default()
            }),
            ..Default::default()
        });
    }
    r
}

/// A sequence of snapshots (for animated replay).
pub fn synthetic_series(count: usize, process_count: usize) -> Vec<Readings> {
    (0..count)
        .map(|s| synthetic_readings(s, process_count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_readings_are_consistent() {
        let r = synthetic_readings(3, 200);
        assert_eq!(r.running_processes.len(), 200);
        assert_eq!(r.running_processes[&1].children.len(), 199);
        assert_eq!(r.running_apps.len(), 3);
        assert_eq!(r.cpu.core_usage_percent.len(), 8);

        let gib = 1024 * 1024 * 1024;
        for step in [0, 7, 22] {
            let memory = synthetic_readings(step, 1).mem_info;
            let used = memory.mem_total - memory.mem_available;
            let cache = memory.cached - memory.sh_mem;
            let used_bar =
                memory.mem_total - memory.mem_free - memory.sh_mem - memory.buffers - cache;
            assert_eq!(memory.cached, 8 * gib);
            assert_eq!(memory.sh_mem, gib);
            assert_eq!(memory.buffers, gib / 2);
            assert_eq!(memory.s_reclaimable, 0);
            assert_eq!(memory.slab, 0);
            assert_eq!(memory.k_reclaimable, 0);
            assert_eq!(
                memory.mem_available,
                memory.mem_free + memory.buffers + cache
            );
            assert_eq!(used_bar, used - memory.sh_mem);
            assert_eq!(
                used_bar + cache + memory.sh_mem + memory.buffers + memory.mem_free,
                memory.mem_total
            );
            assert!([
                used_bar,
                cache,
                memory.sh_mem,
                memory.buffers,
                memory.mem_free
            ]
            .into_iter()
            .all(|bytes| bytes > 0));
        }
    }
}
