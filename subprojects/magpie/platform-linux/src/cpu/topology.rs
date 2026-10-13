/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
use magpie_platform::cpu::Cpu;
use magpie_platform::cpu::LogicalCpu;

pub fn read(id: u32) -> LogicalCpu {
    let root = std::path::PathBuf::from(format!("/sys/devices/system/cpu/cpu{id}/topology"));
    LogicalCpu {
        id,
        package_id: crate::hardware::number(root.join("physical_package_id")),
        core_id: crate::hardware::number(root.join("core_id")),
        kind: if in_cpu_list(id, "/sys/bus/event_source/devices/cpu_lowpower/cpus") {
            Some("LP-E".into())
        } else {
            cpu_kind(id)
        },
    }
}

fn in_cpu_list(id: u32, path: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|list| {
        list.trim().split(',').any(|part| {
            if let Some((first, last)) = part.split_once('-') {
                match (first.parse::<u32>(), last.parse::<u32>()) {
                    (Ok(first), Ok(last)) => (first..=last).contains(&id),
                    _ => false,
                }
            } else {
                part.parse::<u32>() == Ok(id)
            }
        })
    })
}

#[cfg(target_arch = "x86_64")]
fn cpu_kind(id: u32) -> Option<String> {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    // CPUID 1A is per logical processor: save and restore this gatherer's affinity.
    if id as usize >= libc::CPU_SETSIZE as usize {
        return None;
    }
    unsafe {
        let mut original: libc::cpu_set_t = std::mem::zeroed();
        let size = std::mem::size_of::<libc::cpu_set_t>();
        if libc::sched_getaffinity(0, size, &mut original) != 0
            || !libc::CPU_ISSET(id as usize, &original)
        {
            return None;
        }
        let mut target = std::mem::zeroed();
        libc::CPU_SET(id as usize, &mut target);
        if libc::sched_setaffinity(0, size, &target) != 0 {
            return None;
        }
        let basic = __cpuid(0);
        let kind = if basic.ebx == 0x756e6547
            && basic.edx == 0x49656e69
            && basic.ecx == 0x6c65746e
            && basic.eax >= 0x1a
        {
            match __cpuid(0x1a).eax >> 24 {
                0x40 => Some("P".into()),
                0x20 => Some("E".into()),
                _ => None,
            }
        } else if basic.ebx == 0x68747541
            && basic.edx == 0x69746e65
            && basic.ecx == 0x444d4163
            && __cpuid(0x80000000).eax >= 0x80000026
        {
            let topology = __cpuid_count(0x80000026, 0);
            // AMD extended topology explicitly advertises heterogeneous cores in EAX[30].
            // Core type is EBX[31:28], not the preferred-core ranking or clock speed.
            if topology.eax & (1 << 30) != 0 && topology.ebx & 0xffff != 0 {
                let signature = __cpuid(1).eax;
                let family = ((signature >> 8) & 0xf) + ((signature >> 20) & 0xff);
                match (family, topology.ebx >> 28) {
                    (0x19, 0) => Some("Zen 4".into()),
                    (0x19, 1) => Some("Zen 4c".into()),
                    (0x1a, 0) => Some("Zen 5".into()),
                    (0x1a, 1) => Some("Zen 5c".into()),
                    (_, 0) => Some("P".into()),
                    (_, 1) => Some("Compact".into()),
                    (_, 2) => Some("Low power".into()),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        };
        if libc::sched_setaffinity(0, size, &original) != 0 {
            log::error!("Failed to restore gatherer CPU affinity");
        }
        kind
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn cpu_kind(_: u32) -> Option<String> {
    None
}

pub fn matches(info: &Cpu, ids: &[u32]) -> bool {
    info.logical_cpus
        .iter()
        .map(|cpu| cpu.id)
        .eq(ids.iter().copied())
}
