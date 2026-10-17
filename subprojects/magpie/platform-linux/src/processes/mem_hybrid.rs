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

use std::collections::HashMap;
use std::fmt::Write;
use std::io::{BufRead, BufReader};
use std::time::Instant;

use arrayvec::{ArrayString, ArrayVec};
use magpie_platform::profile::{self, Phase, Span};

use crate::processes::MAX_U32_LEN;
use crate::util::parse_proc_u64 as parse_u64;

const MAPS_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct MemoryMaps {
    /// Per PID, the size-weighted harmonic mean of how many processes map each of its files,
    /// from the last maps walk. Only this factor is kept; mappings are dropped after the walk.
    share: HashMap<u32, f64>,
    last_maps_walk: Option<Instant>,
}

impl MemoryMaps {
    pub fn refresh(&mut self, pids: &[u32]) {
        let now = Instant::now();

        let mut do_refresh = || {
            let mut profile = Span::new(Phase::ProcessMaps);
            profile.items(pids.len());
            let mut walk = MapsWalk::default();
            for &pid in pids {
                if let Some(mappings) = read_file_mappings(pid, &mut walk) {
                    walk.add(pid, mappings);
                }
            }
            self.share = walk.finish();
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
                } else {
                    profile::coalesced(Phase::ProcessMaps);
                }
            }
        }
    }
}

/// File mappings of all processes during one maps walk: paths are interned across processes
/// and each mapping keeps only its path id and size.
#[derive(Default)]
struct MapsWalk {
    paths: HashMap<Box<str>, u32>,
    /// Number of processes mapping each path id.
    share_count: Vec<u32>,
    /// Index of the last process counted for each path id, to count a path once per process.
    counted_by: Vec<u32>,
    processes: Vec<(u32, Vec<FileMapping>)>,
}

/// Mapping ranges are page aligned, so sizes are kept in 4 KiB units (at most 16 TiB per
/// entry); the share factor is a ratio of sizes and does not depend on the unit.
const SIZE_UNIT: u64 = 4096;

#[derive(Clone, Copy, Debug)]
struct FileMapping {
    path: u32,
    /// In [`SIZE_UNIT`]s
    size: u32,
}

impl MapsWalk {
    fn intern(&mut self, path: &str) -> u32 {
        if let Some(&id) = self.paths.get(path) {
            return id;
        }
        let id = self.share_count.len() as u32;
        self.paths.insert(path.into(), id);
        self.share_count.push(0);
        self.counted_by.push(u32::MAX);
        id
    }

    fn add(&mut self, pid: u32, mappings: Vec<FileMapping>) {
        // Each unique path in this process counts once toward "this PID maps this file" —
        // collapse duplicate mappings of the same file (common for executables: separate
        // text/rodata/data segments all backed by the same path).
        let index = self.processes.len() as u32;
        for mapping in &mappings {
            let path = mapping.path as usize;
            if self.counted_by[path] != index {
                self.counted_by[path] = index;
                self.share_count[path] += 1;
            }
        }
        self.processes.push((pid, mappings));
    }

    fn finish(self) -> HashMap<u32, f64> {
        self.processes
            .into_iter()
            .map(|(pid, mappings)| (pid, harmonic_avg_share(&mappings, &self.share_count)))
            .collect()
    }
}

