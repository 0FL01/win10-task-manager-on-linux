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

use crate::profile::{self, Domain, Phase, QueueTicket, Span};
use parking_lot::Mutex;
use rayon::ThreadPool;

pub type Fan = types::fan::Fan;
pub type HwmonChip = types::fan::HwmonChip;
pub type TemperatureSensor = types::fan::TemperatureSensor;

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
    fn cached_temperatures(&self) -> &[TemperatureSensor];
}

pub struct FanHandler<FC>
where
    FC: FanCache,
{
    pub(crate) fan: Mutex<FC>,
    /// Fans and temperatures are published together, as one coherent snapshot.
    pub(crate) local_cache: Mutex<FanList>,
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
        let _profile = Span::new(Phase::Init(Domain::Fans));
        Self {
            fan: Mutex::new(FC::new()),
            local_cache: Mutex::new(FanList {
                fans: Vec::new(),
                temperatures: Vec::new(),
                ready: Some(false),
            }),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let refresh = Span::new(Phase::Refresh(Domain::Fans));
        let mut fan = self.fan.lock();
        fan.refresh();
        drop(refresh);
        let mut publish = Span::new(Phase::Publish(Domain::Fans));
        publish.items(fan.cached_entries().len() + fan.cached_temperatures().len());
        *self.local_cache.lock() = FanList {
            fans: fan.cached_entries().to_vec(),
            temperatures: fan.cached_temperatures().to_vec(),
            ready: Some(true),
        };
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            profile::coalesced(Phase::Refresh(Domain::Fans));
            return;
        }

        let queued = QueueTicket::new(Domain::Fans);
        thread_pool.spawn(move || {
            queued.arrived();
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed fan connections in {:?}", start.elapsed());
        });
    }
}
