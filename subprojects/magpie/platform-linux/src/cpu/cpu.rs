/* src/cpu/cpu.rs
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

use std::collections::HashSet;
use std::sync::OnceLock;

use sysinfo::{CpuRefreshKind, RefreshKind, System};

use crate::read_u64;

pub fn name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let s = System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
        );
        s.cpus()
            .iter()
            .next()
            .map(|cpu| cpu.brand().replace("(R)", "®").replace("(TM)", "™"))
            .unwrap_or_default()
    })
    .as_str()
}

pub fn socket_count() -> Option<u32> {
    let mut sockets = HashSet::new();
    sockets.reserve(4);

    let entries = match std::fs::read_dir("/sys/devices/system/cpu/") {
        Ok(entries) => entries,
        Err(e) => {
            log::error!("Could not read `/sys/devices/system/cpu`: {e}");
            return None;
        }
    };

    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let package_id_path = entry.path().join("topology/physical_package_id");
        let package_id_path = package_id_path.as_os_str().to_string_lossy();

        match read_u64(package_id_path.as_ref(), "package id") {
            Some(socket_id) => {
                sockets.insert(socket_id);
            }
            None => continue,
        }
    }

    if sockets.is_empty() {
        log::warn!("Could not determine socket count");
        None
    } else {
        Some(sockets.len() as u32)
    }
}

pub fn temperature() -> Option<f32> {
    let dir = match std::fs::read_dir("/sys/class/hwmon") {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Failed to open `/sys/class/hwmon`: {e}");
            return None;
        }
    };

    for mut entry in dir
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|path| path.is_dir())
    {
        let mut name = entry.clone();
        name.push("name");

        let name = match std::fs::read_to_string(name) {
            Ok(name) => name.trim().to_ascii_lowercase(),
            Err(_) => continue,
        };
        if !["k10temp", "coretemp", "zenpower"].contains(&name.as_str()) {
            continue;
        }

        entry.push("temp1_input");
        let temp = match std::fs::read_to_string(&entry) {
            Ok(temp) => temp,
            Err(e) => {
                log::warn!("Failed to read temperature from `{}`: {e}", entry.display());
                continue;
            }
        };

        return Some(match temp.trim().parse::<u32>() {
            Ok(temp) => (temp as f32) / 1000.,
            Err(e) => {
                log::warn!(
                    "Failed to parse temperature from `{}`: {e}",
                    entry.display()
                );
                continue;
            }
        });
    }

    None
}
