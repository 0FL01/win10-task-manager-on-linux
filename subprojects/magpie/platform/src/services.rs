/* src/services.rs
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
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use rayon::ThreadPool;

pub type Service = types::services::Service;

pub type ServiceList = types::services::services_response::ServiceList;
pub type ServicesRequest = types::services::ServicesRequest;
pub type ServicesRequestKind = types::services::services_request::Request;
pub type ServicesResponse = types::services::ServicesResponse;
pub type ServiceResponseKind = types::services::services_response::Response;
pub type ServicesResponseError = types::services::ServicesResponseError;
pub type ServicesResponseErrorKind = types::services::services_response_error::Error;

pub trait ServiceCache {
    fn new() -> Self
    where
        Self: Sized;

    fn refresh(&mut self);
    fn user_entries(&self) -> &HashMap<u64, Service>;
    fn system_entries(&self) -> &HashMap<u64, Service>;
}

pub trait ServiceManager {
    type ServiceCache: ServiceCache;

    fn new() -> Self
    where
        Self: Sized;

    fn logs(&self, sc: &Self::ServiceCache, id: u64, pid: Option<NonZero<u32>>) -> Option<String>;
    fn start(&self, sc: &Self::ServiceCache, id: u64);
    fn stop(&self, sc: &Self::ServiceCache, id: u64);
    fn restart(&self, sc: &Self::ServiceCache, id: u64);
    fn enable(&self, sc: &Self::ServiceCache, id: u64);
    fn disable(&self, sc: &Self::ServiceCache, id: u64);
}

pub struct ServicesHandler<SC, SM>
where
    SC: ServiceCache,
    SM: ServiceManager,
{
    pub(crate) services: Mutex<SC>,
    pub(crate) service_manager: SM,
    pub(crate) user_local_cache: Mutex<HashMap<u64, Service>>,
    pub(crate) system_local_cache: Mutex<HashMap<u64, Service>>,
    refreshing: AtomicBool,
}

impl<SC, SM> Default for ServicesHandler<SC, SM>
where
    SC: ServiceCache + Send,
    SM: ServiceManager + Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<SC, SM> ServicesHandler<SC, SM>
where
    SC: ServiceCache + Send,
    SM: ServiceManager + Send + Sync,
{
    pub fn new() -> Self {
        Self {
            services: Mutex::new(SC::new()),
            service_manager: SM::new(),
            user_local_cache: Mutex::new(HashMap::new()),
            system_local_cache: Mutex::new(HashMap::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut services = self.services.lock();
        services.refresh();
        *self.user_local_cache.lock() = services.user_entries().clone();
        *self.system_local_cache.lock() = services.system_entries().clone();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed services in {:?}", start.elapsed());
        });
    }
}
