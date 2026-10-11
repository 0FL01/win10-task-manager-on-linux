/* src/battery.rs
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

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use rayon::ThreadPool;

pub type Battery = types::battery::Battery;
pub type HistoryPoint = types::battery::HistoryPoint;
pub type BatteryType = types::battery::BatteryType;
pub type BatteryState = types::battery::BatteryState;

pub type BatteryList = types::battery::battery_response::BatteryList;
pub type BatteryResponseKind = types::battery::battery_response::Response;
pub type BatteryRequest = types::battery::BatteryRequest;
pub type BatteryResponse = types::battery::BatteryResponse;
pub type BatteryResponseError = types::battery::BatteryResponseError;

pub trait BatteryCache {
    fn new() -> Self
    where
        Self: Sized;
    fn refresh(&mut self);
    fn cached_entries(&self) -> &[Battery];
}

pub struct BatteryHandler<BC>
where
    BC: BatteryCache,
{
    pub(crate) battery: Mutex<BC>,
    pub(crate) local_cache: Mutex<Vec<Battery>>,
    refreshing: AtomicBool,
}

impl<BC> Default for BatteryHandler<BC>
where
    BC: BatteryCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<BC> BatteryHandler<BC>
where
    BC: BatteryCache + Send,
{
    pub fn new() -> Self {
        Self {
            battery: Mutex::new(BC::new()),
            local_cache: Mutex::new(Vec::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut battery = self.battery.lock();
        battery.refresh();
        *self.local_cache.lock() = battery.cached_entries().to_vec();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!(
                "PERF: Refreshed battery connections in {:?}",
                start.elapsed()
            );
        });
    }
}
