/* src/memory.rs
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

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::process::Command;

use glob::glob;
use magpie_platform::memory::{Memory, MemoryDevice, SwapDevice, ZramStats};

/// Parses the leading `orig_data_size compr_data_size mem_used_total` of a zram `mm_stat`.
fn parse_mm_stat(mm_stat: &str) -> Option<ZramStats> {
    let mut parts = mm_stat.split_whitespace().map(str::parse::<u64>);
    Some(ZramStats {
        orig_data_size: parts.next()?.ok()?,
        compr_data_size: parts.next()?.ok()?,
        mem_used_total: parts.next()?.ok()?,
    })
}

fn read_mm_stat(device: &str) -> Option<ZramStats> {
    parse_mm_stat(&std::fs::read_to_string(format!("/sys/block/{device}/mm_stat")).ok()?)
}

/// Read zram compression statistics from /sys/block/zram*/mm_stat
/// Aggregates stats across all zram devices
fn read_zram_stats(memory: &mut Memory) {
    // Reset zram values (default to 0 if no zram devices found)
    memory.zram_orig_size = 0;
    memory.zram_compr_size = 0;
    memory.zram_mem_used = 0;

    let Ok(paths) = glob("/sys/block/zram*/mm_stat") else {
        return;
    };

    for path in paths.filter_map(Result::ok) {
        let Some(stats) = std::fs::read_to_string(&path)
            .ok()
            .as_deref()
            .and_then(parse_mm_stat)
        else {
            continue;
        };
        memory.zram_orig_size += stats.orig_data_size;
        memory.zram_compr_size += stats.compr_data_size;
        memory.zram_mem_used += stats.mem_used_total;
    }
}

/// Decodes the octal escapes (`\040` for a space) the kernel uses in /proc/swaps names.
fn unescape_swap_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes
            .get(i + 1..i + 4)
            .filter(|digits| bytes[i] == b'\\' && digits.iter().all(|d| (b'0'..=b'7').contains(d)))
            .map(|digits| {
                digits
                    .iter()
                    .fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0'))
            })
            .and_then(|value| u8::try_from(value).ok());
        if let Some(byte) = octal {
            out.push(byte);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses /proc/swaps: `Filename Type Size Used Priority`, sizes in KiB.
fn parse_proc_swaps(swaps: &str) -> Vec<SwapDevice> {
    swaps
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = unescape_swap_name(fields.next()?);
            let _kind = fields.next()?;
            let size = fields.next()?.parse::<u64>().ok()?;
            let used = fields.next()?.parse::<u64>().ok()?;
            let priority = fields.next()?.parse::<i32>().ok()?;
            Some(SwapDevice {
                name,
                size: size.saturating_mul(1024),
                used: used.saturating_mul(1024),
                priority,
                zram: None,
            })
        })
        .collect()
}

/// `zram0` for `/dev/zram0`; `None` for anything that is not a zram block device.
fn zram_device_name(swap_name: &str) -> Option<&str> {
    let device = swap_name.strip_prefix("/dev/")?;
    let index = device.strip_prefix("zram")?;
    (!index.is_empty() && index.bytes().all(|b| b.is_ascii_digit())).then_some(device)
}

fn read_swap_devices(memory: &mut Memory) {
    memory.swap_devices = match std::fs::read_to_string("/proc/swaps") {
        Ok(swaps) => parse_proc_swaps(&swaps),
        Err(e) => {
            log::debug!("Failed to read `/proc/swaps`: {e}");
            Vec::new()
        }
    };
    for device in &mut memory.swap_devices {
        device.zram = zram_device_name(&device.name).and_then(read_mm_stat);
    }
}

fn read_zswap_enabled(memory: &mut Memory) {
    memory.zswap_enabled = std::fs::read_to_string("/sys/module/zswap/parameters/enabled")
        .ok()
        .and_then(|value| match value.trim() {
            "Y" | "y" | "1" => Some(true),
            "N" | "n" | "0" => Some(false),
            _ => None,
        });
}

