/* src/lib.rs
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
use std::num::NonZeroU32;

pub use anyhow::Error;
pub use anyhow::Result;
pub use parking_lot::Mutex;
use rayon::{ThreadPool, ThreadPoolBuilder};
pub use types::IntoEnumIterator;

use about::{About, AboutCache, AboutHandler};
use apps::{App, AppCache, AppsHandler, Icon};
use battery::{Battery, BatteryCache, BatteryHandler};
use cpu::{Cpu, CpuCache, CpuHandler};
use disks::{DiskSmartData, DisksCache, DisksHandler, DisksManager, DisksResponseErrorEjectFailed};
use fan::{FanCache, FanHandler};
use gpus::{Gpu, GpuCache, GpusHandler};
use memory::{Memory, MemoryCache, MemoryDevice, MemoryHandler};
use network::{Connection, NetworkCache, NetworkHandler};
use processes::{Process, ProcessCache, ProcessManager, ProcessesHandler};
use services::{Service, ServiceCache, ServiceManager, ServicesHandler};

pub mod about;
pub mod apps;
pub mod battery;
pub mod cpu;
pub mod disks;
pub mod fan;
pub mod gpus;
pub mod ipc;
pub mod memory;
pub mod network;
pub mod processes;
pub mod profile;
pub mod services;

pub trait DataCacheTypes {
    type AboutCache: AboutCache + Send;
    type AppCache: AppCache + Send;
    type BatteryCache: BatteryCache + Send;
    type CpuCache: CpuCache + Send;
    type DisksCache: DisksCache + Send;
    type DisksManager: DisksManager + Send + Sync;
    type FanCache: FanCache + Send;
    type GpuCache: GpuCache + Send;
    type MemoryCache: MemoryCache + Send;
    type NetworkCache: NetworkCache + Send;
    type ProcessCache: ProcessCache + Send;
    type ProcessManager: ProcessManager + Send + Sync;
    type ServiceCache: ServiceCache + Send;
    type ServicesManager: ServiceManager<ServiceCache = Self::ServiceCache> + Send + Sync;
}

pub struct DataCache<CT>
where
    CT: DataCacheTypes,
{
    about: AboutHandler<CT::AboutCache>,
    apps: AppsHandler<CT::AppCache>,
    batteries: BatteryHandler<CT::BatteryCache>,
    cpu: CpuHandler<CT::CpuCache>,
    disks: DisksHandler<CT::DisksCache, CT::DisksManager>,
    fans: FanHandler<CT::FanCache>,
    gpus: GpusHandler<CT::GpuCache>,
    memory: MemoryHandler<CT::MemoryCache>,
    network: NetworkHandler<CT::NetworkCache>,
    processes: ProcessesHandler<CT::ProcessCache, CT::ProcessManager>,
    services: ServicesHandler<CT::ServiceCache, CT::ServicesManager>,

    thread_pool: ThreadPool,
}

impl<CT> Default for DataCache<CT>
where
    CT: DataCacheTypes,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<CT> DataCache<CT>
where
    CT: DataCacheTypes,
{
    pub fn new() -> Self {
        let _profile = profile::Span::new(profile::Phase::Startup);
        Self {
            about: AboutHandler::new(),
            apps: AppsHandler::new(),
            batteries: BatteryHandler::new(),
            cpu: CpuHandler::new(),
            disks: DisksHandler::new(),
            fans: FanHandler::new(),
            gpus: GpusHandler::new(),
            memory: MemoryHandler::new(),
            network: NetworkHandler::new(),
            processes: ProcessesHandler::new(),
            services: ServicesHandler::new(),

            thread_pool: ThreadPoolBuilder::new()
                .num_threads(4)
                .thread_name(|i| format!("data-cache-{}", i))
                .build()
                .expect("Failed to create thread pool"),
        }
    }

    pub fn refresh(&'static self) {
        self.thread_pool.scope(|s| {
            s.spawn(|_| {
                self.refresh_processes();
                self.refresh_apps();
            });
            s.spawn(|_| self.refresh_batteries());
            s.spawn(|_| self.refresh_cpu());
            s.spawn(|_| self.refresh_disks());
            s.spawn(|_| self.refresh_fans());
            s.spawn(|_| self.refresh_memory());
            s.spawn(|_| self.refresh_gpus());
            s.spawn(|_| self.refresh_network());
            s.spawn(|_| self.refresh_services());
        });
    }

    pub fn refresh_async(&'static self) {
        self.refresh_apps_async();
        self.refresh_batteries_async();
        self.refresh_cpu_async();
        self.refresh_disks_async();
        self.refresh_fans_async();
        self.refresh_memory_async();
        self.refresh_gpus_async();
        self.refresh_network_async();
        self.refresh_processes_async();
        self.refresh_services_async();
    }

    pub fn about(&'static self) -> About {
        self.refresh_about_async();
        let mut data = self.about.local_cache.lock().clone();
        data.ready = Some(data.ready == Some(true));
        data
    }

    pub fn refresh_about_async(&'static self) {
        self.about.refresh_async(&self.thread_pool);
    }

    pub fn refresh_apps(&'static self) {
        self.apps.refresh(&self.processes.local_cache);
    }

    pub fn refresh_apps_async(&'static self) {
        self.apps
            .refresh_async(&self.thread_pool, &self.processes.local_cache)
    }

    pub fn refresh_batteries(&'static self) {
        self.batteries.refresh();
    }

    pub fn refresh_batteries_async(&'static self) {
        self.batteries.refresh_async(&self.thread_pool);
    }

    pub fn refresh_cpu(&'static self) {
        self.cpu.refresh();
    }

    pub fn refresh_cpu_async(&'static self) {
        self.cpu.refresh_async(&self.thread_pool);
    }

    pub fn refresh_disks(&'static self) {
        self.disks.refresh();
        self.disks.refresh_smart_async(&self.thread_pool);
    }

    pub fn refresh_disks_async(&'static self) {
        self.disks.refresh_async(&self.thread_pool);
    }

    pub fn refresh_fans(&'static self) {
        self.fans.refresh();
    }

    pub fn refresh_fans_async(&'static self) {
        self.fans.refresh_async(&self.thread_pool);
    }

    pub fn refresh_gpus(&'static self) {
        self.gpus.refresh();
    }

    pub fn refresh_gpus_async(&'static self) {
        self.gpus.refresh_async(&self.thread_pool);
    }

    pub fn refresh_memory(&'static self) {
        self.memory.refresh();
    }

    pub fn refresh_memory_async(&'static self) {
        self.memory.refresh_async(&self.thread_pool);
    }

    pub fn refresh_network(&'static self) {
        self.network.refresh();
    }

    pub fn refresh_network_async(&'static self) {
        self.network.refresh_async(&self.thread_pool);
    }

    pub fn refresh_processes(&'static self) {
        self.processes.refresh();
    }

    pub fn refresh_processes_async(&'static self) {
        self.processes
            .refresh_async(&self.thread_pool, &self.gpus.local_cache);
    }

    pub fn refresh_services(&'static self) {
        self.services.refresh();
    }

    pub fn refresh_services_async(&'static self) {
        self.services.refresh_async(&self.thread_pool);
    }

    pub fn apps(&self) -> Vec<App> {
        self.apps.local_cache.lock().clone()
    }

    pub fn app_icons(&self) -> HashMap<String, Icon> {
        self.apps.local_icons_cache.lock().clone()
    }

    pub fn batteries(&self) -> Vec<Battery> {
        self.batteries.local_cache.lock().clone()
    }

    pub fn cpu(&self) -> Cpu {
        self.cpu.local_cache.lock().clone()
    }

    pub fn disks(&self) -> Vec<types::disks::Disk> {
        self.disks_snapshot().disks
    }

    pub fn disks_snapshot(&self) -> disks::DiskList {
        self.disks.snapshot()
    }

    pub fn eject_disk(&self, id: &str) -> std::result::Result<(), DisksResponseErrorEjectFailed> {
        self.disks
            .disk_manager
            .eject(id, &self.processes.local_cache)
    }

    pub fn smart_data(&self, id: &str) -> Option<DiskSmartData> {
        self.disks.disk_manager.smart_data(id)
    }

    pub fn fans(&self) -> Vec<types::fan::Fan> {
        self.fans.local_cache.lock().clone()
    }

    pub fn gpus(&self) -> HashMap<String, Gpu> {
        self.gpus.cached_entries_with_age()
    }

    pub fn gpus_ready(&self) -> bool {
        self.gpus.ready()
    }

    pub fn memory(&self) -> Memory {
        self.memory.local_cache.lock().clone()
    }

    pub fn memory_devices(&self) -> Vec<MemoryDevice> {
        self.memory.local_device_cache.lock().clone()
    }

    pub fn connections(&self) -> HashMap<String, Connection> {
        self.network.local_cache.lock().clone()
    }

    pub fn processes(&self) -> HashMap<u32, Process> {
        self.processes.local_cache.lock().clone()
    }

    pub fn terminate_processes(&self, pids: Vec<u32>) {
        self.processes.process_manager.terminate_processes(pids)
    }

    pub fn kill_processes(&self, pids: Vec<u32>) {
        self.processes.process_manager.kill_processes(pids)
    }

    pub fn user_services(&self) -> Vec<Service> {
        self.services
            .user_local_cache
            .lock()
            .values()
            .cloned()
            .collect()
    }

    pub fn system_services(&self) -> Vec<Service> {
        self.services
            .system_local_cache
            .lock()
            .values()
            .cloned()
            .collect()
    }

    pub fn service_logs(&self, id: u64, pid: Option<NonZeroU32>) -> Option<String> {
        let sc = self.services.services.lock();
        self.services.service_manager.logs(&*sc, id, pid)
    }

    pub fn service_start(&self, id: u64) {
        let sc = self.services.services.lock();
        self.services.service_manager.start(&*sc, id)
    }

    pub fn service_stop(&self, id: u64) {
        let sc = self.services.services.lock();
        self.services.service_manager.stop(&*sc, id)
    }

    pub fn service_restart(&self, id: u64) {
        let sc = self.services.services.lock();
        self.services.service_manager.restart(&*sc, id)
    }

    pub fn service_enable(&self, id: u64) {
        let sc = self.services.services.lock();
        self.services.service_manager.enable(&*sc, id)
    }

    pub fn service_disable(&self, id: u64) {
        let sc = self.services.services.lock();
        self.services.service_manager.disable(&*sc, id)
    }
}
