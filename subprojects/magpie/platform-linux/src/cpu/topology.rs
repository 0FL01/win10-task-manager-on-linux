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
        let kind = cpuid_kind(|leaf, subleaf| {
            if subleaf == 0 {
                __cpuid(leaf)
            } else {
                __cpuid_count(leaf, subleaf)
            }
        });
        if libc::sched_setaffinity(0, size, &original) != 0 {
            log::error!("Failed to restore gatherer CPU affinity");
        }
        kind.map(str::to_owned)
    }
}

#[cfg(target_arch = "x86_64")]
fn cpuid_kind(read: impl Fn(u32, u32) -> std::arch::x86_64::CpuidResult) -> Option<&'static str> {
    let basic = read(0, 0);
    if basic.ebx == 0x756e6547
        && basic.edx == 0x49656e69
        && basic.ecx == 0x6c65746e
        && basic.eax >= 0x1a
    {
        match read(0x1a, 0).eax >> 24 {
            0x40 => Some("P"),
            0x20 => Some("E"),
            _ => None,
        }
    } else if basic.ebx == 0x68747541
        && basic.edx == 0x69746e65
        && basic.ecx == 0x444d4163
        && read(0x80000000, 0).eax >= 0x80000026
    {
        let topology = read(0x80000026, 0);
        // AMD extended topology explicitly advertises heterogeneous cores in EAX[30].
        // Core type is EBX[31:28], not the preferred-core ranking or clock speed.
        if topology.eax & (1 << 30) != 0 && topology.ebx & 0xffff != 0 {
            let signature = read(1, 0).eax;
            let family = ((signature >> 8) & 0xf) + ((signature >> 20) & 0xff);
            match (family, topology.ebx >> 28) {
                (0x19, 0) => Some("Zen 4"),
                (0x19, 1) => Some("Zen 4c"),
                (0x1a, 0) => Some("Zen 5"),
                (0x1a, 1) => Some("Zen 5c"),
                (_, 0) => Some("P"),
                (_, 1) => Some("Compact"),
                (_, 2) => Some("Low power"),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_cpu_ids_and_same_count_replacements_require_fresh_topology() {
        let info = Cpu {
            logical_cpus: [0, 4, 12]
                .into_iter()
                .map(|id| LogicalCpu {
                    id,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        assert!(matches(&info, &[0, 4, 12]));
        assert!(!matches(&info, &[0, 4, 13]));
        assert!(!matches(&info, &[0, 12, 4]));
        assert!(!matches(&info, &[0, 4]));
    }

    #[test]
    fn unavailable_cpu_topology_keeps_the_kernel_id_without_guessing_kind() {
        let cpu = read(u32::MAX);
        assert_eq!(cpu.id, u32::MAX);
        assert_eq!(cpu.package_id, None);
        assert_eq!(cpu.core_id, None);
        assert_eq!(cpu.kind, None);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn unsupported_cpuid_leaves_and_core_types_remain_unknown() {
        use std::arch::x86_64::CpuidResult;
        let intel = CpuidResult {
            eax: 0x19,
            ebx: 0x756e6547,
            ecx: 0x6c65746e,
            edx: 0x49656e69,
        };
        assert_eq!(
            cpuid_kind(|leaf, _| {
                assert_eq!(leaf, 0, "unsupported leaf must not be queried");
                intel
            }),
            None
        );
        for (core_type, expected) in [
            (0, None),
            (0x30, None),
            (0x40, Some("P")),
            (0x20, Some("E")),
        ] {
            assert_eq!(
                cpuid_kind(|leaf, _| match leaf {
                    0 => CpuidResult { eax: 0x1a, ..intel },
                    0x1a => CpuidResult {
                        eax: core_type << 24,
                        ebx: 0,
                        ecx: 0,
                        edx: 0
                    },
                    _ => panic!("unexpected CPUID leaf"),
                }),
                expected
            );
        }
        assert_eq!(
            cpuid_kind(|leaf, _| {
                assert_eq!(leaf, 0);
                CpuidResult {
                    eax: 0,
                    ebx: 0,
                    ecx: 0,
                    edx: 0,
                }
            }),
            None
        );
    }
}
