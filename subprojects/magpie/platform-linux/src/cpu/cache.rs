/* src/cpu/cache.rs
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

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use arrayvec::ArrayString;

use crate::read_u64;

pub fn info() -> [Option<u64>; 5] {
    fn as_bytes(mut caches: [Option<u64>; 5]) -> [Option<u64>; 5] {
        for cache in caches.iter_mut().skip(1) {
            *cache = cache.map(|size| size * 1024);
        }
        caches
    }

    let node_entries = match std::fs::read_dir("/sys/devices/system/node/") {
        Ok(entries) => entries,
        Err(err) => {
            log::warn!("Could not read `/sys/devices/system/node`: {err}. Falling back to `/sys/devices/system/cpu`");
            return as_bytes(read_cache_values(Path::new("/sys/devices/system/cpu")));
        }
    };

    let mut node_package_ids = HashMap::new();

    let mut result = [None; 5];
    for nn_entry in node_entries.filter_map(|e| e.ok()) {
        let path = nn_entry.path();
        if !path.is_dir() {
            continue;
        }

        let is_node = path
            .file_name()
            .map(|file| &file.as_bytes()[0..4] == b"node")
            .unwrap_or(false);
        if !is_node {
            continue;
        }

        if let Some(package_id) = find_package_id(&path) {
            if let Some(node_count) = node_package_ids.get_mut(&package_id) {
                *node_count += 1;
            } else {
                node_package_ids.insert(package_id, 1);
            }
        }

        let node_vals = read_cache_values(&path);
        for i in 0..result.len() {
            if let Some(size) = node_vals[i] {
                result[i] = match result[i] {
                    None => Some(size),
                    Some(s) => Some(s + size),
                };
            }
        }
    }

    let nodes_per_package = node_package_ids.values().max().cloned().unwrap_or(1);
    if let Some(l3_size) = result[3] {
        result[3] = Some(l3_size / nodes_per_package as u64);
    }
    if let Some(l4_size) = result[4] {
        result[4] = Some(l4_size / nodes_per_package as u64);
    }

    as_bytes(result)
}

fn read_index_entry_content(file_name: &str, index_path: &Path) -> Option<String> {
    let path = index_path.join(file_name);
    match std::fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(e) => {
            log::warn!("Could not read `{}/{file_name}`: {e}", index_path.display());
            None
        }
    }
}

fn find_package_id(node_path: &Path) -> Option<u16> {
    let cpulist_loc = node_path.join("cpulist");
    let cpulist_loc = cpulist_loc.to_string_lossy();

    let first_cpu_id = read_u64(cpulist_loc.as_ref(), cpulist_loc.as_ref())?;

    let mut phy_pkg_id_path = ArrayString::<256>::new();
    let _ = write!(
        &mut phy_pkg_id_path,
        "/sys/devices/system/cpu/cpu{first_cpu_id}/topology/physical_package_id"
    );
    let phy_pkg_id = read_u64(phy_pkg_id_path.as_str(), phy_pkg_id_path.as_str())?;

    Some(phy_pkg_id as u16)
}

fn read_cache_values(path: &Path) -> [Option<u64>; 5] {
    let mut result = [None; 5];

    let mut l1_visited_data = HashSet::new();
    let mut l1_visited_instr = HashSet::new();
    let mut l2_visited = HashSet::new();
    let mut l3_visited = HashSet::new();
    let mut l4_visited = HashSet::new();

    let cpu_entries = match path.read_dir() {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("Could not read `{}`: {}", path.display(), e);
            return result;
        }
    };
    for cpu_entry in cpu_entries {
        let cpu_entry = match cpu_entry {
            Ok(entry) => entry,
            Err(e) => {
                log::warn!("Could not read cpu entry in `{}`: {e}", path.display());
                continue;
            }
        };
        let mut path = cpu_entry.path();

        let cpu_name = match path.file_name() {
            Some(name) => name,
            None => continue,
        };

        if &cpu_name.as_bytes()[0..3] != b"cpu" {
            continue;
        }

        let cpu_number = match unsafe { std::str::from_utf8_unchecked(&cpu_name.as_bytes()[3..]) }
            .parse::<u16>()
        {
            Ok(n) => n,
            Err(_) => continue,
        };

        path.push("cache");
        let cache_entries = match path.read_dir() {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("Could not read `{}`: {}", path.display(), e);
                return result;
            }
        };
        for cache_entry in cache_entries.filter_map(|e| e.ok()) {
            let path = cache_entry.path();
            let path = path.as_path();

            let is_cache_entry = path
                .file_name()
                .map(|file| &file.as_bytes()[0..5] == b"index")
                .unwrap_or(false);

            if !is_cache_entry {
                continue;
            }
            let level = match read_u64(path.join("level").to_string_lossy().as_ref(), "level index")
                .map(|l| l as u8)
            {
                None => continue,
                Some(l) => l,
            };

            let cache_type = match read_index_entry_content("type", path) {
                None => continue,
                Some(ct) => ct,
            };

            let visited_cpus = match cache_type.trim() {
                "Data" => &mut l1_visited_data,
                "Instruction" => &mut l1_visited_instr,
                "Unified" => match level {
                    2 => &mut l2_visited,
                    3 => &mut l3_visited,
                    4 => &mut l4_visited,
                    _ => continue,
                },
                _ => continue,
            };

            if visited_cpus.contains(&cpu_number) {
                continue;
            }

            let size = match read_u64(path.join("size").to_string_lossy().as_ref(), "size index")
                .map(|l| l as usize)
            {
                None => continue,
                Some(s) => s,
            };

            let result_index = level as usize;
            result[result_index] = match result[result_index] {
                None => Some(size as u64),
                Some(s) => Some(s + size as u64),
            };

            let Some(scl) = read_index_entry_content("shared_cpu_list", path) else {
                continue;
            };
            let shared_cpu_list = scl.trim().split(',');
            for cpu in shared_cpu_list {
                let mut shared_cpu_sequence = cpu.split('-');

                let start = match shared_cpu_sequence.next().map(|s| s.parse::<u16>()) {
                    Some(Ok(s)) => s,
                    Some(Err(_)) | None => continue,
                };

                let end = match shared_cpu_sequence.next().map(|e| e.parse::<u16>()) {
                    Some(Ok(e)) => e,
                    Some(Err(_)) | None => {
                        visited_cpus.insert(start);
                        continue;
                    }
                };

                for i in start..=end {
                    visited_cpus.insert(i);
                }
            }
        }
    }

    result
}
