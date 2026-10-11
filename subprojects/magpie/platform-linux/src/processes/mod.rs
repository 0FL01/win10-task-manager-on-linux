/* src/processes/mod.rs
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
use std::fmt::Write;
use std::io::Read;
use std::time::Instant;

use arrayvec::ArrayString;

use magpie_platform::processes::Process;

use crate::processes::mem_hybrid::{hybrid_pss, MemoryMaps};
pub use linux_process::ProcessStats;
pub use manager::ProcessManager;

mod io;
mod linux_process;
mod manager;
mod mem_hybrid;
mod raw_stats;
mod stat;
mod statm;

const MAX_U32_LEN: usize = 10;

fn open_cmdline(pid: u32) -> Option<std::fs::File> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/cmdline".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/cmdline", pid);

    let cmdline_file = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!(
                "Failed to read `cmdline` file for process {}, skipping: {}",
                pid,
                e,
            );
            return None;
        }
    };

    Some(cmdline_file)
}

fn exe_path(pid: u32) -> ArrayString<20> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/exe".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/exe", pid);

    path
}

fn task_path(pid: u32) -> ArrayString<21> {
    const MAX_PATH_LEN: usize = "/proc/".len() + "/task".len() + MAX_U32_LEN;

    let mut path: ArrayString<MAX_PATH_LEN> = ArrayString::new();
    let _ = write!(path, "/proc/{}/task", pid);

    path
}

pub struct ProcessCache {
    processes: HashMap<u32, Process>,
    process_stats: HashMap<u32, ProcessStats>,

    memory_maps: MemoryMaps,
}

impl magpie_platform::processes::ProcessCache for ProcessCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            processes: HashMap::new(),
            process_stats: HashMap::new(),

            memory_maps: Default::default(),
        }
    }

    fn refresh(&mut self) {
        let mut old_processes = std::mem::take(&mut self.processes);
        let mut old_stats = std::mem::take(&mut self.process_stats);

        let result = &mut self.processes;
        result.reserve(old_processes.len());

        let result_stats = &mut self.process_stats;
        result_stats.reserve(old_stats.len());

        let mut stat_file_content = String::new();
        stat_file_content.reserve(512);

        let mut read_buffer = String::new();
        read_buffer.reserve(512);

        let now = Instant::now();

        let proc = match std::fs::read_dir("/proc") {
            Ok(proc) => proc,
            Err(e) => {
                log::warn!("Failed to read /proc directory: {}", e);
                return;
            }
        };

        // Floor capacity at 500 to avoid reallocations on cold start when `old_processes` is empty;
        // a typical Linux desktop should have around 500 running processes.
        let mut pids = Vec::with_capacity(old_processes.len().max(500));
        let proc_entries = proc
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false));
        for entry in proc_entries {
            let pid = match entry.file_name().to_string_lossy().parse::<u32>().ok() {
                Some(pid) => pid,
                _ => continue,
            };
            pids.push(pid);
        }

        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();

        self.memory_maps.refresh(&pids);

        for pid in pids.iter().copied() {
            let Some(mut stat_file) = stat::open(pid) else {
                continue;
            };
            stat_file_content.clear();
            match stat_file.read_to_string(&mut stat_file_content) {
                Ok(sfc) => {
                    if sfc == 0 {
                        log::warn!(
                            "Failed to read stat information for process {}, skipping",
                            pid
                        );
                        continue;
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to read stat information for process {}, skipping: {}",
                        pid,
                        e,
                    );
                    continue;
                }
            };
            let Some(stat_parsed) = stat::parse_stat_file(&stat_file_content) else {
                log::warn!(
                    "Failed to parse stat information for process {}, skipping",
                    pid
                );
                continue;
            };

            let utime = stat::user_mode_jiffies(&stat_parsed);
            let stime = stat::kernel_mode_jiffies(&stat_parsed);

            let io_parsed = io::open(pid)
                .map(|mut io_file| {
                    read_buffer.clear();
                    match io_file.read_to_string(&mut read_buffer) {
                        Ok(rb) => {
                            if rb == 0 {
                                log::debug!("Failed to read io information for process {}", pid);
                                [0; 7]
                            } else {
                                io::parse_io_file(&read_buffer)
                            }
                        }
                        Err(e) => {
                            log::debug!("Failed to read io information for process {}: {}", pid, e);
                            [0; 7]
                        }
                    }
                })
                .unwrap_or([0; 7]);

            let (mut process, mut stats) = match old_processes.remove(&pid) {
                None => (Process::default(), ProcessStats::default()),
                Some(mut process) => {
                    let stats = unsafe { old_stats.remove(&pid).unwrap_unchecked() };
                    let delta_time = now - stats.raw_stats.timestamp;

                    let prev_utime = stats.raw_stats.user_jiffies;
                    let prev_stime = stats.raw_stats.kernel_jiffies;

                    let delta_utime = ((utime.saturating_sub(prev_utime) as f32) * 1000.)
                        / crate::sys_hz() as f32;
                    let delta_stime = ((stime.saturating_sub(prev_stime) as f32) * 1000.)
                        / crate::sys_hz() as f32;

                    process.usage_stats.cpu_usage =
                        (((delta_utime + delta_stime) / delta_time.as_millis() as f32) * 100.)
                            .min((crate::cpu_count() as f32) * 100.);

                    let prev_read_bytes = stats.raw_stats.disk_read_bytes;
                    let prev_write_bytes = stats.raw_stats.disk_write_bytes;

                    let read_speed = io::read_bytes(&io_parsed).saturating_sub(prev_read_bytes)
                        as f32
                        / delta_time.as_secs_f32();
                    let write_speed = io::write_bytes(&io_parsed).saturating_sub(prev_write_bytes)
                        as f32
                        / delta_time.as_secs_f32();
                    process.usage_stats.disk_usage = read_speed + write_speed;

                    process.children.clear();

                    (process, stats)
                }
            };

            read_buffer.clear();
            let cmd = open_cmdline(pid)
                .and_then(|mut f| {
                    match f
                        .read_to_string(&mut read_buffer)
                        .ok()
                        .map(|_| &read_buffer)
                    {
                        Some(cmd) => Some(
                            cmd.split('\0')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>(),
                        ),
                        None => {
                            log::warn!("Failed to read `cmdline` for process {}", pid);
                            None
                        }
                    }
                })
                .unwrap_or(vec![]);

            let exe = std::path::Path::new(exe_path(pid).as_str())
                .read_link()
                .map(|p| p.as_os_str().to_string_lossy().to_string())
                .unwrap_or("".to_owned());

            let (memory, swap) = if let Some((pss, swap_pss)) = hybrid_pss(pid, &self.memory_maps) {
                (pss, swap_pss)
            } else {
                (
                    statm::open(pid)
                        .map(|mut statm_file| {
                            read_buffer.clear();
                            match statm_file.read_to_string(&mut read_buffer) {
                                Ok(rb) => {
                                    if rb == 0 {
                                        log::warn!(
                                            "Failed to read 'statm' information for process {}",
                                            pid
                                        );
                                        0
                                    } else {
                                        statm::parse_statm_file(&read_buffer)
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to read 'statm' information for process {}: {}",
                                        pid,
                                        e
                                    );
                                    0
                                }
                            }
                        })
                        .unwrap_or(0),
                    0,
                )
            };

            let task_count = match std::fs::read_dir(task_path(pid).as_str()) {
                Ok(tasks) => {
                    let mut task_count = 0u64;
                    for task in tasks.filter_map(|t| t.ok()) {
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

            process.pid = pid;
            process.start_time_ticks = stat::start_time_ticks(&stat_parsed);
            process.name = stat::name(&stat_parsed).to_string();
            process.cmd = cmd;
            process.exe = exe;
            process.state = stat::state(&stat_parsed) as _;
            process.parent = stat::parent_pid(&stat_parsed);
            process.usage_stats.gpu_usage = 0.;
            process.usage_stats.gpu_memory_usage = 0;
            process.usage_stats.memory_usage = memory;
            process.usage_stats.swap_usage = swap;
            process.task_count = task_count;
            stats.raw_stats.user_jiffies = utime;
            stats.raw_stats.kernel_jiffies = stime;
            stats.raw_stats.disk_read_bytes = io::read_bytes(&io_parsed);
            stats.raw_stats.disk_write_bytes = io::write_bytes(&io_parsed);
            stats.raw_stats.timestamp = now;

            match children.get_mut(&process.parent) {
                Some(children) => children.push(pid),
                None => {
                    children.insert(process.parent, vec![pid]);
                }
            }

            result.insert(pid, process);
            result_stats.insert(pid, stats);
        }

        // Prune processes that don't have a parent (init and kernel processes)
        children.remove(&0);

        for (parent, children) in children.drain() {
            // SAFETY: parent is always a valid pid
            if let Some(parent) = result.get_mut(&parent) {
                parent.children = children;
            }
        }

        log::debug!("PERF: Refreshed process information in {:?}", now.elapsed());
    }

    fn cached_entries(&self) -> &HashMap<u32, Process> {
        &self.processes
    }
}