fn update_zfs_arc_usage(memory: &mut Memory) {
    const ARCSTATS_PATH: &str = "/proc/spl/kstat/zfs/arcstats";

    const LABEL_C_MIN: &str = "c_min ";
    const LABEL_SIZE: &str = "size ";

    const EXPECTED_LABEL_COUNT: u8 = 2;

    fn read_value_from_line(line: &str) -> Option<u64> {
        line.rsplit(' ').next().and_then(|v| {
            v.trim()
                .parse()
                .inspect_err(|err| {
                    log::warn!(
                        "Failed to parse value from '{line}': {err}. Using a default value."
                    );
                })
                .ok()
        })
    }

    let mut arcstats_reader = match OpenOptions::new().read(true).open(ARCSTATS_PATH) {
        Ok(f) => BufReader::new(f),
        Err(err) => {
            log::info!("Failed to open `{ARCSTATS_PATH}`: {err}");
            return;
        }
    };

    let mut mem_used = 0u64;
    let mut mem_min = 0u64;

    let mut line = String::with_capacity(64);
    let mut labels_found = 0u8;
    loop {
        line.clear();
        match arcstats_reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.starts_with(LABEL_C_MIN) {
                    mem_min = read_value_from_line(&line).unwrap_or(0);
                    labels_found += 1;
                }

                if line.starts_with(LABEL_SIZE) {
                    mem_used = read_value_from_line(&line).unwrap_or(0);
                    labels_found += 1;
                }

                if labels_found == EXPECTED_LABEL_COUNT {
                    break;
                }
            }
            Err(err) => {
                log::error!("Failed to read line from `{ARCSTATS_PATH}`: {err}");
                return;
            }
        }
    }

    let arc_used = if labels_found == EXPECTED_LABEL_COUNT {
        mem_used.saturating_sub(mem_min)
    } else {
        log::warn!("Failed to find expected labels in `{ARCSTATS_PATH}`. Using 0 for ARC usage.");
        0
    };

    memory.mem_free = memory.mem_free.saturating_add(arc_used);
    memory.mem_available = memory.mem_available.saturating_add(arc_used);
}

pub struct MemoryCache {
    memory: Memory,
    devices: Vec<MemoryDevice>,
}

fn refresh_memory(memory: &mut Memory) {
    (memory.corrected_errors, memory.uncorrected_errors) =
        crate::hardware::memory_errors(std::path::Path::new("/sys/devices/system/edac/mc"));
    let hwmon = std::path::Path::new("/sys/class/hwmon");
    memory.fclk_mhz = crate::hardware::clock_mhz(hwmon, &["FCLK", "Fabric Clock"]);
    memory.mclk_mhz = crate::hardware::clock_mhz(hwmon, &["MCLK", "Memory Clock"]);
    memory.uclk_mhz = crate::hardware::clock_mhz(hwmon, &["UCLK", "Memory Controller Clock"]);
    memory.voltage_v = crate::hardware::voltage(
        std::path::Path::new("/sys/class/hwmon"),
        &["DRAM", "VDIMM", "VDDRAM", "DDR", "DRAM Voltage"],
    );
    let meminfo = match std::fs::read_to_string("/proc/meminfo") {
        Ok(meminfo) => meminfo,
        Err(e) => {
            log::error!("Failed to read memory information: {e}");
            return;
        }
    };

    for line in meminfo.trim().lines() {
        let mut split = line.split_whitespace();
        let key = split.next().map_or("", |s| s);
        let value = split
            .next()
            .map_or("", |s| s)
            .parse::<u64>()
            .map_or(0, |v| v)
            * 1024;

        match key {
            "MemTotal:" => memory.mem_total = value,
            "MemFree:" => memory.mem_free = value,
            "MemAvailable:" => memory.mem_available = value,
            "Buffers:" => memory.buffers = value,
            "Cached:" => memory.cached = value,
            "SwapCached:" => memory.swap_cached = value,
            "Active:" => memory.active = value,
            "Inactive:" => memory.inactive = value,
            "Active(anon):" => memory.active_anon = value,
            "Inactive(anon):" => memory.inactive_anon = value,
            "Active(file):" => memory.active_file = value,
            "Inactive(file):" => memory.inactive_file = value,
            "Unevictable:" => memory.unevictable = value,
            "Mlocked:" => memory.m_locked = value,
            "SwapTotal:" => memory.swap_total = value,
            "SwapFree:" => memory.swap_free = value,
            // Support both old (ZSwap/ZSwapTotal) and new (Zswap/Zswapped) field names for kernel compatibility
            "Zswap:" | "ZSwap:" => memory.zswap = value,
            "Zswapped:" | "ZSwapTotal:" => memory.zswapped = value,
            "Dirty:" => memory.dirty = value,
            "Writeback:" => memory.writeback = value,
            "AnonPages:" => memory.anon_pages = value,
            "Mapped:" => memory.mapped = value,
            "Shmem:" => memory.sh_mem = value,
            "KReclaimable:" => memory.k_reclaimable = value,
            "Slab:" => memory.slab = value,
            "SReclaimable:" => memory.s_reclaimable = value,
            "SUnreclaim:" => memory.s_unreclaim = value,
            "KernelStack:" => memory.kernel_stack = value,
            "PageTables:" => memory.page_tables = value,
            "SecMemTables:" => memory.sec_page_tables = value,
            "NFS_Unstable:" => memory.nfs_unstable = value,
            "Bounce:" => memory.bounce = value,
            "WritebackTmp:" => memory.writeback_tmp = value,
            "CommitLimit:" => memory.commit_limit = value,
            "Committed_AS:" => memory.committed = value,
            "VmallocTotal:" => memory.vmalloc_total = value,
            "VmallocUsed:" => memory.vmalloc_used = value,
            "VmallocChunk:" => memory.vmalloc_chunk = value,
            "Percpu:" => memory.percpu = value,
            "HardwareCorrupted:" => memory.hardware_corrupted = value,
            "AnonHugePages:" => memory.anon_huge_pages = value,
            "ShmemHugePages:" => memory.shmem_huge_pages = value,
            "ShmemPmdMapped:" => memory.shmem_pmd_mapped = value,
            "FileHugePages:" => memory.file_huge_pages = value,
            "FilePmdMapped:" => memory.file_pmd_mapped = value,
            "CmaTotal:" => memory.cma_total = value,
            "CmaFree:" => memory.cma_free = value,
            "HugePages_Total:" => memory.huge_pages_total = value / 1024,
            "HugePages_Free:" => memory.huge_pages_free = value / 1024,
            "HugePages_Rsvd:" => memory.huge_pages_rsvd = value / 1024,
            "HugePages_Surp:" => memory.huge_pages_surp = value / 1024,
            "Hugepagesize:" => memory.hugepagesize = value,
            "Hugetlb:" => memory.hugetlb = value,
            "DirectMap4k:" => memory.direct_map4k = value,
            "DirectMap2M:" => memory.direct_map2m = value,
            "DirectMap1G:" => memory.direct_map1g = value,
            _ => (),
        }
    }

    read_zram_stats(memory);
    read_swap_devices(memory);
    read_zswap_enabled(memory);
    update_zfs_arc_usage(memory);
}

