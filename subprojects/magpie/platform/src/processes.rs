/* src/processes.rs
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
use std::sync::atomic::{AtomicBool, Ordering};

use crate::gpus::Gpu;
use parking_lot::Mutex;
use rayon::ThreadPool;

pub type ProcessState = types::processes::ProcessState;
pub type ProcessUsageStats = types::processes::ProcessUsageStats;
pub type LibraryMissingError = types::processes::LibraryMissingError;
pub type PermissionsError = types::processes::PermissionsError;
pub type NetworkStatsError = types::processes::processes_response::process_map::NetworkStatsError;
pub type Process = types::processes::Process;

pub type ProcessMap = types::processes::processes_response::ProcessMap;
pub type ProcessesResponseKind = types::processes::processes_response::Response;
pub type ProcessesRequestKind = types::processes::processes_request::Request;
pub type ProcessesTerminateRequest = types::processes::TerminateProcessesRequest;
pub type ProcessesKillRequest = types::processes::KillProcessesRequest;
pub type ProcessesRequest = types::processes::ProcessesRequest;
pub type ProcessesResponse = types::processes::ProcessesResponse;
pub type ProcessesResponseError = types::processes::ProcessesResponseError;

pub trait ProcessCache {
    fn new() -> Self
    where
        Self: Sized;

    fn refresh(&mut self);
    fn cached_entries(&self) -> &HashMap<u32, Process>;
    fn cached_network_stats_error(&self) -> &Option<NetworkStatsError>;
}

pub trait ProcessManager {
    fn new() -> Self
    where
        Self: Sized;

    fn terminate_processes(&self, pids: Vec<u32>);
    fn kill_processes(&self, pids: Vec<u32>);
    fn interrupt_processes(&self, pids: Vec<u32>);
    fn signal_user_one_processes(&self, pids: Vec<u32>);
    fn signal_user_two_processes(&self, pids: Vec<u32>);
    fn hangup_processes(&self, pids: Vec<u32>);
    fn continue_processes(&self, pids: Vec<u32>);
    fn suspend_processes(&self, pids: Vec<u32>);
}

pub struct ProcessesHandler<PC, PM>
where
    PC: ProcessCache,
    PM: ProcessManager,
{
    pub(crate) processes: Mutex<PC>,
    pub(crate) process_manager: PM,
    pub(crate) local_cache: Mutex<HashMap<u32, Process>>,
    pub(crate) network_stats_error: Mutex<Option<NetworkStatsError>>,
    refreshing: AtomicBool,
}

impl<PC, PM> Default for ProcessesHandler<PC, PM>
where
    PC: ProcessCache + Send,
    PM: ProcessManager + Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<PC, PM> ProcessesHandler<PC, PM>
where
    PC: ProcessCache + Send,
    PM: ProcessManager + Send + Sync,
{
    pub fn new() -> Self {
        Self {
            processes: Mutex::new(PC::new()),
            process_manager: PM::new(),
            local_cache: Mutex::new(HashMap::new()),
            network_stats_error: Mutex::new(None),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut processes = self.processes.lock();
        processes.refresh();
        *self.local_cache.lock() = processes.cached_entries().clone();
        *self.network_stats_error.lock() = *processes.cached_network_stats_error();
    }

    pub fn refresh_async(
        &'static self,
        thread_pool: &ThreadPool,
        gpus: &'static Mutex<HashMap<String, Gpu>>,
    ) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            {
                let gpu_processes = gpus
                    .lock()
                    .values()
                    .map(|gpu| gpu.processes.clone())
                    .collect::<Vec<_>>();
                let mut processes = self.local_cache.lock();
                for (pid, process) in processes.iter_mut() {
                    for gpu in &gpu_processes {
                        if let Some(gpu_process) = gpu.get(pid) {
                            process.usage_stats.gpu_usage +=
                                gpu_process.gpu_usage_percent.unwrap_or(0.0);
                            process.usage_stats.gpu_memory_usage +=
                                gpu_process.memory_usage_bytes.unwrap_or(0);
                        }
                    }
                }
            }
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed processes in {:?}", start.elapsed());
        });
    }
}
