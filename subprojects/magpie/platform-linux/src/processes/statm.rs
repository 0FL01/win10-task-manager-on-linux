/* src/processes/statm.rs
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

use super::MAX_U32_LEN;

#[allow(dead_code)]
const PROC_PID_STATM_VIRT: usize = 0;
const PROC_PID_STATM_RES: usize = 1;
#[allow(dead_code)]
const PROC_PID_STATM_SHARED: usize = 2;

pub fn open(pid: u32) -> Option<std::fs::File> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/statm".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/statm", pid);

    let statm_file = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("Failed to open `statm` file for process {}: {}", pid, e,);
            return None;
        }
    };

    Some(statm_file)
}

// (pss, shared, swap)
pub fn parse_statm_file(statm_file_content: &str) -> u64 {
    let mut output = [0u64; 7];

    for (part_index, entry) in statm_file_content.split_whitespace().enumerate() {
        output[part_index] = entry.trim().parse::<u64>().unwrap_or_default();
    }

    output[PROC_PID_STATM_RES] * crate::sys_page_size() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resident_pages_to_bytes() {
        // 7 fields: size resident shared text lib data dirty
        let content = "100 50 30 10 0 60 0\n";
        let bytes = parse_statm_file(content);
        assert_eq!(bytes, 50 * crate::sys_page_size() as u64);
    }

    #[test]
    fn empty_input_yields_zero() {
        assert_eq!(parse_statm_file(""), 0);
    }

    #[test]
    fn non_numeric_resident_defaults_to_zero() {
        let content = "100 oops 30 10 0 60 0";
        assert_eq!(parse_statm_file(content), 0);
    }

    #[test]
    fn partial_input_with_resident_present() {
        // Only size + resident provided.
        let content = "100 7";
        assert_eq!(parse_statm_file(content), 7 * crate::sys_page_size() as u64);
    }
}
