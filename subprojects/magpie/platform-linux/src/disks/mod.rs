/* src/disks/mod.rs
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

use phf::phf_set;
use uucore::fsext::read_fs_list;

use magpie_platform::disks::Disk;

use crate::disks::disk_wrapper::DiskWrapper;
use crate::{async_runtime, sync, system_bus};

pub use manager::DisksManager;

mod disk_wrapper;
mod manager;
mod smart_data;
mod stats;
mod util;

static IGNORED_DISK_PREFIXES: phf::Set<&'static str> = phf_set! {
    "loop",
    "ram",
    "zram",
    "fd",
    "md",
    "dm",
    "zd",
};

pub struct DisksCache {
    ignored: HashSet<String>,

    udisks2: Option<udisks2::Client>,

    disks: Vec<Disk>,
    disk_wrappers: HashMap<String, DiskWrapper>,
}

impl magpie_platform::disks::DisksCache for DisksCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let bus = match system_bus() {
            Some(bus) => bus.clone(),
            None => {
                log::warn!("Failed to connect to system bus");
                return Self {
                    ignored: HashSet::new(),
                    udisks2: None,
                    disks: Default::default(),
                    disk_wrappers: HashMap::new(),
                };
            }
        };

        let rt = async_runtime();
        let udisks2 = match sync!(rt, udisks2::Client::new_for_connection(bus)) {
            Ok(udisks2) => Some(udisks2),
            Err(e) => {
                log::warn!("Failed to connect to udisks2: {}", e);
                None
            }
        };

        Self {
            ignored: HashSet::new(),
            udisks2,
            disks: Default::default(),
            disk_wrappers: HashMap::new(),
        }
    }

    fn refresh(&mut self) {
        let udisks2 = self.udisks2.as_ref();
        let rt = async_runtime();

        let dir = match std::fs::read_dir("/sys/block") {
            Ok(dir) => dir,
            Err(e) => {
                log::warn!("Failed to read `/sys/block`: {e}");
                return;
            }
        };

        // we could in theory re-implement read_fs_list here
        let fs_list = read_fs_list().ok().map(|mut mis| {
            let mut fs_list: HashMap<String, Vec<_>> = HashMap::with_capacity(mis.len());
            for mi in mis
                .drain(..)
                .filter(|mi| !mi.dummy && !mi.remote && mi.dev_name.starts_with("/dev"))
            {
                let key = std::fs::canonicalize(&mi.dev_name)
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    // if ANYTHING goes wrong lets forget about this whole misadventure
                    .unwrap_or_else(|| mi.dev_name.clone());
                fs_list.entry(key).or_default().push(mi);
            }
            fs_list
        });

        let mut new_wrappers: HashMap<_, _> = Default::default();

        'outer: for entry in dir.filter_map(Result::ok) {
            let file_name = entry.file_name();
            let disk_id = file_name.to_string_lossy();
            let disk_id = disk_id.as_ref();

            if self.ignored.contains(disk_id) {
                continue;
            }

            for i in 2..=disk_id.len().min(4) {
                if IGNORED_DISK_PREFIXES.contains(&disk_id[..i]) {
                    self.ignored.insert(disk_id.to_string());
                    continue 'outer;
                }
            }

            let mut disk_wrapper = match self.disk_wrappers.remove(disk_id) {
                Some(d) => d,
                None => {
                    if let Some((udisks2, object)) = udisks2
                        .and_then(|client| util::object(client, disk_id).map(|obj| (client, obj)))
                    {
                        sync!(rt, DiskWrapper::new(disk_id, udisks2, object))
                    } else {
                        continue;
                    }
                }
            };

            disk_wrapper.update_stats();

            sync!(rt, disk_wrapper.update_disk_obj(udisks2));

            if let Some(fs_list) = fs_list.as_ref() {
                disk_wrapper.update_partitions(fs_list);
            }

            new_wrappers.insert(disk_id.to_string(), disk_wrapper);
        }

        self.disk_wrappers = new_wrappers;

        self.disks = self
            .disk_wrappers
            .values()
            .map(|v| v.disk.clone())
            .collect();
    }

    fn cached_entries(&self) -> &[Disk] {
        &self.disks
    }
}

#[cfg(test)]
mod tests {
    use magpie_platform::disks::DisksCache;

    #[test]
    fn test_disks_cache() {
        let mut cache = super::DisksCache::new();
        cache.refresh();
    }
}
