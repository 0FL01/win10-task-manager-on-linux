/* src/processes/io.rs
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

const PROC_PID_IO_READ_BYTES: usize = 4;
const PROC_PID_IO_WRITE_BYTES: usize = 5;

pub fn open(pid: u32) -> Option<std::fs::File> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/io".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/io", pid);

    let io_file = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("Failed to open `io` file for process {}: {}", pid, e,);
            return None;
        }
    };

    Some(io_file)
}

pub fn parse_io_file(io_file_content: &str) -> [u64; 7] {
    let mut output = [0u64; 7];

    for (part_index, entry) in io_file_content.lines().enumerate() {
        let entry = entry.split_whitespace().last().unwrap_or_default();
        output[part_index] = entry.trim().parse::<u64>().unwrap_or_default();
    }

    output
}

pub fn read_bytes(io_parsed: &[u64; 7]) -> u64 {
    io_parsed[PROC_PID_IO_READ_BYTES]
}

pub fn write_bytes(io_parsed: &[u64; 7]) -> u64 {
    io_parsed[PROC_PID_IO_WRITE_BYTES]
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic /proc/<pid>/io content: 7 lines, last token of each line is
    // the value. read_bytes is line 5 (index 4), write_bytes line 6 (index 5).
    const TYPICAL: &str = "rchar: 1000\n\
                           wchar: 2000\n\
                           syscr: 30\n\
                           syscw: 40\n\
                           read_bytes: 4096\n\
                           write_bytes: 8192\n\
                           cancelled_write_bytes: 0\n";

    #[test]
    fn parses_typical_io_file() {
        let parsed = parse_io_file(TYPICAL);
        assert_eq!(parsed, [1000, 2000, 30, 40, 4096, 8192, 0]);
        assert_eq!(read_bytes(&parsed), 4096);
        assert_eq!(write_bytes(&parsed), 8192);
    }

    #[test]
    fn empty_input_yields_zeros() {
        let parsed = parse_io_file("");
        assert_eq!(parsed, [0; 7]);
        assert_eq!(read_bytes(&parsed), 0);
        assert_eq!(write_bytes(&parsed), 0);
    }

    #[test]
    fn non_numeric_tail_token_defaults_to_zero() {
        let content = "rchar: 1000\n\
                       wchar: bogus\n\
                       syscr: 30\n\
                       syscw: 40\n\
                       read_bytes: 4096\n\
                       write_bytes: 8192\n\
                       cancelled_write_bytes: 0\n";
        let parsed = parse_io_file(content);
        assert_eq!(parsed[1], 0);
        assert_eq!(read_bytes(&parsed), 4096);
        assert_eq!(write_bytes(&parsed), 8192);
    }
}