pub fn read_memory_device_info(devices: &mut Vec<MemoryDevice>) -> u64 {
    let mut cmd = Command::new("udevadm");
    cmd.arg("info")
        .arg("-q")
        .arg("property")
        .arg("-p")
        .arg("/sys/devices/virtual/dmi/id");
    cmd.env_remove("LD_PRELOAD");

    let cmd_output = match cmd.output() {
        Ok(output) => {
            if !output.stderr.is_empty() {
                log::error!(
                    "Failed to read memory device information, host command execution failed: {}",
                    std::str::from_utf8(output.stderr.as_slice()).unwrap_or("Unknown error")
                );
                return 0;
            }

            match std::str::from_utf8(output.stdout.as_slice()) {
                Ok(out) => out.to_owned(),
                Err(err) => {
                    log::error!(
                        "Failed to read memory device information, host command execution failed: {:?}",
                        err
                    );
                    return 0;
                }
            }
        }
        Err(err) => {
            log::error!(
                "Failed to read memory device information, host command execution failed: {:?}",
                err
            );
            return 0;
        }
    };

    parse_memory_devices(&cmd_output, devices)
}

fn parse_memory_devices(properties: &str, devices: &mut Vec<MemoryDevice>) -> u64 {
    use std::collections::BTreeMap;
    let mut slots = BTreeMap::<u32, MemoryDevice>::new();
    let mut max_devices = 0;
    for line in properties.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key == "MEMORY_ARRAY_NUM_DEVICES" {
            max_devices = value.parse().unwrap_or(0);
        }
        let Some((index, field)) = key
            .strip_prefix("MEMORY_DEVICE_")
            .and_then(|s| s.split_once('_'))
        else {
            continue;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        // Bound malformed firmware data before allocating a diagram.
        if index >= 256 {
            continue;
        }
        let module = slots.entry(index).or_insert_with(|| MemoryDevice {
            slot_index: Some(index),
            ..Default::default()
        });
        match field {
            "SIZE" => module.size = value.parse().unwrap_or(0),
            "FORM_FACTOR" => module.form_factor = value.into(),
            "LOCATOR" => module.locator = value.into(),
            "BANK_LOCATOR" => module.bank_locator = value.into(),
            "TYPE" => module.ram_type = value.into(),
            "SPEED_MTS" if module.speed == 0 => module.speed = value.parse().unwrap_or(0),
            "CONFIGURED_SPEED_MTS" => {
                if let Ok(speed) = value.parse::<u64>() {
                    if speed > 0 {
                        module.speed = speed;
                    }
                }
            }
            "RANK" => module.rank = value.parse().unwrap_or(0),
            "MANUFACTURER" => module.manufacturer = Some(value.into()),
            "SERIAL_NUMBER" => module.serial_number = Some(value.into()),
            "PART_NUMBER" => module.part_number = Some(value.trim().into()),
            "CONFIGURED_VOLTAGE" => {
                module.configured_voltage_v = value
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite() && *v > 0. && *v < 5.)
            }
            _ => {}
        }
    }
    devices.extend(slots.into_values());
    max_devices
}

