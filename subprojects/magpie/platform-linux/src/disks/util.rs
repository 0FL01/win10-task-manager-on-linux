/* src/disks/util.rs
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

use std::fmt::Write;

use arrayvec::ArrayString;
use glob::glob;
use magpie_platform::disks::{DiskKind, DiskSmartInterfaceKind};
use trim_in_place::TrimInPlace;
use udisks2::ata::AtaProxy;
use udisks2::block::BlockProxy;
use udisks2::drive::{DriveProxy, MediaCompatibility, RotationRate};
use udisks2::nvme::controller::ControllerProxy;
use udisks2::Object;

pub(crate) const MK_TO_0_C: i64 = 273150;

pub fn model(disk_id: &str) -> Option<String> {
    let mut model_dir = ArrayString::<256>::new();

    if let Err(e) = write!(&mut model_dir, "/sys/block/{disk_id}/device/vendor") {
        log::warn!("Failed to format vendor device directory: {e:?}");
        return None;
    };
    let mut vendor = std::fs::read_to_string(model_dir.as_str())
        .ok()
        .and_then(|mut vendor| {
            vendor.trim_in_place();
            if vendor.is_empty() {
                None
            } else {
                Some(vendor)
            }
        });

    model_dir.clear();
    if let Err(e) = write!(&mut model_dir, "/sys/block/{disk_id}/device/model") {
        log::warn!("Failed to format model device directory: {e:?}");
        return None;
    };
    let model = std::fs::read_to_string(model_dir.as_str())
        .ok()
        .and_then(|mut model| {
            model.trim_in_place();
            if model.is_empty() {
                None
            } else {
                Some(model)
            }
        })
        .inspect(|model| {
            let mut has_vendor = false;
            if let Some(vendor) = &vendor {
                has_vendor = model.starts_with(vendor);
            }
            if has_vendor {
                vendor = None;
            }
        });

    match (vendor, model) {
        (Some(vendor), Some(model)) => Some(format!("{} {}", vendor, model)),
        (Some(vendor), None) => Some(vendor),
        (None, Some(model)) => Some(model),
        (None, None) => None,
    }
}

pub async fn kind<'a>(disk_id: &str, drive_proxy: Option<&DriveProxy<'a>>) -> Option<DiskKind> {
    fn get_mmc_kind(device_id: &str) -> Option<DiskKind> {
        let hwmon_idx = device_id[6..].parse::<u64>().ok()?;

        let globs = match glob(&format!(
            "/sys/class/mmc_host/mmc{}/mmc{}*/type",
            hwmon_idx, hwmon_idx
        )) {
            Ok(globs) => globs,
            Err(err) => {
                log::warn!("Failed to read mmc type entry: {err:?}");
                return None;
            }
        };

        let mut res = None;
        for entry in globs {
            let Ok(path) = entry else {
                continue;
            };

            let Ok(kind) = std::fs::read_to_string(&path) else {
                log::error!("Could not read mmc type: {}", path.display());
                continue;
            };

            match kind.trim() {
                "SD" => {
                    res = Some(DiskKind::Sd);
                    break;
                }
                "MMC" => {
                    res = Some(DiskKind::EMmc);
                    break;
                }
                _ => {
                    log::error!("Unknown mmc type: '{kind}'");
                    continue;
                }
            }
        }

        res
    }

    if disk_id.starts_with("nvme") {
        Some(DiskKind::NvMe)
    } else if disk_id.starts_with("mmc") {
        get_mmc_kind(disk_id)
    } else if disk_id.starts_with("fd") {
        Some(DiskKind::Floppy)
    } else if disk_id.starts_with("sr") {
        Some(DiskKind::Optical)
    } else if let Some(proxy) = drive_proxy {
        if let Ok(media_compat) = proxy.media().await {
            match media_compat {
                MediaCompatibility::Thumb => {
                    return Some(DiskKind::ThumbDrive);
                }
                MediaCompatibility::Flash
                | MediaCompatibility::FlashCf
                | MediaCompatibility::FlashMs
                | MediaCompatibility::FlashSd
                | MediaCompatibility::FlashSdhc
                | MediaCompatibility::FlashSdxc
                | MediaCompatibility::FlashMmc
                | MediaCompatibility::FlashSdio
                | MediaCompatibility::FlashSdCombo => {
                    return Some(DiskKind::Sd);
                }
                _ => {}
            }
        }

        let rotation_rate = proxy.rotation_rate().await;
        match rotation_rate {
            Ok(RotationRate::NonRotating) | Ok(RotationRate::Rotating(0)) => Some(DiskKind::Ssd),
            Ok(RotationRate::Rotating(_) | RotationRate::Unknown) => {
                log::debug!("Detected rotating disk: {disk_id}. If this is wrong run: `cat /sys/block/{disk_id}/queue/rotational` to confirm.");
                Some(DiskKind::Hdd)
            }
            _ => None,
        }
    } else {
        None
    }
}

pub fn smart_interface<'a>(
    ata: Option<&AtaProxy<'a>>,
    nvme: Option<&ControllerProxy<'a>>,
) -> Option<DiskSmartInterfaceKind> {
    if ata.is_some() {
        Some(DiskSmartInterfaceKind::Ata)
    } else if nvme.is_some() {
        Some(DiskSmartInterfaceKind::NvMe)
    } else {
        None
    }
}

pub fn object(udisks2: &udisks2::Client, disk_id: &str) -> Option<Object> {
    let mut block_dev_path = ArrayString::<256>::new();
    if let Err(e) = write!(
        &mut block_dev_path,
        "/org/freedesktop/UDisks2/block_devices/{disk_id}"
    ) {
        log::warn!("Failed to format block device DBus path dirs: {e:?}");
        return None;
    };

    match udisks2.object(block_dev_path.as_str()) {
        Ok(object) => Some(object),
        Err(e) => {
            log::warn!("Failed to find block object {disk_id}: {e}",);
            None
        }
    }
}

pub async fn block(object: &Object, disk_id: &str) -> Option<BlockProxy<'static>> {
    let block = match object.block().await {
        Ok(block) => block,
        Err(e) => {
            log::warn!("Failed to find block {disk_id}: {e}",);
            return None;
        }
    };

    Some(block)
}

pub async fn drive(
    udisks2: &udisks2::Client,
    block: &BlockProxy<'_>,
    disk_id: &str,
) -> Option<Object> {
    let drive_path = match block.drive().await {
        Ok(drive) => drive,
        Err(e) => {
            log::error!("Failed to find drive for {disk_id}: {e}",);
            return None;
        }
    };

    let drive_object = match udisks2.object(drive_path) {
        Ok(drive_object) => Some(drive_object),
        Err(e) => {
            log::error!("Failed to find drive object {disk_id}: {e}",);
            return None;
        }
    };

    drive_object
}
