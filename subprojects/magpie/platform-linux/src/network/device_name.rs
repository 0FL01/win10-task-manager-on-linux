/* src/network/device_name.rs
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
use std::fs::File;
use std::io::Read;
use std::path::Path;

use rusqlite::OpenFlags;

use crate::hardware_db;

pub fn device_name(device_name_cache: &mut HashMap<String, String>, udi: &str) -> Option<String> {
    if let Some(device_name) = device_name_cache.get(udi) {
        return Some(device_name.clone());
    }

    let hardware_db = hardware_db();

    let conn = match rusqlite::Connection::open_with_flags(
        hardware_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to open hardware database: {e}");
            return None;
        }
    };

    let mut stmt = match conn.prepare("SELECT value FROM key_len WHERE key = 'min'") {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to extract min key length: Prepare query failed: {e}");
            return None;
        }
    };
    let mut query_result = match stmt.query_map([], |row| row.get::<usize, i32>(0)) {
        Ok(qr) => qr,
        Err(e) => {
            log::warn!("Failed to extract min key length: Query map failed: {e}");
            return None;
        }
    };
    let min_key_len = if let Some(min_len) = query_result.next() {
        min_len.unwrap_or(0)
    } else {
        0
    };

    let mut stmt = match conn.prepare("SELECT value FROM key_len WHERE key = 'max'") {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to extract max key length: Prepare query failed: {e}");
            return None;
        }
    };
    let mut query_result = match stmt.query_map([], |row| row.get::<usize, i32>(0)) {
        Ok(qr) => qr,
        Err(e) => {
            log::warn!("Failed to extract max key length: Query map failed: {e}");
            return None;
        }
    };
    let mut max_key_len = if let Some(max_len) = query_result.next() {
        max_len.unwrap_or(i32::MAX)
    } else {
        i32::MAX
    };

    let device_id = format!("{}/device", udi);
    let mut sys_device_path = Path::new(&device_id);
    let mut modalias = String::with_capacity(64);
    for _ in 0..4 {
        if let Some(p) = sys_device_path.parent() {
            sys_device_path = p;
        } else {
            break;
        }

        let modalias_path = sys_device_path.join("modalias");

        let mut modalias_file = match File::open(modalias_path) {
            Ok(f) => f,
            Err(_) => {
                continue;
            }
        };

        modalias.clear();
        if modalias_file.read_to_string(&mut modalias).is_err() {
            continue;
        }
        modalias = modalias.trim().to_owned();
        if max_key_len == i32::MAX {
            max_key_len = modalias.len() as i32;
        }

        for i in (min_key_len..max_key_len).rev() {
            modalias.truncate(i as usize);
            let mut stmt =
                match conn.prepare("SELECT value FROM models WHERE key LIKE ?1 || '%' LIMIT 1") {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("Failed to find model: Prepare query failed: {e}");
                        continue;
                    }
                };
            let mut query_result =
                match stmt.query_map([modalias.trim()], |row| row.get::<usize, String>(0)) {
                    Ok(qr) => qr,
                    Err(e) => {
                        log::warn!("Failed to find model: Query map failed: {e}");
                        continue;
                    }
                };

            let model_name = if let Some(model) = query_result.next() {
                model.ok()
            } else {
                None
            };

            if let Some(model_name) = model_name {
                device_name_cache.insert(udi.to_owned(), model_name.clone());
                return Some(model_name);
            }
        }
    }

    None
}
