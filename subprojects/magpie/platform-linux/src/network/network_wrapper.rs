/* src/network/network_wrapper.rs
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
use crate::network::device_name::device_name;
use crate::network::network_manager::NetworkManagerProxy;
use crate::network::{ip, stats, util, wireless, TransferStats};
use crate::sync;
use magpie_platform::network::{Connection, ConnectionKind, ConnectionState};
use std::collections::HashMap;
use tokio::runtime::Handle;
use zbus::Proxy;

pub struct NetworkWrapper {
    pub connection: Connection,

    pub device_proxy: Proxy<'static>,
}

impl NetworkWrapper {
    pub fn new(
        rt: &Handle,
        proxy: &NetworkManagerProxy,
        bus: &zbus::Connection,
        iface: &str,
        device_name_cache: &mut HashMap<String, String>,
    ) -> Option<Self> {
        let mut conn = Connection::default();

        let device_path = match sync!(rt, proxy.get_device_by_ip_iface(iface)) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to get device path for {iface}: {e}");
                return None;
            }
        };

        let device_proxy = match sync!(
            rt,
            zbus::Proxy::new(
                bus,
                "org.freedesktop.NetworkManager",
                device_path,
                "org.freedesktop.NetworkManager.Device"
            )
        ) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to get device proxy for {iface}: {e}");
                return None;
            }
        };

        let syspath = match sync!(rt, device_proxy.get_property::<String>("Udi")) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to get Udi for {iface}: {e}");
                return None;
            }
        };

        conn.hw_address = match sync!(rt, device_proxy.get_property::<String>("HwAddress")) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to get HwAddress for {iface}: {e}");
                return None;
            }
        };

        conn.kind = match sync!(rt, device_proxy.get_property::<u64>("DeviceType")) {
            Ok(p) => util::connection_kind_from_nm_id(p),
            Err(e) => {
                log::warn!("Failed to get DeviceType for {iface}: {e}");
                util::connection_kind_from_name(iface)
            }
        } as i32;

        conn.id = iface.to_owned();
        conn.device_name = device_name(device_name_cache, &syspath);

        Some(Self {
            connection: conn,

            device_proxy,
        })
    }

    // TODO: are all fields from constructor (type, hw_addr) actually static or should they be refreshed here
    pub fn refresh(
        &mut self,
        rt: &Handle,
        stats_cache: &mut HashMap<String, TransferStats>,
        bus: &zbus::Connection,
    ) {
        self.connection.wireless_connection =
            if self.connection.kind == ConnectionKind::Wireless as i32 {
                wireless::wireless_connection(&self.connection.id, rt, &self.device_proxy)
            } else {
                None
            };

        self.connection.state = match sync!(rt, self.device_proxy.get_property::<u32>("State")) {
            Ok(p) => match p {
                0 => ConnectionState::Unknown,
                10 => ConnectionState::Unknown,
                20 => ConnectionState::Unavailable,
                30 => ConnectionState::Disconnected,
                40 => ConnectionState::Connecting,
                50 => ConnectionState::Connecting,
                60 => ConnectionState::NeedsAuth,
                70 => ConnectionState::ConfiguringIp,
                80 => ConnectionState::ConfiguringIp,
                90 => ConnectionState::Connecting,
                100 => ConnectionState::Connected,
                110 => ConnectionState::Disconnecting,
                120 => ConnectionState::Failed,
                _ => ConnectionState::Unknown,
            },
            Err(e) => {
                log::warn!("Failed to get state for {}: {e}", self.connection.id);
                ConnectionState::Unknown
            }
        } as i32;

        let (tx_bytes, rx_bytes) = stats::bytes_transfered(&self.connection.id);
        (
            self.connection.tx_total_bytes,
            self.connection.rx_total_bytes,
        ) = (tx_bytes.unwrap_or(0), rx_bytes.unwrap_or(0));

        (
            self.connection.tx_rate_bytes_ps,
            self.connection.rx_rate_bytes_ps,
        ) = stats::transfer_rates(
            &self.connection.id,
            stats_cache,
            self.connection.tx_total_bytes,
            self.connection.rx_total_bytes,
        );

        self.connection.max_speed_bytes_ps = stats::max_speed(&self.connection.id);

        self.connection.ipv4_address =
            ip::ipv4_address(&self.connection.id, rt, bus, &self.device_proxy);
        self.connection.ipv6_address =
            ip::ipv6_address(&self.connection.id, rt, bus, &self.device_proxy);
    }
}
