/* src/footprint.rs
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

//! Process-wide memory and thread limits.
//!
//! glibc gives threads that allocate concurrently separate malloc arenas (up to eight per
//! core) and keeps freed pages inside them. NNG sizes its pools by core count (16 task, 8
//! expiry and 4 resolver threads on a 16-thread CPU) although the daemon serves one IPC
//! client. Two arenas, minimal NNG pools and periodic trimming keep the resident set close
//! to the live heap.

use std::time::{Duration, Instant};

use crate::nng;

/// `nng_init_set_parameter` ids from `nng.h` 1.11; nng-c-sys does not export them and they
/// are outside NNG's stability promise (see `pinned_nng_matches_parameter_ids`).
const NNG_INIT_NUM_TASK_THREADS: nng::nng_init_parameter = 1;
const NNG_INIT_NUM_EXPIRE_THREADS: nng::nng_init_parameter = 2;
const NNG_INIT_NUM_POLLER_THREADS: nng::nng_init_parameter = 3;
const NNG_INIT_NUM_RESOLVER_THREADS: nng::nng_init_parameter = 4;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MALLOC_ARENAS: libc::c_int = 2;
const TRIM_INTERVAL: Duration = Duration::from_secs(10);

/// Applies the limits. Call first in `main`, before any thread or NNG call.
pub fn configure() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // An explicit allocator setting from the environment wins.
        let tuned = std::env::var_os("MALLOC_ARENA_MAX").is_some()
            || std::env::var("GLIBC_TUNABLES").is_ok_and(|t| t.contains("arena_max"));
        if !tuned {
            unsafe { libc::mallopt(libc::M_ARENA_MAX, MALLOC_ARENAS) };
        }
    }

    // NNG never uses fewer than two task threads; the IPC transport needs no resolver.
    for (parameter, value) in [
        (NNG_INIT_NUM_TASK_THREADS, 2),
        (NNG_INIT_NUM_EXPIRE_THREADS, 1),
        (NNG_INIT_NUM_POLLER_THREADS, 1),
        (NNG_INIT_NUM_RESOLVER_THREADS, 1),
    ] {
        unsafe { nng::nng_init_set_parameter(parameter, value) };
    }
}

/// Returns free heap pages to the system at most once per [`TRIM_INTERVAL`].
pub struct Trimmer {
    last: Instant,
}

impl Trimmer {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
        }
    }

    pub fn tick(&mut self) {
        if self.last.elapsed() < TRIM_INTERVAL {
            return;
        }
        self.last = Instant::now();
        let before = log::log_enabled!(target: LOG_TARGET, log::Level::Debug).then(Usage::read);
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            libc::malloc_trim(0);
        }
        if let Some(before) = before {
            log::debug!(target: LOG_TARGET, "before trim: {before}; after: {}", Usage::read());
        }
    }
}

const LOG_TARGET: &str = "magpie::footprint";

/// Resident memory and allocator state, for the debug log around each trim.
struct Usage {
    /// `RssAnon`, `RssFile` from `/proc/self/status`, KiB
    rss: Option<(u64, u64)>,
    /// Heap bytes in use, free inside the heap, and held in separate mmap chunks
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    heap: (usize, usize, usize),
}

impl Usage {
    fn read() -> Self {
        let rss = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                let kib = |key: &str| {
                    status
                        .lines()
                        .find_map(|line| line.strip_prefix(key))
                        .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
                };
                Some((kib("RssAnon:")?, kib("RssFile:")?))
            });
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        let heap = {
            let info = unsafe { libc::mallinfo2() };
            (info.uordblks, info.fordblks, info.hblkhd)
        };
        Self {
            rss,
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            heap,
        }
    }
}

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.rss {
            Some((anon, file)) => write!(
                f,
                "rss_anon={:.1}MiB rss_file={:.1}MiB",
                anon as f64 / 1024.0,
                file as f64 / 1024.0
            )?,
            None => f.write_str("rss=unavailable")?,
        }
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            const MIB: f64 = 1024.0 * 1024.0;
            let (used, free, mapped) = self.heap;
            write!(
                f,
                " heap_used={:.1}MiB heap_free={:.1}MiB mmapped={:.1}MiB",
                used as f64 / MIB,
                free as f64 / MIB,
                mapped as f64 / MIB
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::*;

    #[test]
    fn pinned_nng_matches_parameter_ids() {
        // Recheck the ids above against `nng.h` before changing the NNG version.
        let version = unsafe { CStr::from_ptr(nng::nng_version()) };
        assert!(
            version.to_str().unwrap().starts_with("1.11."),
            "{version:?}"
        );
    }
}
