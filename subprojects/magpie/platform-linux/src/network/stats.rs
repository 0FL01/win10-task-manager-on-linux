/* src/network/stats.rs
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
use std::fmt::Write;

use super::TransferStats;
use crate::read_u64;
use arrayvec::ArrayString;

pub fn max_speed(if_name: &str) -> Option<u64> {
    let mut path: ArrayString<256> = ArrayString::new();
    let _ = write!(&mut path, "/sys/class/net/{if_name}/speed");

    read_u64(path.as_str(), "max device speed").map(|speed| (speed * 1_000_000) / 8)
}

pub fn bytes_transfered(if_name: &str) -> (Option<u64>, Option<u64>) {
    let mut path: ArrayString<256> = ArrayString::new();
    let _ = write!(&mut path, "/sys/class/net/{if_name}/statistics/tx_bytes");
    let tx_bytes = read_u64(path.as_str(), "bytes transmitted");

    path.clear();
    let _ = write!(&mut path, "/sys/class/net/{if_name}/statistics/rx_bytes");
    let rx_bytes = read_u64(path.as_str(), "bytes received");

    (tx_bytes, rx_bytes)
}

pub fn transfer_rates(
    if_name: &str,
    stats_cache: &mut HashMap<String, TransferStats>,
    tx_total_bytes: u64,
    rx_total_bytes: u64,
) -> (f32, f32) {
    let mut tx_rate_bps = 0.0;
    let mut rx_rate_bps = 0.0;

    if let Some(cached_stats) = stats_cache.get_mut(if_name) {
        let prev_tx_bytes = if cached_stats.tx_bytes > tx_total_bytes {
            tx_total_bytes
        } else {
            cached_stats.tx_bytes
        };

        let prev_rx_bytes = if cached_stats.rx_bytes > rx_total_bytes {
            rx_total_bytes
        } else {
            cached_stats.rx_bytes
        };

        let elapsed = cached_stats.update_timestamp.elapsed().as_secs_f32();
        tx_rate_bps = (tx_total_bytes - prev_tx_bytes) as f32 / elapsed;
        rx_rate_bps = (rx_total_bytes - prev_rx_bytes) as f32 / elapsed;

        cached_stats.tx_bytes = tx_total_bytes;
        cached_stats.rx_bytes = rx_total_bytes;
        cached_stats.update_timestamp = std::time::Instant::now();
    } else {
        stats_cache.insert(
            if_name.to_string(),
            TransferStats {
                tx_bytes: tx_total_bytes,
                rx_bytes: rx_total_bytes,
                update_timestamp: std::time::Instant::now(),
            },
        );
    }

    (tx_rate_bps, rx_rate_bps)
}
