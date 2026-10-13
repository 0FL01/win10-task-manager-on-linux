/* src/memory.rs
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

use std::sync::atomic::{AtomicBool, Ordering};

use crate::profile::{self, Domain, Phase, QueueTicket, Span};
use parking_lot::Mutex;
use rayon::ThreadPool;

pub type Memory = types::memory::Memory;
pub type MemoryDevice = types::memory::MemoryDevice;
pub type SwapDevice = types::memory::SwapDevice;
pub type ZramStats = types::memory::ZramStats;

pub type MemoryInfo = types::memory::memory_response::MemoryInfo;
pub type MemoryRequestKind = types::memory::memory_request::Kind;
pub type MemoryRequest = types::memory::MemoryRequest;
pub type MemoryDeviceList = types::memory::memory_response::MemoryDeviceList;
pub type MemoryResponseKind = types::memory::memory_response::Response;
pub type MemoryResponse = types::memory::MemoryResponse;
pub type MemoryResponseInfo = types::memory::memory_response::MemoryInfo;
pub type MemoryResponseInfoResponse = types::memory::memory_response::memory_info::Response;
pub type MemoryResponseError = types::memory::MemoryResponseError;

pub trait MemoryCache {
    fn new() -> Self
    where
        Self: Sized;
    fn refresh(&mut self);
    fn memory(&self) -> &Memory;
    fn devices(&self) -> &[MemoryDevice];
}

pub struct MemoryHandler<MC>
where
    MC: MemoryCache,
{
    pub(crate) memory: Mutex<MC>,
    pub(crate) local_cache: Mutex<Memory>,
    pub(crate) local_device_cache: Mutex<Vec<MemoryDevice>>,
    refreshing: AtomicBool,
}

impl<MC> Default for MemoryHandler<MC>
where
    MC: MemoryCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<MC> MemoryHandler<MC>
where
    MC: MemoryCache + Send,
{
    pub fn new() -> Self {
        let _profile = Span::new(Phase::Init(Domain::Memory));
        Self {
            memory: Mutex::new(MC::new()),
            local_cache: Mutex::new(Memory::default()),
            local_device_cache: Mutex::new(Vec::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let refresh = Span::new(Phase::Refresh(Domain::Memory));
        let mut memory = self.memory.lock();
        memory.refresh();
        drop(refresh);
        let mut publish = Span::new(Phase::Publish(Domain::Memory));
        publish.items(1 + memory.devices().len());
        self.local_cache.lock().clone_from(memory.memory());
        *self.local_device_cache.lock() = memory.devices().to_vec();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            profile::coalesced(Phase::Refresh(Domain::Memory));
            return;
        }

        let queued = QueueTicket::new(Domain::Memory);
        thread_pool.spawn(move || {
            queued.arrived();
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!(
                "PERF: Refreshed memory information in {:?}",
                start.elapsed()
            );
        });
    }
}
