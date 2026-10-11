/* src/disks/stats.rs
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

use std::fmt::Write;
use std::time::Instant;

use arrayvec::ArrayString;

use crate::beginning_of_time;
use magpie_platform::disks::Disk;

const IDX_READ_IOS: usize = 0;
const IDX_READ_SECTORS: usize = 2;
const IDX_READ_TICKS: usize = 3;
const IDX_WRITE_IOS: usize = 4;
const IDX_WRITE_SECTORS: usize = 6;
const IDX_WRITE_TICKS: usize = 7;
const IDX_IO_TICKS: usize = 9;
const IDX_DISCARD_IOS: usize = 11;
const IDX_DISCARD_TICKS: usize = 14;
const IDX_FLUSH_IOS: usize = 15;
const IDX_FLUSH_TICKS: usize = 16;

pub struct Stats {
    sectors_read: u64,
    sectors_written: u64,

    read_ios: u64,
    write_ios: u64,
    discard_ios: u64,
    flush_ios: u64,
    io_total_time_ms: u64,

    read_ticks_weighted_ms: u64,
    write_ticks_weighted_ms: u64,
    discard_ticks_weighted_ms: u64,
    flush_ticks_weighted_ms: u64,

    read_time: Instant,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            sectors_read: 0,
            sectors_written: 0,

            read_ios: 0,
            write_ios: 0,
            discard_ios: 0,
            flush_ios: 0,
            io_total_time_ms: 0,

            read_ticks_weighted_ms: 0,
            write_ticks_weighted_ms: 0,
            discard_ticks_weighted_ms: 0,
            flush_ticks_weighted_ms: 0,

            read_time: beginning_of_time(),
        }
    }
}

impl Stats {
    pub fn load(device_id: &str) -> Self {
        let mut stat_path = ArrayString::<256>::new();
        if let Err(e) = write!(&mut stat_path, "/sys/block/{device_id}/stat") {
            log::warn!("Failed to format disk stat path: {e:?}");
            return Self::default();
        }

        let stats = match std::fs::read_to_string(stat_path.as_str()) {
            Ok(stats) => stats,
            Err(e) => {
                log::warn!("Failed to read disk stats: {e:?}");
                return Self::default();
            }
        };
        let stats = stats.trim();

        let mut read_ios = 0;
        let mut sectors_read = 0;
        let mut read_ticks_weighted_ms = 0;
        let mut write_ios = 0;
        let mut sectors_written = 0;
        let mut write_ticks_weighted_ms = 0;
        let mut io_total_time_ms: u64 = 0;
        let mut discard_ios = 0;
        let mut discard_ticks_weighted_ms = 0;
        let mut flush_ios = 0;
        let mut flush_ticks_weighted_ms = 0;

        for (i, entry) in stats
            .split_whitespace()
            .enumerate()
            .map(|(i, v)| (i, v.trim()))
        {
            match i {
                IDX_READ_IOS => read_ios = entry.parse::<u64>().unwrap_or(0),
                IDX_READ_SECTORS => sectors_read = entry.parse::<u64>().unwrap_or(0),
                IDX_READ_TICKS => read_ticks_weighted_ms = entry.parse::<u64>().unwrap_or(0),
                IDX_WRITE_IOS => write_ios = entry.parse::<u64>().unwrap_or(0),
                IDX_WRITE_SECTORS => sectors_written = entry.parse::<u64>().unwrap_or(0),
                IDX_WRITE_TICKS => write_ticks_weighted_ms = entry.parse::<u64>().unwrap_or(0),
                IDX_IO_TICKS => {
                    io_total_time_ms = entry.parse::<u64>().unwrap_or(0);
                }
                IDX_DISCARD_IOS => discard_ios = entry.parse::<u64>().unwrap_or(0),
                IDX_DISCARD_TICKS => discard_ticks_weighted_ms = entry.parse::<u64>().unwrap_or(0),
                IDX_FLUSH_IOS => flush_ios = entry.parse::<u64>().unwrap_or(0),
                IDX_FLUSH_TICKS => {
                    flush_ticks_weighted_ms = entry.parse::<u64>().unwrap_or(0);
                    break;
                }
                _ => (),
            }
        }

        Self {
            sectors_read,
            sectors_written,

            read_ios,
            write_ios,
            discard_ios,
            flush_ios,

            io_total_time_ms,
            read_ticks_weighted_ms,
            write_ticks_weighted_ms,
            discard_ticks_weighted_ms,
            flush_ticks_weighted_ms,

            read_time: Instant::now(),
        }
    }

    pub fn update(&mut self, other: &Stats, disk: &mut Disk) {
        let elapsed = other.read_time.elapsed().as_secs_f32();

        // Do what iostats %util does; take the time the disk had at least one ongoing request and
        // divide it by the time elapsed since the last read. This gives us the percentage of time
        // the disk was busy.
        let delta_io_time_ms = self.io_total_time_ms.saturating_sub(other.io_total_time_ms);
        // ms / (s * 1000) * 100 == ms / (s * 10)
        disk.busy_percent = (delta_io_time_ms as f32 / (elapsed * 10.0)).min(100.);

        let delta_read_ticks_weighted_ms = self
            .read_ticks_weighted_ms
            .saturating_sub(other.read_ticks_weighted_ms);
        let delta_write_ticks_weighted_ms = self
            .write_ticks_weighted_ms
            .saturating_sub(other.write_ticks_weighted_ms);
        let delta_discard_ticks_weighted_ms = self
            .discard_ticks_weighted_ms
            .saturating_sub(other.discard_ticks_weighted_ms);
        let delta_flush_ticks_weighted_ms = self
            .flush_ticks_weighted_ms
            .saturating_sub(other.flush_ticks_weighted_ms);
        let delta_ticks_weighted_ms = delta_read_ticks_weighted_ms
            + delta_write_ticks_weighted_ms
            + delta_discard_ticks_weighted_ms
            + delta_flush_ticks_weighted_ms;

        let delta_read_ios = self.read_ios.saturating_sub(other.read_ios);
        let delta_write_ios = self.write_ios.saturating_sub(other.write_ios);
        let delta_discard_ios = self.discard_ios.saturating_sub(other.discard_ios);
        let delta_flush_ios = self.flush_ios.saturating_sub(other.flush_ios);

        let delta_ios = delta_read_ios + delta_write_ios + delta_discard_ios + delta_flush_ios;
        disk.response_time_ms = if delta_ios > 0 {
            // The number of milliseconds the disk spent processing requests divided by the number
            // of requests processed gives us the average response time in milliseconds.
            delta_ticks_weighted_ms as f32 / delta_ios as f32
        } else {
            0.
        };

        let read_speed =
            (self.sectors_read.saturating_sub(other.sectors_read) as f32 * 512.) / elapsed;
        disk.rx_speed_bytes_ps = read_speed.round() as u64;

        let write_speed =
            (self.sectors_written.saturating_sub(other.sectors_written) as f32 * 512.) / elapsed;
        disk.tx_speed_bytes_ps = write_speed.round() as u64;

        disk.rx_bytes_total = self.sectors_read * 512;
        disk.tx_bytes_total = self.sectors_written * 512;
    }
}
