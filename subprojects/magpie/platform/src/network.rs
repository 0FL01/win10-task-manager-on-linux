/* src/network.rs
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

use parking_lot::Mutex;
use rayon::ThreadPool;

pub type ConnectionKind = types::network::ConnectionKind;
pub type ConnectionState = types::network::ConnectionState;
pub type WirelessConnection = types::network::WirelessConnection;
pub type Connection = types::network::Connection;

pub type ConnectionList = types::network::connections_response::ConnectionList;
pub type ConnectionResponseKind = types::network::connections_response::Response;
pub type ConnectionsRequest = types::network::ConnectionsRequest;
pub type ConnectionsResponse = types::network::ConnectionsResponse;
pub type ConnectionsResponseError = types::network::ConnectionsResponseError;

pub trait NetworkCache {
    fn new() -> Self
    where
        Self: Sized;
    fn refresh(&mut self);
    fn cached_entries(&self) -> &HashMap<String, Connection>;
}

pub struct NetworkHandler<NC>
where
    NC: NetworkCache,
{
    pub(crate) network: Mutex<NC>,
    pub(crate) local_cache: Mutex<HashMap<String, Connection>>,
    refreshing: AtomicBool,
}

impl<NC> Default for NetworkHandler<NC>
where
    NC: NetworkCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<NC> NetworkHandler<NC>
where
    NC: NetworkCache + Send,
{
    pub fn new() -> Self {
        Self {
            network: Mutex::new(NC::new()),
            local_cache: Mutex::new(Default::default()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut network = self.network.lock();
        network.refresh();
        *self.local_cache.lock() = network.cached_entries().clone();
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
                "PERF: Refreshed network connections in {:?}",
                start.elapsed()
            );
        });
    }
}
