/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
//! Read registered SPD EEPROMs only. No I2C scanning, driver binding, or bus writes.
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpdEeprom {
    pub source: String,
    pub driver: String,
    pub hex: String,
}

#[cfg(target_os = "linux")]
pub(crate) fn read() -> Result<Vec<SpdEeprom>, String> {
    let mut devices = vec![];
    let entries = std::fs::read_dir("/sys/bus/i2c/devices").map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((bus, address)) = name.split_once('-') else {
            continue;
        };
        if bus.parse::<u32>().is_err() || address.len() != 4 {
            continue;
        }
        let Ok(address) = u16::from_str_radix(address, 16) else {
            continue;
        };
        if !(0x50..=0x57).contains(&address) {
            continue;
        }
        let driver = std::fs::read_link(path.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()));
        let Some(driver) = driver.filter(|s| matches!(s.as_str(), "eeprom" | "ee1004" | "spd5118"))
        else {
            continue;
        };
        if devices.len() == 32 {
            break;
        }
        let mut bytes = vec![];
        if std::fs::File::open(path.join("eeprom"))
            .and_then(|f| f.take(1024).read_to_end(&mut bytes))
            .is_err()
        {
            continue;
        }
        if bytes.len() < 128 {
            continue;
        }
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
        devices.push(SpdEeprom {
            source: name,
            driver,
            hex,
        });
    }
    devices.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(devices)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn read() -> Result<Vec<SpdEeprom>, String> {
    Err("SPD reading is available on Linux.".into())
}
