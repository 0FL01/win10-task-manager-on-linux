/* src/processes/stat.rs
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
use magpie_platform::processes::ProcessState;

use super::MAX_U32_LEN;

const PROC_PID_STAT_TCOMM: usize = 1;
const PROC_PID_STAT_STATE: usize = 2;
const PROC_PID_STAT_PPID: usize = 3;
const PROC_PID_STAT_UTIME: usize = 13;
const PROC_PID_STAT_STIME: usize = 14;
const PROC_PID_STAT_STARTTIME: usize = 21;

pub fn open(pid: u32) -> Option<std::fs::File> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/stat".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/stat", pid);

    let stat_file = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!(
                "Failed to open `stat` file for process {}, skipping: {}",
                pid,
                e,
            );
            return None;
        }
    };

    Some(stat_file)
}

pub fn parse_stat_file(stat_file_content: &str) -> Option<[&str; 52]> {
    let mut output = [""; 52];

    // The comm field is wrapped in parens but its contents may itself contain
    // parens (kernel allows it in task_struct->comm). Anchor on the first '('
    // and the last ')' to extract the comm verbatim, then split the remainder
    // on whitespace.
    let first_paren = stat_file_content.find('(')?;
    let last_paren = stat_file_content.rfind(')')?;
    if last_paren <= first_paren {
        return None;
    }

    output[0] = stat_file_content[..first_paren].trim();
    output[1] = &stat_file_content[first_paren + 1..last_paren];

    let rest = &stat_file_content[last_paren + 1..];
    for (part_index, entry) in (2..).zip(rest.split_whitespace()).take(output.len() - 2) {
        output[part_index] = entry;
    }

    Some(output)
}

pub fn name<'a>(stat: &[&'a str; 52]) -> &'a str {
    stat[PROC_PID_STAT_TCOMM]
}

pub fn state(stat: &[&str; 52]) -> ProcessState {
    match stat[PROC_PID_STAT_STATE] {
        "R" => ProcessState::Running,
        "S" => ProcessState::Sleeping,
        "D" => ProcessState::SleepingUninterruptible,
        "Z" => ProcessState::Zombie,
        "T" => ProcessState::Stopped,
        "t" => ProcessState::Tracing,
        "X" | "x" => ProcessState::Dead,
        "K" => ProcessState::WakeKill,
        "W" => ProcessState::Waking,
        "P" => ProcessState::Parked,
        _ => ProcessState::Unknown,
    }
}

pub fn parent_pid(stat: &[&str; 52]) -> u32 {
    stat[PROC_PID_STAT_PPID].parse::<u32>().unwrap_or(0)
}

pub fn user_mode_jiffies(stat: &[&str; 52]) -> u64 {
    stat[PROC_PID_STAT_UTIME].parse::<u64>().unwrap_or(0)
}

pub fn kernel_mode_jiffies(stat: &[&str; 52]) -> u64 {
    stat[PROC_PID_STAT_STIME].parse::<u64>().unwrap_or(0)
}

pub fn start_time_ticks(stat: &[&str; 52]) -> Option<u64> {
    stat[PROC_PID_STAT_STARTTIME].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic /proc/<pid>/stat line: pid, (comm), state, ppid, then 49 more
    // numeric fields. utime is field 13, stime is field 14.
    const TYPICAL: &str = "1234 (bash) S 1 1234 1234 34816 1234 4194304 \
        345 0 0 0 12 34 0 0 20 0 1 0 5678 4567000 1234 18446744073709551615 \
        94000000000000 94000000010000 140700000000000 0 0 0 65536 3686400 \
        1266761467 0 0 0 17 0 0 0 0 0 0 94000000020000 94000000030000 \
        94000000040000 140700000050000 140700000060000 140700000070000 \
        140700000080000 0";

    #[test]
    fn parses_typical_stat_line() {
        let parsed = parse_stat_file(TYPICAL).expect("should parse");
        assert_eq!(parsed[0], "1234");
        assert_eq!(name(&parsed), "bash");
        assert_eq!(state(&parsed), ProcessState::Sleeping);
        assert_eq!(parent_pid(&parsed), 1);
        assert_eq!(user_mode_jiffies(&parsed), 12);
        assert_eq!(kernel_mode_jiffies(&parsed), 34);
        assert_eq!(start_time_ticks(&parsed), Some(5678));
    }

    #[test]
    fn parses_name_with_spaces() {
        let line = "4242 (Web Content) R 1 4242 4242 0 -1 0 0 0 0 0 99 1 \
            0 0 20 0 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 \
            0 0 0 0 0 0 0";
        let parsed = parse_stat_file(line).expect("should parse");
        assert_eq!(name(&parsed), "Web Content");
        assert_eq!(state(&parsed), ProcessState::Running);
        assert_eq!(user_mode_jiffies(&parsed), 99);
        assert_eq!(kernel_mode_jiffies(&parsed), 1);
    }

    #[test]
    fn parses_name_with_nested_parens() {
        // Kernel permits parens inside tcomm; anchoring on the last ')' is
        // required to extract the comm verbatim.
        let line = "9 (weird (name) here) S 1 9 9 0 -1 0 0 0 0 0 7 8 \
            0 0 20 0 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 \
            0 0 0 0 0 0 0";
        let parsed = parse_stat_file(line).expect("should parse");
        assert_eq!(name(&parsed), "weird (name) here");
        assert_eq!(parent_pid(&parsed), 1);
        assert_eq!(user_mode_jiffies(&parsed), 7);
        assert_eq!(kernel_mode_jiffies(&parsed), 8);
    }

    #[test]
    fn maps_all_state_codes() {
        let cases = [
            ("R", ProcessState::Running),
            ("S", ProcessState::Sleeping),
            ("D", ProcessState::SleepingUninterruptible),
            ("Z", ProcessState::Zombie),
            ("T", ProcessState::Stopped),
            ("t", ProcessState::Tracing),
            ("X", ProcessState::Dead),
            ("x", ProcessState::Dead),
            ("K", ProcessState::WakeKill),
            ("W", ProcessState::Waking),
            ("P", ProcessState::Parked),
            ("?", ProcessState::Unknown),
        ];
        for (code, expected) in cases {
            let mut parsed = [""; 52];
            parsed[2] = code;
            assert_eq!(state(&parsed), expected, "state code {code:?}");
        }
    }

    #[test]
    fn returns_none_for_empty_input() {
        assert!(parse_stat_file("").is_none());
    }

    #[test]
    fn returns_none_when_comm_is_unterminated() {
        assert!(parse_stat_file("1234 (bash S 1 1234").is_none());
    }

    #[test]
    fn returns_none_when_no_comm_present() {
        assert!(parse_stat_file("1234 bash S 1 1234").is_none());
    }
}