impl magpie_platform::memory::MemoryCache for MemoryCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let mut devices = Vec::new();
        let memory = Memory {
            max_devices: read_memory_device_info(&mut devices),
            ..Memory::default()
        };

        Self { memory, devices }
    }

    fn refresh(&mut self) {
        refresh_memory(&mut self.memory);
    }

    fn memory(&self) -> &Memory {
        &self.memory
    }

    fn devices(&self) -> &[MemoryDevice] {
        &self.devices
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slots_keep_empty_locations_and_do_not_leak_speed_between_modules() {
        let properties="MEMORY_ARRAY_NUM_DEVICES=4\nMEMORY_DEVICE_2_PRESENT=0\nMEMORY_DEVICE_2_LOCATOR=DIMMB1\nMEMORY_DEVICE_0_SIZE=8589934592\nMEMORY_DEVICE_0_SPEED_MTS=3200\nMEMORY_DEVICE_0_CONFIGURED_SPEED_MTS=3600\nMEMORY_DEVICE_0_LOCATOR=DIMMA1\nMEMORY_DEVICE_1_SIZE=8589934592\nMEMORY_DEVICE_1_PART_NUMBER=TEST\n";
        let mut devices = vec![];
        assert_eq!(parse_memory_devices(properties, &mut devices), 4);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].speed, 3600);
        assert_eq!(devices[1].speed, 0);
        assert_eq!(devices[2].size, 0);
        assert_eq!(devices[2].locator, "DIMMB1");
        assert_eq!(devices[2].slot_index, Some(2));
    }

    #[test]
    fn proc_swaps_parses_bytes_priorities_and_escaped_names() {
        let swaps = "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
            /swapfile                               file\t\t33554428\t823708\t\t10\n\
            /dev/zram0                              partition\t15268860\t7450164\t\t100\n\
            /mnt/my\\040swap                          file\t\t1024\t\t0\t\t-2\n";
        let devices = parse_proc_swaps(swaps);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].name, "/swapfile");
        assert_eq!(devices[0].size, 33554428 * 1024);
        assert_eq!(devices[0].used, 823708 * 1024);
        assert_eq!(devices[0].priority, 10);
        assert_eq!(devices[1].name, "/dev/zram0");
        assert_eq!(devices[1].priority, 100);
        assert_eq!(devices[2].name, "/mnt/my swap");
        assert_eq!(devices[2].priority, -2);
    }

    #[test]
    fn proc_swaps_without_areas_or_with_broken_lines_is_empty() {
        assert!(parse_proc_swaps("").is_empty());
        assert!(parse_proc_swaps("Filename Type Size Used Priority\n").is_empty());
        assert!(parse_proc_swaps(
            "Filename Type Size Used Priority\n/swapfile file x 0 -2\n/dev/sda2\n"
        )
        .is_empty());
    }

    #[test]
    fn only_zram_block_devices_get_mm_stat() {
        assert_eq!(zram_device_name("/dev/zram0"), Some("zram0"));
        assert_eq!(zram_device_name("/dev/zram12"), Some("zram12"));
        assert_eq!(zram_device_name("/dev/zram"), None);
        assert_eq!(zram_device_name("/swap/zram0"), None);
        assert_eq!(zram_device_name("/dev/zram0p1"), None);
        assert_eq!(zram_device_name("/swapfile"), None);
    }

    #[test]
    fn mm_stat_reads_leading_three_columns() {
        let stats = parse_mm_stat(
            "7092310016 1900551853 1948770304        0 4198735872    47251  5020964    77521  1390373\n",
        )
        .unwrap();
        assert_eq!(stats.orig_data_size, 7092310016);
        assert_eq!(stats.compr_data_size, 1900551853);
        assert_eq!(stats.mem_used_total, 1948770304);
        assert!(parse_mm_stat("1 2").is_none());
        assert!(parse_mm_stat("1 x 3").is_none());
    }

    #[test]
    fn swap_name_escapes_decode_and_invalid_ones_stay() {
        assert_eq!(unescape_swap_name("/a\\040b\\011c"), "/a b\tc");
        assert_eq!(unescape_swap_name("/a\\9b"), "/a\\9b");
        assert_eq!(unescape_swap_name("/a\\"), "/a\\");
    }
}
