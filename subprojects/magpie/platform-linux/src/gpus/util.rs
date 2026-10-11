/* src/gpus/util.rs
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

use std::io::Read;
use std::path::Path;

fn read_attribute<'a>(device: &Path, attribute: &str, buffer: &'a mut String) -> Option<&'a str> {
    buffer.clear();

    let path = device.join(attribute);
    let mut file = match std::fs::OpenOptions::new().read(true).open(&path) {
        Ok(file) => file,
        Err(e) => {
            log::debug!("Failed to open {}: {}", path.display(), e);
            return None;
        }
    };

    if let Err(e) = file.read_to_string(buffer) {
        log::debug!("Failed to read {}: {}", path.display(), e);
        return None;
    }

    Some(buffer.trim())
}

pub fn shared_mem_used(device: &Path, buffer: &mut String) -> Option<u64> {
    read_attribute(device, "mem_info_gtt_used", buffer)?
        .parse()
        .ok()
}

pub fn shared_mem_total(device: &Path, buffer: &mut String) -> Option<u64> {
    read_attribute(device, "mem_info_gtt_total", buffer)?
        .parse()
        .ok()
}

pub fn pcie_speed(device: &Path, buffer: &mut String) -> Option<u32> {
    pcie_gen(read_attribute(device, "current_link_speed", buffer)?)
}

pub fn max_pcie_speed(device: &Path, buffer: &mut String) -> Option<u32> {
    pcie_gen(read_attribute(device, "max_link_speed", buffer)?)
}

pub fn pcie_width(device: &Path, buffer: &mut String) -> Option<u32> {
    link_width(read_attribute(device, "current_link_width", buffer)?)
}

pub fn max_pcie_width(device: &Path, buffer: &mut String) -> Option<u32> {
    link_width(read_attribute(device, "max_link_width", buffer)?)
}

pub fn ven_dev_ids(device: &Path, buffer: &mut String) -> Option<(u32, u32)> {
    let mut ids = read_attribute(device, "uevent", buffer)?
        .lines()
        .find_map(|line| line.trim().strip_prefix("PCI_ID="))?
        .split(':');

    let vendor_id = ids.next().and_then(|id| u16::from_str_radix(id, 16).ok())?;
    let device_id = ids.next().and_then(|id| u16::from_str_radix(id, 16).ok())?;

    Some((vendor_id as u32, device_id as u32))
}

fn pcie_gen(link_speed: &str) -> Option<u32> {
    if !link_speed.ends_with(" GT/s PCIe") {
        return None;
    }

    match link_speed.lines().last()?.split_whitespace().next()? {
        "2.5" => Some(1),
        "5.0" => Some(2),
        "8.0" => Some(3),
        "16.0" => Some(4),
        "32.0" => Some(5),
        "64.0" => Some(6),
        _ => None,
    }
}

fn link_width(link_width: &str) -> Option<u32> {
    link_width.lines().last()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_link_speeds_to_pcie_generations() {
        assert_eq!(pcie_gen("2.5 GT/s PCIe"), Some(1));
        assert_eq!(pcie_gen("16.0 GT/s PCIe"), Some(4));
        assert_eq!(pcie_gen("64.0 GT/s PCIe"), Some(6));
    }

    #[test]
    fn rejects_unknown_link_speeds() {
        assert_eq!(pcie_gen("Unknown"), None);
        assert_eq!(pcie_gen("1.0 GT/s PCIe"), None);
        assert_eq!(pcie_gen("16.0 GT/s"), None);
    }

    #[test]
    fn parses_link_width() {
        assert_eq!(link_width("16"), Some(16));
        assert_eq!(link_width("Unknown"), None);
    }
}
