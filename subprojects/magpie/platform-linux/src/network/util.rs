/* src/network/utils.rs
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

use std::path::Path;

use magpie_platform::network::ConnectionKind;

pub fn connection_kind_from_nm_id(if_type: u64) -> ConnectionKind {
    match if_type {
        // NM_DEVICE_TYPE_UNKNOWN
        0 => ConnectionKind::Other,
        // NM_DEVICE_TYPE_ETHERNET
        1 => ConnectionKind::Wired,
        // NM_DEVICE_TYPE_WIFI
        2 => ConnectionKind::Wireless,
        // NM_DEVICE_TYPE_UNUSED1
        3 => ConnectionKind::Other,
        // NM_DEVICE_TYPE_UNUSED2
        4 => ConnectionKind::Other,
        // NM_DEVICE_TYPE_BT
        5 => ConnectionKind::Bluetooth,
        // NM_DEVICE_TYPE_OLPC_MESH
        // 6 => ConnectionKind::,
        // NM_DEVICE_TYPE_WIMAX
        // 7 => ConnectionKind::,
        // NM_DEVICE_TYPE_MODEM
        8 => ConnectionKind::Wwan,
        // NM_DEVICE_TYPE_INFINIBAND
        9 => ConnectionKind::InfiniBand,
        // NM_DEVICE_TYPE_BOND
        // 10 => ConnectionKind::,
        // NM_DEVICE_TYPE_VLAN
        // 11 => ConnectionKind::,
        // NM_DEVICE_TYPE_ADSL
        // 12 => ConnectionKind::,
        // NM_DEVICE_TYPE_BRIDGE
        13 => ConnectionKind::Bridge,
        // NM_DEVICE_TYPE_GENERIC
        14 => ConnectionKind::Other,
        // NM_DEVICE_TYPE_TEAM
        // 15 => ConnectionKind::,
        // NM_DEVICE_TYPE_TUN
        // 16 => ConnectionKind::,
        // NM_DEVICE_TYPE_IP_TUNNEL
        // 17 => ConnectionKind::,
        // NM_DEVICE_TYPE_MACVLAN
        // 18 => ConnectionKind::,
        // NM_DEVICE_TYPE_VXLAN
        // 19 => ConnectionKind::,
        // NM_DEVICE_TYPE_VETH
        // 20 => ConnectionKind::,
        // NM_DEVICE_TYPE_MACSEC
        // 21 => ConnectionKind::,
        // NM_DEVICE_TYPE_DUMMY
        // 22 => ConnectionKind::,
        // NM_DEVICE_TYPE_PPP
        // 23 => ConnectionKind::,
        // NM_DEVICE_TYPE_OVS_INTERFACE
        // 24 => ConnectionKind::,
        // NM_DEVICE_TYPE_OVS_PORT
        // 25 => ConnectionKind::,
        // NM_DEVICE_TYPE_OVS_BRIDGE
        // 26 => ConnectionKind::,
        // NM_DEVICE_TYPE_WPAN
        // 27 => ConnectionKind::,
        // NM_DEVICE_TYPE_6LOWPAN
        // 28 => ConnectionKind::,
        // NM_DEVICE_TYPE_WIREGUARD
        29 => ConnectionKind::Vpn,
        // NM_DEVICE_TYPE_WIFI_P2P
        // 30 => ConnectionKind::,
        // NM_DEVICE_TYPE_VRF
        // 31 => ConnectionKind::,
        // NM_DEVICE_TYPE_LOOPBACK
        // 32 => ConnectionKind::,
        // NM_DEVICE_TYPE_HSR
        // 33 => ConnectionKind::,
        // NM_DEVICE_TYPE_IPVLAN
        // 34 => ConnectionKind::,
        _ => ConnectionKind::Other,
    }
}

pub fn connection_kind_from_name(if_name: &str) -> ConnectionKind {
    if if_name.starts_with("bn") {
        ConnectionKind::Bluetooth
    } else if if_name.starts_with("br") || if_name.starts_with("virbr") {
        ConnectionKind::Bridge
    } else if if_name.starts_with("docker") {
        ConnectionKind::Docker
    } else if if_name.starts_with("eth") || if_name.starts_with("en") {
        ConnectionKind::Wired
    } else if if_name.starts_with("ib") {
        ConnectionKind::InfiniBand
    } else if if_name.starts_with("mp") {
        ConnectionKind::Multipass
    } else if if_name.starts_with("veth") {
        ConnectionKind::Virtual
    } else if if_name.starts_with("vpn") || if_name.starts_with("wg") {
        ConnectionKind::Vpn
    } else if if_name.starts_with("wl") || if_name.starts_with("ww") {
        ConnectionKind::Wireless
    } else if if_name.starts_with("mlan") {
        let path = Path::new("/sys/class/net").join(if_name).join("wireless");
        if path.exists() {
            ConnectionKind::Wireless
        } else {
            ConnectionKind::Other
        }
    } else {
        ConnectionKind::Other
    }
}
