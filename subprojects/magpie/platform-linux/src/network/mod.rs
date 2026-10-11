/* src/network/mod.rs
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

use magpie_platform::network::Connection;

use crate::async_runtime;
use crate::network::network_wrapper::NetworkWrapper;
use crate::{sync, system_bus};

use interface_iter::InterfaceIter;
use network_manager::NetworkManagerProxy;

mod device_name;
mod interface_iter;
mod ip;
mod network_manager;
mod network_wrapper;
mod stats;
mod util;
mod wireless;

struct TransferStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,

    pub update_timestamp: std::time::Instant,
}

#[derive(Default)]
pub struct NetworkCache {
    proxy: Option<NetworkManagerProxy<'static>>,

    connection_wrappers: HashMap<String, NetworkWrapper>,
    connections: HashMap<String, Connection>,

    stats_cache: HashMap<String, TransferStats>,
    device_name_cache: HashMap<String, String>,
}

impl magpie_platform::network::NetworkCache for NetworkCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let dbus_connection = match system_bus() {
            Some(c) => c,
            None => {
                log::warn!(
                    "Failed to connect to system bus, network information will not be available"
                );
                return Self::default();
            }
        };

        let rt = async_runtime();

        let proxy = match sync!(rt, NetworkManagerProxy::new(dbus_connection)) {
            Ok(p) => Some(p),
            Err(e) => {
                log::warn!("Failed to connect to NetworkManager, network information will not be available: {e}");
                return Self::default();
            }
        };

        Self {
            proxy,

            connection_wrappers: Default::default(),
            connections: Default::default(),

            stats_cache: HashMap::new(),
            device_name_cache: HashMap::new(),
        }
    }

    fn refresh(&mut self) {
        let proxy = match &self.proxy {
            Some(p) => p,
            None => return,
        };

        let system_bus = match system_bus() {
            Some(c) => c,
            None => {
                log::warn!(
                    "Failed to connect to system bus, network information will not be available"
                );
                return;
            }
        };

        let mut prev_nets = std::mem::take(&mut self.connection_wrappers);

        let rt = async_runtime();

        for if_name in InterfaceIter::new() {
            // TODO: skip based on type now that detection by name is (mostly) deprecated
            if if_name.starts_with("lo") {
                continue;
            }

            let mut prev_network = match prev_nets.remove(&*if_name) {
                Some(n) => n,
                None => {
                    let Some(nw) = NetworkWrapper::new(
                        rt,
                        proxy,
                        system_bus,
                        &if_name,
                        &mut self.device_name_cache,
                    ) else {
                        continue;
                    };

                    nw
                }
            };

            prev_network.refresh(rt, &mut self.stats_cache, system_bus);

            self.connections
                .insert(if_name.to_string(), prev_network.connection.clone());
            self.connection_wrappers
                .insert(if_name.into_owned(), prev_network);
        }
    }

    fn cached_entries(&self) -> &HashMap<String, Connection> {
        &self.connections
    }
}
