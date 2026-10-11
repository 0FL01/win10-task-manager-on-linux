/* src/processes/raw_stats.rs
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

use std::time::Instant;

#[derive(Debug, Copy, Clone)]
pub struct RawStats {
    pub user_jiffies: u64,
    pub kernel_jiffies: u64,

    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,

    pub net_bytes_sent: u64,
    pub net_bytes_recv: u64,

    pub timestamp: Instant,
}

impl Default for RawStats {
    fn default() -> Self {
        Self {
            user_jiffies: 0,
            kernel_jiffies: 0,

            disk_read_bytes: 0,
            disk_write_bytes: 0,

            net_bytes_sent: 0,
            net_bytes_recv: 0,

            timestamp: Instant::now(),
        }
    }
}
