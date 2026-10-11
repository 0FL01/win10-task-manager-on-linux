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
use regex::RegexBuilder;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use virtual_terminal::Output;

use magpie_platform::processes::{NetworkStatsError, PermissionsError, Process};

use crate::processes::mem_hybrid::{hybrid_pss, MemoryMaps};
use crate::util::async_runtime;
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

#[derive(Debug, Default)]
pub struct NethogsRecord {
    pub _name: String,
    pub pid: u32,
    pub _device_name: String,
    pub sent_bytes: u64,
    pub recv_bytes: u64,
}

impl NethogsRecord {
    pub fn new(
        name: String,
        pid: u32,
        device_name: String,
        sent_bytes: u64,
        recv_bytes: u64,
    ) -> Self {
        Self {
            _name: name,
            pid,
            _device_name: device_name,
            sent_bytes,
            recv_bytes,
        }
    }
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
    network_stats_error: Option<NetworkStatsError>,

    nethogs_reciever: Receiver<NethogsRecord>,
    nethogs_process_lut: HashMap<u32, NethogsRecord>,

    memory_maps: MemoryMaps,
}

async fn run_nethogs(sender: Sender<NethogsRecord>) {
    let cmd = virtual_terminal::Command::new("nethogs")
        .terminal_size((250, 1000))
        .arg("-v")
        .arg("2")
        .arg("-C");

    let mut nethogs_term = vt100::Parser::new(1000, 250, 0);

    let out_rx = cmd.out_rx();
    async_runtime().spawn(cmd.run());

    let nethogs_regex = match RegexBuilder::new(
        r"^\s*(\d+|\?) (.{8}) (.*?)\s+(.{0,16})([ \d.]{12})([ \d.]{12})[Bb]",
    )
    .multi_line(true)
    .build()
    {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to build nethogs regex {}", e);
            return;
        }
    };

    // TODO report the difference between no nethogs and no perms
    // let mut is_alive = false;

    while let Ok(msg) = out_rx.recv().await {
        match msg {
            Output::Pid(pid) => {
                log::debug!("Nethogs PID: {}", pid);
                // is_alive = true;
            }
            Output::Stdout(out) => {
                nethogs_term.process(&out);

                let (_, cursor_col) = nethogs_term.screen().cursor_position();

                if cursor_col == 0 {
                    for (_, [pid, _user, command_line, iface, sent_b, recv_b]) in nethogs_regex
                        .captures_iter(nethogs_term.screen().contents().as_str())
                        .map(|c| c.extract())
                    {
                        if pid == "?" {
                            continue;
                        }

                        let pid = match pid.parse::<u32>() {
                            Ok(pid) => pid,
                            _ => {
                                continue;
                            }
                        };

                        let sent_b = match sent_b.trim().parse::<f64>() {
                            Ok(sent_b) => sent_b as u64,
                            _ => continue,
                        };

                        let recv_b = match recv_b.trim().parse::<f64>() {
                            Ok(recv_b) => recv_b as u64,
                            _ => continue,
                        };

                        // ignore result, just keep sending
                        let _ = sender
                            .send(NethogsRecord::new(
                                command_line.into(),
                                pid,
                                iface.into(),
                                sent_b,
                                recv_b,
                            ))
                            .await;
                    }
                }
            }
            Output::Error(err) => {
                log::warn!("Nethogs stderr: {}", err);
            }
            Output::Terminated(e) => {
                if let Some(code) = e {
                    log::warn!("Nethogs exited with code {}", code);
                }

                break;
            }
        }
    }
}

impl magpie_platform::processes::ProcessCache for ProcessCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let (tx, rx): (Sender<NethogsRecord>, Receiver<NethogsRecord>) = channel(1000);

        async_runtime().spawn(run_nethogs(tx));

        Self {
            processes: HashMap::new(),
            process_stats: HashMap::new(),

            network_stats_error: None,
            nethogs_reciever: rx,
            nethogs_process_lut: HashMap::new(),

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

        let nethogs_process_lut = &mut self.nethogs_process_lut;
        let mut has_cleared = false;

        loop {
            match self.nethogs_reciever.try_recv() {
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    log::debug!("Nethogs process channel closed");
                    self.network_stats_error =
                        Some(NetworkStatsError::PermissionsError(PermissionsError {}));
                    break;
                }
                Ok(item) => {
                    if !has_cleared {
                        nethogs_process_lut.clear();
                        has_cleared = true;
                    }
                    nethogs_process_lut.insert(item.pid, item);
                    self.network_stats_error = None;
                }
            }
        }

        log::debug!("LUT: {:?}", nethogs_process_lut);

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

            let total_net_sent = nethogs_process_lut
                .get(&pid)
                .map(|it| it.sent_bytes)
                .unwrap_or(0);
            let total_net_recv = nethogs_process_lut
                .get(&pid)
                .map(|it| it.recv_bytes)
                .unwrap_or(0);

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

                    let prev_send_bytes = stats.raw_stats.net_bytes_sent;
                    let prev_recv_bytes = stats.raw_stats.net_bytes_recv;

                    if prev_send_bytes + prev_recv_bytes > total_net_sent + total_net_recv {
                        log::warn!(
                            "Process {} had negative network usage {} -> {}",
                            pid,
                            prev_send_bytes + prev_recv_bytes,
                            total_net_sent + total_net_recv
                        );
                    }

                    process.usage_stats.network_usage = (total_net_sent + total_net_recv)
                        .saturating_sub(prev_send_bytes + prev_recv_bytes)
                        as f32
                        / delta_time.as_secs_f32();

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
            stats.raw_stats.net_bytes_sent = total_net_sent;
            stats.raw_stats.net_bytes_recv = total_net_recv;
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

    fn cached_network_stats_error(&self) -> &Option<NetworkStatsError> {
        &self.network_stats_error
    }
}
