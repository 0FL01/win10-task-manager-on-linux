/* src/cpu/virt.rs
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

use std::fs::File;
use std::io::Read;
use std::path::Path;

use zbus::Proxy;

use crate::{async_runtime, sync, system_bus};

pub fn technology() -> Option<String> {
    let mut result: Option<String> = None;

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    match std::fs::read_to_string("/proc/cpuinfo") {
        Ok(cpuinfo) => {
            for line in cpuinfo.split('\n').map(|l| l.trim()) {
                if !line.starts_with("flags") {
                    continue;
                }

                for flag in line.split(':').nth(1).unwrap_or("").trim().split(' ') {
                    if flag == "vmx" {
                        result = Some("Intel VT-x".into());
                        break;
                    }

                    if flag == "svm" {
                        result = Some("AMD-V".into());
                        break;
                    }
                }

                break;
            }
        }
        Err(e) => {
            log::warn!("Failed to read virtualization capabilities from `/proc/cpuinfo`: {e}");
        }
    }

    if Path::new("/dev/kvm").exists() {
        result = if let Some(virt) = result {
            Some(format!("KVM / {}", virt))
        } else {
            Some("KVM".into())
        };
    } else {
        log::debug!("Virtualization: `/dev/kvm` does not exist");
    }

    let mut buffer = [0u8; 9];
    match File::open("/proc/xen/capabilities") {
        Ok(mut file) => {
            if file
                .read(&mut buffer)
                .inspect_err(|err| {
                    log::error!(
                        "Virtualization: Failed to read from `/proc/xen/capabilities`: {err}"
                    );
                })
                .is_ok()
                && &buffer == b"control_d"
            {
                result = if let Some(virt) = result {
                    if virt.starts_with("KVM") {
                        Some(format!("KVM & Xen / {}", virt))
                    } else {
                        Some(format!("Xen / {}", virt))
                    }
                } else {
                    Some("Xen".into())
                };
            }
        }
        Err(e) => {
            log::debug!("Virtualization: Failed to open /proc/xen/capabilities: {e}");
        }
    }

    result
}

pub fn is_virtual_machine() -> Option<bool> {
    let bus = match system_bus() {
        Some(c) => c,
        None => {
            log::warn!("Failed to determine VM: Failed to connect to DBus system bus");
            return None;
        }
    };

    let rt = async_runtime();
    let proxy = match sync!(
        rt,
        Proxy::new(
            bus,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager"
        )
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to determine VM: Failed to create SystemD proxy: {e}");
            return None;
        }
    };

    match sync!(rt, proxy.get_property::<String>("Virtualization")) {
        Ok(virt) => Some(!virt.is_empty()),
        Err(e) => {
            log::warn!("Failed to determine VM: Failed to get Virtualization property: {e}");
            None
        }
    }
}
