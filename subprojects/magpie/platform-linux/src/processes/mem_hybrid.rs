/* src/processes/mem_hybrid.rs
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

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Instant;

use arrayvec::{ArrayString, ArrayVec};

use crate::processes::MAX_U32_LEN;
use crate::util::parse_u64;

const MAPS_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct FileMapping {
    path: Arc<str>,
    size: u64,
}

#[derive(Debug, Default)]
pub struct MemoryMaps {
    share_count: HashMap<Arc<str>, u32>,
    file_mappings: HashMap<u32, Vec<FileMapping>>,
    last_maps_walk: Option<Instant>,
}

impl MemoryMaps {
    pub fn refresh(&mut self, pids: &[u32]) {
        let now = Instant::now();

        let mut do_refresh = || {
            let mut share_count: HashMap<Arc<str>, u32> = HashMap::new();
            let mut file_mappings: HashMap<u32, Vec<FileMapping>> = HashMap::new();

            for &pid in pids {
                let mappings = match read_file_mappings(pid) {
                    Some(m) => m,
                    None => continue,
                };
                // Each unique path in this process counts once toward "this
                // PID maps this file" — collapse duplicate mappings of the
                // same file (common for executables: separate text/rodata/
                // data segments all backed by the same path).
                let mut seen: HashSet<*const str> = HashSet::new();
                for m in &mappings {
                    if seen.insert(Arc::as_ptr(&m.path)) {
                        *share_count.entry(Arc::clone(&m.path)).or_insert(0) += 1;
                    }
                }
                file_mappings.insert(pid, mappings);
            }

            self.share_count = share_count;
            self.file_mappings = file_mappings;
        };

        match self.last_maps_walk {
            None => {
                do_refresh();
                self.last_maps_walk = Some(now);
            }
            Some(t) => {
                if now.duration_since(t) >= MAPS_REFRESH_INTERVAL {
                    do_refresh();
                    self.last_maps_walk = Some(now);
                }
            }
        }
    }
}

pub fn hybrid_pss(pid: u32, maps: &MemoryMaps) -> Option<(u64, u64)> {
    let status = read_status(pid)?;
    let share = match maps.file_mappings.get(&pid) {
        Some(file_mappings) => harmonic_avg_share(file_mappings, &maps.share_count),
        None => 1.0,
    };
    let amortized_file = (status.rss_file as f64 / share.max(1.0)) as u64;

    Some((
        status.rss_anon + status.rss_shmem + amortized_file,
        status.vm_swap,
    ))
}

#[derive(Clone, Debug, Default)]
struct ProcStatus {
    rss_anon: u64,
    rss_file: u64,
    rss_shmem: u64,
    vm_swap: u64,
}

fn read_status(pid: u32) -> Option<ProcStatus> {
    let mut status_file = BufReader::new(open_status(pid)?);
    let mut buffer = Vec::with_capacity(64);

    let mut result = ProcStatus::default();
    loop {
        buffer.clear();
        let bytes_read = match status_file.read_until(b'\n', &mut buffer) {
            Ok(n) => n,
            Err(err) => {
                log::error!(
                    "Failed to read line from `status` file for process {}: {}",
                    pid,
                    err,
                );
                return None;
            }
        };

        if bytes_read == 0 {
            break;
        }

        if let Some(rest) = buffer.strip_prefix(b"RssAnon:") {
            result.rss_anon = parse_u64(to_c_buf(rest).as_slice(), 10, "RssAnon")? * 1024;
        } else if let Some(rest) = buffer.strip_prefix(b"RssFile:") {
            result.rss_file = parse_u64(to_c_buf(rest).as_slice(), 10, "RssFile")? * 1024;
        } else if let Some(rest) = buffer.strip_prefix(b"RssShmem:") {
            result.rss_shmem = parse_u64(to_c_buf(rest).as_slice(), 10, "RssShmem")? * 1024;
        } else if let Some(rest) = buffer.strip_prefix(b"VmSwap:") {
            result.vm_swap = parse_u64(to_c_buf(rest).as_slice(), 10, "VmSwap")? * 1024;
        }
    }

    Some(result)
}

fn read_file_mappings(pid: u32) -> Option<Vec<FileMapping>> {
    let mut interned: HashSet<Arc<str>> = HashSet::new();
    let mut maps_file = BufReader::new(open_maps(pid)?);
    let mut buffer = Vec::with_capacity(256);

    let mut result = Vec::new();
    // Each line: "start-end perms offset dev inode  [path]"
    loop {
        buffer.clear();
        let bytes_read = match maps_file.read_until(b'\n', &mut buffer) {
            Ok(n) => n,
            Err(err) => {
                log::error!(
                    "Failed to read line from `maps` file for process {}: {}",
                    pid,
                    err,
                );
                return None;
            }
        };

        if bytes_read == 0 {
            break;
        }

        let mut fields = buffer.splitn(6, u8::is_ascii_whitespace);
        let range = match fields.next() {
            Some(r) => r,
            None => continue,
        };

        // Skip perms / offset / dev / inode.
        for _ in 0..4 {
            if fields.next().is_none() {
                break;
            }
        }

        let path = match fields.next() {
            Some(p) => p.trim_ascii(),
            None => continue, // anonymous mapping — already accounted for by RssAnon
        };

        if path.is_empty() || path.starts_with(b"[") {
            // pseudo-paths like [heap], [stack], [vvar] — anonymous-equivalent
            continue;
        }

        if path.ends_with(b" (deleted)") {
            continue;
        }

        let path = String::from_utf8_lossy(path);
        let path = match interned.get(path.as_ref()) {
            Some(k) => Arc::clone(k),
            None => {
                let arc = Arc::from(path.as_ref());
                interned.insert(Arc::clone(&arc));
                arc
            }
        };

        let dash = match range.iter().position(|byte| *byte == b'-') {
            Some(i) => i,
            None => continue,
        };

        let start = match parse_u64(to_c_buf(&range[..dash]).as_slice(), 16, "maps range start") {
            Some(s) => s,
            None => continue,
        };
        let end = match parse_u64(
            to_c_buf(&range[dash + 1..]).as_slice(),
            16,
            "maps range end",
        ) {
            Some(e) => e,
            None => continue,
        };
        if end <= start {
            continue;
        }

        result.push(FileMapping {
            path,
            size: end - start,
        });
    }

    Some(result)
}

/// Size-weighted harmonic mean of per-file share counts
fn harmonic_avg_share(maps: &[FileMapping], share_count: &HashMap<Arc<str>, u32>) -> f64 {
    let mut total_size: f64 = 0.0;
    let mut weighted_inv: f64 = 0.0;
    for m in maps {
        let share = (*share_count.get(&m.path).unwrap_or(&1)).max(1) as f64;
        let sz = m.size as f64;
        weighted_inv += sz / share;
        total_size += sz;
    }
    if weighted_inv == 0.0 {
        1.0
    } else {
        total_size / weighted_inv
    }
}

fn open_status(pid: u32) -> Option<std::fs::File> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/status".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/status", pid);

    let file = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(err) => {
            log::debug!("Failed to open `status` file for process {}: {}", pid, err,);
            return None;
        }
    };

    Some(file)
}

fn open_maps(pid: u32) -> Option<std::fs::File> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/maps".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/maps", pid);

    let file = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(err) => {
            log::debug!("Failed to open `maps` file for process {}: {}", pid, err,);
            return None;
        }
    };

    Some(file)
}

fn to_c_buf(s: &[u8]) -> ArrayVec<u8, 65> {
    let mut out = ArrayVec::new();
    out.extend(s.trim_ascii_start().iter().cloned().take(64));
    out.push(0);
    out
}
