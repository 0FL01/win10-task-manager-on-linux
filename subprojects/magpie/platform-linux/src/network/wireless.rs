/* src/network/wireless.rs
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
use zbus::zvariant::OwnedObjectPath;
use zbus::Proxy;

use crate::sync;
use magpie_platform::network::WirelessConnection;

pub fn wireless_connection(
    if_name: &str,
    rt: &Handle,
    device_proxy: &Proxy,
) -> Option<WirelessConnection> {
    let device_path = device_proxy.path();

    let device_proxy = match sync!(
        rt,
        Proxy::new(
            device_proxy.connection(),
            "org.freedesktop.NetworkManager",
            device_path,
            "org.freedesktop.NetworkManager.Device.Wireless"
        )
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get wireless device proxy for {if_name}: {e}");
            return None;
        }
    };

    let access_point_object_path = match sync!(
        rt,
        device_proxy.get_property::<OwnedObjectPath>("ActiveAccessPoint")
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get access point object path for {if_name}: {e}");
            return None;
        }
    };

    let access_point_proxy = match sync!(
        rt,
        Proxy::new(
            device_proxy.connection(),
            "org.freedesktop.NetworkManager",
            access_point_object_path,
            "org.freedesktop.NetworkManager.AccessPoint"
        )
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get access point proxy for {if_name}: {e}");
            return None;
        }
    };

    let ssid = match sync!(rt, access_point_proxy.get_property::<Vec<u8>>("Ssid")) {
        Ok(v) => Some(String::from_utf8_lossy(&v).to_string()),
        Err(e) => {
            log::warn!("Failed to get SSID for {if_name}: {e}");
            None
        }
    };

    let frequency_mhz = match sync!(rt, access_point_proxy.get_property::<u32>("Frequency")) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("Failed to get frequency for {if_name}: {e}");
            None
        }
    };

    let bitrate_kbps = match sync!(rt, access_point_proxy.get_property::<u32>("MaxBitrate")) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("Failed to get bitrate for {if_name}: {e}");
            None
        }
    };

    let signal_strength_percent = match sync!(rt, access_point_proxy.get_property::<u8>("Strength"))
    {
        Ok(p) => Some(p as u32),
        Err(e) => {
            log::warn!("Failed to get signal strength for {if_name}: {e}");
            None
        }
    };

    Some(WirelessConnection {
        ssid,
        frequency_mhz,
        bitrate_kbps,
        signal_strength_percent,
    })
}
