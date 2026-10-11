/* src/disks/manager.rs
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

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use arrayvec::ArrayString;
use glob::glob;
use tokio::runtime::Handle;
use zbus::zvariant::Value;

use magpie_platform::disks::{
    DiskSmartData, DisksResponseErrorBlocker, DisksResponseErrorEjectFailed,
};
use magpie_platform::processes::Process;
use magpie_platform::Mutex;

use crate::disks::smart_data::smart_data;
use crate::disks::util;
use crate::sync;
use crate::util::{async_runtime, system_bus};

pub struct DisksManager {
    udisks2: Option<udisks2::Client>,
}

impl magpie_platform::disks::DisksManager for DisksManager {
    fn new() -> Self
    where
        Self: Sized,
    {
        let bus = match system_bus() {
            Some(bus) => bus.clone(),
            None => return Self { udisks2: None },
        };

        let rt = async_runtime();
        let udisks2 = match sync!(rt, udisks2::Client::new_for_connection(bus)) {
            Ok(udisks2) => Some(udisks2),
            Err(e) => {
                log::warn!("Failed to connect to udisks2: {}", e);
                None
            }
        };

        Self { udisks2 }
    }

    fn eject(
        &self,
        id: &str,
        processes: &Mutex<HashMap<u32, Process>>,
    ) -> Result<(), DisksResponseErrorEjectFailed> {
        let udisks2 = match &self.udisks2 {
            Some(udisks2) => udisks2,
            None => {
                return {
                    log::error!("No udisks2 client available while trying to eject {id}");
                    Ok(())
                }
            }
        };

        let rt = async_runtime();

        let object = match udisks2.object(format!("/org/freedesktop/UDisks2/block_devices/{id}",)) {
            Ok(object) => object,
            Err(e) => {
                log::error!("Failed to find block object {id}: {e}",);
                return Ok(());
            }
        };

        let block = match sync!(rt, object.block()) {
            Ok(block) => block,
            Err(e) => {
                log::error!("Failed to find block {id}: {e}",);
                return Ok(());
            }
        };

        let drive = match sync!(rt, udisks2.drive_for_block(&block)) {
            Ok(drive) => drive,
            Err(e) => {
                log::error!("Failed to find drive for {id}: {e}");
                return Ok(());
            }
        };

        let part_paths = match glob(&format!("/sys/block/{id}/{id}*")) {
            Ok(paths) => paths,
            Err(e) => {
                log::error!("Failed to find partition paths for {id}: {e}");
                return Ok(());
            }
        }
        .filter_map(Result::ok);

        let mut auth_no_user_interaction = HashMap::new();
        auth_no_user_interaction.insert("auth.no_user_interaction", true.into());

        let mut blockers = Vec::new();

        for path in part_paths {
            let Some(id) = path.file_name().map(|f| f.to_string_lossy()) else {
                continue;
            };

            match unmount(
                rt,
                udisks2,
                id.as_ref(),
                &auth_no_user_interaction,
                processes,
            ) {
                Ok(_) => (),
                Err(mut block) => blockers.append(&mut block),
            }
        }
        match unmount(rt, udisks2, id, &auth_no_user_interaction, processes) {
            Ok(_) => (),
            Err(mut block) => blockers.append(&mut block),
        }

        if !blockers.is_empty() {
            return Err(DisksResponseErrorEjectFailed { blockers });
        }

        match sync!(rt, drive.eject(auth_no_user_interaction)) {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Failed to eject {id}: {e}");
                Ok(())
            }
        }
    }

    fn smart_data(&self, id: &str) -> Option<DiskSmartData> {
        let udisks2 = match &self.udisks2 {
            Some(udisks2) => udisks2,
            None => {
                return {
                    log::error!("No udisks2 client available while getting SMART data");
                    None
                }
            }
        };

        let rt = async_runtime();

        let drive_obj = util::object(udisks2, id).and_then(|obj| {
            sync!(rt, util::block(&obj, id)).and_then(|bp| sync!(rt, util::drive(udisks2, &bp, id)))
        })?;
        smart_data(rt, &drive_obj, id)
    }
}

fn unmount(
    rt: &Handle,
    udisks2: &udisks2::Client,
    id: &str,
    options: &HashMap<&str, Value>,
    processes: &Mutex<HashMap<u32, Process>>,
) -> Result<(), Vec<DisksResponseErrorBlocker>> {
    let fsobject = match udisks2.object(format!("/org/freedesktop/UDisks2/block_devices/{id}",)) {
        Ok(object) => object,
        Err(e) => {
            log::error!("Failed to find block device for `{id}`: {e}",);
            return Err(Vec::new());
        }
    };

    // No filesystems, nothing to unmount
    let Ok(fs) = sync!(rt, fsobject.filesystem()) else {
        log::debug!("No filesystem found for `{id}`");
        return Ok(());
    };

    let mountpoints = sync!(rt, fs.mount_points()).unwrap_or_default();
    // No mountpoints, nothing to unmount
    if mountpoints.is_empty() {
        log::debug!("No mountpoints found for `{id}`");
        return Ok(());
    }

    match sync!(rt, fs.unmount(options.clone())) {
        Ok(_) => return Ok(()),
        Err(e) => {
            log::warn!("Failed to unmount `{id}`: {e}",);
        }
    }

    let mountpoints = mountpoints
        .iter()
        .map(|mp| String::from_utf8_lossy(&mp[..mp.len() - 1]))
        .collect::<Vec<_>>();
    let processes = processes.lock().values().cloned().collect::<Vec<_>>();
    let mut blockers = Vec::new();

    for process in &processes {
        let mut path = ArrayString::<256>::new();

        let mut files = Vec::new();
        let mut dirs = Vec::new();

        match write!(&mut path, "/proc/{}/cwd", process.pid) {
            Ok(()) => {
                let path = path.as_str();
                if let Ok(cwd) = Path::new(path).read_link() {
                    if mountpoints
                        .iter()
                        .any(|mp| cwd.starts_with(Path::new(mp.as_ref())))
                    {
                        dirs.push(cwd.to_string_lossy().to_string());
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to format `cwd` path: {e:?}");
                continue;
            }
        }

        path.clear();
        match write!(&mut path, "/proc/{}/fd", process.pid) {
            Ok(()) => {
                let path = path.as_str();
                if let Ok(fd_dir) = Path::new(path).read_dir() {
                    for fd in fd_dir.filter_map(Result::ok) {
                        let Ok(path) = fd.path().read_link() else {
                            continue;
                        };

                        if mountpoints
                            .iter()
                            .any(|mp| path.starts_with(Path::new(mp.as_ref())))
                        {
                            files.push(path.to_string_lossy().to_string());
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to format `fd` path: {e:?}");
                continue;
            }
        }

        if !files.is_empty() || !dirs.is_empty() {
            blockers.push(DisksResponseErrorBlocker {
                pid: process.pid,
                files,
                dirs,
            });
        }
    }

    Err(blockers)
}
