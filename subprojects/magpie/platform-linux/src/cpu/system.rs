/* src/cpu/system.rs
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

use std::ffi::CStr;
use std::fmt::Write;
use std::time::Duration;

use arrayvec::ArrayString;

use crate::{for_process_in_proc, read_u64};

pub fn process_count() -> u64 {
    let mut count = 0;
    for_process_in_proc(|_, _| count += 1);

    count
}

pub fn thread_count() -> u64 {
    fn task_path(pid: u32) -> ArrayString<21> {
        const MAX_U32_LEN: usize = 10;
        const MAX_PATH_LEN: usize = "/proc/".len() + "/task".len() + MAX_U32_LEN;

        let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
        let _ = write!(path, "/proc/{}/task", pid);

        path
    }

    let mut result = 0;
    for_process_in_proc(|_, pid| {
        result += match std::fs::read_dir(task_path(pid).as_str()) {
            Ok(tasks) => {
                let mut task_count = 0u64;
                for task in tasks.filter_map(|t| t.ok()) {
                    // TODO: Would using a Regex be faster here?
                    if task.file_name().to_string_lossy().parse::<u32>().is_err() {
                        continue;
                    }
                    task_count += 1;
                }
                task_count
            }
            Err(e) => {
                log::debug!("Failed to read task directory for process {pid}: {e}");
                1
            }
        };
    });

    result
}

pub fn handle_count() -> u64 {
    read_u64("/proc/sys/fs/file-nr", "handle count").unwrap_or_default()
}

pub fn uptime() -> Duration {
    let ts = unsafe {
        let mut ts = std::mem::MaybeUninit::uninit();
        if libc::clock_gettime(libc::CLOCK_BOOTTIME, ts.as_mut_ptr()) == -1 {
            let errno = *libc::__errno_location();
            let error_string = CStr::from_ptr(libc::strerror(errno));
            log::error!("Failed to get system uptime: {:?}", error_string);
            return Duration::from_millis(0);
        }
        ts.assume_init()
    };

    Duration::from_secs(ts.tv_sec as u64) + Duration::from_nanos(ts.tv_nsec as u64)
}
