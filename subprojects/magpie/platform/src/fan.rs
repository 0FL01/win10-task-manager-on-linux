/* src/fan.rs
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

use parking_lot::Mutex;
use rayon::ThreadPool;

pub type Fan = types::fan::Fan;

pub type FanList = types::fan::fans_response::FanList;
pub type FansResponseKind = types::fan::fans_response::Response;
pub type FansRequest = types::fan::FansRequest;
pub type FansResponse = types::fan::FansResponse;
pub type FansResponseError = types::fan::FansResponseError;

pub trait FanCache {
    fn new() -> Self
    where
        Self: Sized;
    fn refresh(&mut self);
    fn cached_entries(&self) -> &[Fan];
}

pub struct FanHandler<FC>
where
    FC: FanCache,
{
    pub(crate) fan: Mutex<FC>,
    pub(crate) local_cache: Mutex<Vec<Fan>>,
    refreshing: AtomicBool,
}

impl<FC> Default for FanHandler<FC>
where
    FC: FanCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<FC> FanHandler<FC>
where
    FC: FanCache + Send,
{
    pub fn new() -> Self {
        Self {
            fan: Mutex::new(FC::new()),
            local_cache: Mutex::new(Vec::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut fan = self.fan.lock();
        fan.refresh();
        *self.local_cache.lock() = fan.cached_entries().to_vec();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed fan connections in {:?}", start.elapsed());
        });
    }
}