pub fn hybrid_pss(pid: u32, maps: &MemoryMaps) -> Option<(u64, u64)> {
    let status = read_status(pid)?;
    let share = maps.share.get(&pid).copied().unwrap_or(1.0);
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
                if !profile::proc_io_error(&err) {
                    log::error!(
                        "Failed to read line from `status` file for process {}: {}",
                        pid,
                        err,
                    );
                }
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

fn read_file_mappings(pid: u32, walk: &mut MapsWalk) -> Option<Vec<FileMapping>> {
    let mut maps_file = BufReader::new(open_maps(pid)?);
    let mut buffer = Vec::with_capacity(256);

    let mut result = Vec::new();
    // Each line: "start-end perms offset dev inode  [path]"
    loop {
        buffer.clear();
        let bytes_read = match maps_file.read_until(b'\n', &mut buffer) {
            Ok(n) => n,
            Err(err) => {
                if !profile::proc_io_error(&err) {
                    log::error!(
                        "Failed to read line from `maps` file for process {}: {}",
                        pid,
                        err,
                    );
                }
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

        let path = walk.intern(&String::from_utf8_lossy(path));

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

        push_mapping(&mut result, path, end - start);
    }

    Some(result)
}

/// Segments of one file are usually adjacent; their sizes add up in the weighted mean, so one
/// entry per run gives the same share factor and keeps the walk small.
fn push_mapping(mappings: &mut Vec<FileMapping>, path: u32, bytes: u64) {
    let size = u32::try_from(bytes.div_ceil(SIZE_UNIT)).unwrap_or(u32::MAX);
    match mappings.last_mut() {
        Some(last) if last.path == path => last.size = last.size.saturating_add(size),
        _ => mappings.push(FileMapping { path, size }),
    }
}

/// Size-weighted harmonic mean of per-file share counts
fn harmonic_avg_share(maps: &[FileMapping], share_count: &[u32]) -> f64 {
    let mut total_size: f64 = 0.0;
    let mut weighted_inv: f64 = 0.0;
    for m in maps {
        let share = share_count[m.path as usize].max(1) as f64;
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
            if !profile::proc_io_error(&err) {
                log::debug!("Failed to open `status` file for process {}: {}", pid, err,);
            }
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
            if !profile::proc_io_error(&err) {
                log::debug!("Failed to open `maps` file for process {}: {}", pid, err,);
            }
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use super::*;

    /// Entries one process keeps during a walk
    fn walk_entries(mappings: &[(&str, u64)]) -> usize {
        let mut walk = MapsWalk::default();
        let mut merged = Vec::new();
        for &(path, size) in mappings {
            let path = walk.intern(path);
            push_mapping(&mut merged, path, size);
        }
        merged.len()
    }

    /// The previous representation: per-PID mappings with paths interned per process.
    fn reference(processes: &[(u32, Vec<(&str, u64)>)]) -> HashMap<u32, f64> {
        let mut share_count: HashMap<Arc<str>, u32> = HashMap::new();
        let mut file_mappings: HashMap<u32, Vec<(Arc<str>, u64)>> = HashMap::new();
        for (pid, mappings) in processes {
            let mut interned: HashSet<Arc<str>> = HashSet::new();
            let mappings: Vec<(Arc<str>, u64)> = mappings
                .iter()
                .map(|&(path, size)| {
                    let path = match interned.get(path) {
                        Some(k) => Arc::clone(k),
                        None => {
                            let arc: Arc<str> = Arc::from(path);
                            interned.insert(Arc::clone(&arc));
                            arc
                        }
                    };
                    (path, size)
                })
                .collect();
            let mut seen: HashSet<*const str> = HashSet::new();
            for (path, _) in &mappings {
                if seen.insert(Arc::as_ptr(path)) {
                    *share_count.entry(Arc::clone(path)).or_insert(0) += 1;
                }
            }
            file_mappings.insert(*pid, mappings);
        }
        file_mappings
            .iter()
            .map(|(pid, maps)| {
                let mut total_size: f64 = 0.0;
                let mut weighted_inv: f64 = 0.0;
                for (path, size) in maps {
                    let share = (*share_count.get(path).unwrap_or(&1)).max(1) as f64;
                    weighted_inv += *size as f64 / share;
                    total_size += *size as f64;
                }
                let share = if weighted_inv == 0.0 {
                    1.0
                } else {
                    total_size / weighted_inv
                };
                (*pid, share)
            })
            .collect()
    }

    #[test]
    fn share_factors_match_per_process_mappings() {
        let processes = vec![
            (
                1,
                vec![
                    ("/usr/bin/a", 4096),
                    ("/usr/bin/a", 8192),
                    ("/usr/lib/libc.so.6", 1_900_544),
                    ("/usr/lib/libc.so.6", 4096),
                    ("/usr/bin/a", 8192),
                    ("/usr/lib/libm.so.6", 905_216),
                ],
            ),
            (
                2,
                vec![
                    ("/usr/lib/libc.so.6", 1_900_544),
                    ("/usr/lib/libLLVM.so.22.1", 123_457_536),
                ],
            ),
            (
                3,
                vec![("/usr/lib/libc.so.6", 4096), ("/usr/bin/c", 28_672)],
            ),
            (4, vec![]),
        ];

        let mut walk = MapsWalk::default();
        for (pid, mappings) in &processes {
            let mut merged = Vec::new();
            for &(path, size) in mappings {
                let path = walk.intern(path);
                push_mapping(&mut merged, path, size);
            }
            walk.add(*pid, merged);
        }
        let share = walk.finish();

        let expected = reference(&processes);
        assert_eq!(share.len(), expected.len());
        for (pid, factor) in &expected {
            // Merged runs and 4 KiB units only regroup the same sums
            let error = (share[pid] - factor).abs() / factor;
            assert!(error < 1e-12, "pid {pid}: {} vs {factor}", share[pid]);
        }
        assert_eq!(share[&4], 1.0);
        assert_eq!(walk_entries(&processes[0].1), 4);
        assert!(share[&2] > 1.0);
    }
}
