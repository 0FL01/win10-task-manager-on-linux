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
use magpie_platform::memory::{Memory, MemoryDevice};

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
        let Ok(mm_stat) = std::fs::read_to_string(&path) else {
            continue;
        };

        // mm_stat format: orig_data_size compr_data_size mem_used_total mem_limit max_used_memory ...
        let parts: Vec<&str> = mm_stat.split_whitespace().collect();
        if parts.len() >= 3 {
            if let (Ok(orig), Ok(compr), Ok(mem_used)) = (
                parts[0].parse::<u64>(),
                parts[1].parse::<u64>(),
                parts[2].parse::<u64>(),
            ) {
                memory.zram_orig_size += orig;
                memory.zram_compr_size += compr;
                memory.zram_mem_used += mem_used;
            }
        }
    }
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

    let mut cmd_output_str = cmd_output.as_str();
    let mut cmd_output_str_index = 0; // Index for what character we are at in the string
    let mut module_index = 0; // Index for what RAM module we are working with
    let mut speed_fallback = 0; // If CONFIGURED_SPEED_MTS is not available, use SPEED_MTS
    let mut max_devices: u64 = 0;

    while cmd_output_str_index < cmd_output_str.len() {
        let to_parse = cmd_output_str.trim();
        let mem_dev_string = format!("MEMORY_DEVICE_{}_", module_index);
        let mem_dev_str = mem_dev_string.as_str();
        cmd_output_str_index = match to_parse.find(mem_dev_str) {
            None => {
                // prolly overkill
                for line in to_parse[cmd_output_str_index.saturating_sub(mem_dev_string.len())..]
                    .trim()
                    .lines()
                {
                    let mut split = line.trim().split("=");
                    let key = split.next().map_or("", |s| s).trim();
                    let value = split.next().map_or("", |s| s).trim();

                    match key {
                        "MEMORY_ARRAY_NUM_DEVICES" => {
                            max_devices = value.parse::<u64>().map_or(0, |s| s)
                        }
                        other => log::debug!("{}", other),
                    }
                }
                break;
            }
            Some(cmd_output_str_index) => cmd_output_str_index,
        };
        cmd_output_str_index += mem_dev_str.len();
        module_index += 1;
        if cmd_output_str_index < cmd_output_str.len() {
            cmd_output_str = cmd_output_str[cmd_output_str_index..].trim();
        }

        let mut mem_dev = Some(MemoryDevice::default());

        for line in to_parse[cmd_output_str_index..].trim().lines() {
            let mut split = line.trim().split("=");
            let mut key = split.next().map_or("", |s| s).trim();
            let value = split.next().map_or("", |s| s).trim();

            key = key.strip_prefix(mem_dev_str).unwrap_or(key);

            let md = match mem_dev.as_mut() {
                Some(mem_dev) => mem_dev,
                None => {
                    break;
                }
            };

            match key {
                "PRESENT" => {
                    if value == "0" {
                        // Module does not actually exist; drop the module so it is not counted
                        // towards the installed module count and is not used to get values to
                        // display.
                        #[allow(dropping_references)]
                        drop(md);
                        mem_dev = None;
                        break;
                    }
                }
                "SIZE" => md.size = value.parse::<u64>().map_or(0, |s| s),
                "FORM_FACTOR" => md.form_factor = value.to_owned(),
                "LOCATOR" => md.locator = value.to_owned(),
                "BANK_LOCATOR" => md.bank_locator = value.to_owned(),
                "TYPE" => md.ram_type = value.to_owned(),
                "SPEED_MTS" => speed_fallback = value.parse::<u64>().map_or(0, |s| s),
                "CONFIGURED_SPEED_MTS" => md.speed = value.parse::<u64>().map_or(0, |s| s),
                "RANK" => md.rank = value.parse::<u32>().map_or(0, |s| s),
                _ => (), // Ignore unused values and other RAM modules we are not currently parsing
            }
        }

        if let Some(mut mem_dev) = mem_dev {
            if mem_dev.speed == 0 {
                // If CONFIGURED_SPEED_MTS is not available,
                mem_dev.speed = speed_fallback; // then use SPEED_MTS instead
            }
            devices.push(mem_dev);
        }
    }

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
