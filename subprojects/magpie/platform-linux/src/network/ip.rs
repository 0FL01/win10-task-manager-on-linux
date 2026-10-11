/* src/network/ip.rs
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

use tokio::runtime::Handle;
use zbus::zvariant::{Array, Dict, OwnedObjectPath};
use zbus::Proxy;

use crate::sync;

pub fn ipv4_address(
    if_name: &str,
    rt: &Handle,
    bus: &zbus::Connection,
    device_proxy: &Proxy,
) -> Option<String> {
    let ipv4_config_path = match sync!(
        rt,
        device_proxy.get_property::<OwnedObjectPath>("Ip4Config")
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get syspath for {if_name}: {e}");
            return None;
        }
    };

    let config_proxy = match sync!(
        rt,
        Proxy::new(
            bus,
            "org.freedesktop.NetworkManager",
            ipv4_config_path,
            "org.freedesktop.NetworkManager.IP4Config"
        )
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get IPv4 configuration proxy for {if_name}: {e}");
            return None;
        }
    };

    find_ip_address(if_name, rt, &config_proxy)
}

pub fn ipv6_address(
    if_name: &str,
    rt: &Handle,
    bus: &zbus::Connection,
    device_proxy: &Proxy,
) -> Option<String> {
    let ipv4_config_path = match sync!(
        rt,
        device_proxy.get_property::<OwnedObjectPath>("Ip6Config")
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get syspath for {if_name}: {e}");
            return None;
        }
    };

    let config_proxy = match sync!(
        rt,
        Proxy::new(
            bus,
            "org.freedesktop.NetworkManager",
            ipv4_config_path,
            "org.freedesktop.NetworkManager.IP6Config"
        )
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get IPv6 configuration proxy for {if_name}: {e}");
            return None;
        }
    };

    find_ip_address(if_name, rt, &config_proxy)
}

fn find_ip_address(if_name: &str, rt: &Handle, config_proxy: &Proxy) -> Option<String> {
    match sync!(rt, config_proxy.get_property::<Array>("AddressData")) {
        Ok(arr) => {
            for address in arr.inner() {
                let Ok(address) = address.downcast_ref::<Dict>() else {
                    continue;
                };

                for (key, value) in address.iter() {
                    let Ok(key) = key.downcast_ref::<&str>() else {
                        continue;
                    };

                    if key == "address" {
                        let Ok(value) = value.downcast_ref::<&str>() else {
                            continue;
                        };

                        return Some(value.to_owned());
                    }
                }
            }
        }
        Err(e) => {
            log::error!("Failed to get syspath for {if_name}: {e}");
            return None;
        }
    };

    None
}
