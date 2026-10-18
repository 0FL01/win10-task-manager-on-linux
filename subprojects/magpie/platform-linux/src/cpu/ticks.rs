/* src/cpu/ticks.rs
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

const PROC_STAT_IGNORE: [usize; 3] = [0, 9, 10];
const PROC_STAT_IDLE: [usize; 2] = [4, 5];
const PROC_STAT_KERNEL: [usize; 3] = [3, 6, 7];

#[derive(Debug, Copy, Clone, Default)]
pub struct Ticks {
    used: u64,
    idle: u64,
    kernel: u64,
}

impl Ticks {
    pub fn update(&mut self, line: &str) -> (f32, f32) {
        let failure = |e| {
            log::error!("Failed to read /proc/stat: {}", e);
            0
        };
        let fields = line.split_whitespace();
        let mut new_ticks = Ticks::default();
        for (pos, field) in fields.enumerate() {
            match pos {
                x if PROC_STAT_IGNORE.contains(&x) => (),
                x if PROC_STAT_IDLE.contains(&x) => {
                    new_ticks.idle += field.trim().parse::<u64>().unwrap_or_else(failure)
                }
                x if PROC_STAT_KERNEL.contains(&x) => {
                    new_ticks.kernel += field.trim().parse::<u64>().unwrap_or_else(failure)
                }
                _ => new_ticks.used += field.trim().parse::<u64>().unwrap_or_else(failure),
            }
        }

        let used = new_ticks.used.saturating_sub(self.used);
        let kernel = new_ticks.kernel.saturating_sub(self.kernel);
        let idle = new_ticks.idle.saturating_sub(self.idle);
        *self = new_ticks;

        let total = (used + kernel + idle) as f32;
        if total == 0. {
            return (0., 0.);
        }
        (
            (used + kernel) as f32 / total * 100.0,
            kernel as f32 / total * 100.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_counters_are_finite_and_deltas_are_not_boot_averages() {
        let mut ticks = Ticks::default();
        let baseline = "cpu 100 0 0 900 0 0 0 0 0 0";
        ticks.update(baseline);
        assert_eq!(ticks.update(baseline), (0., 0.));
        assert_eq!(ticks.update("cpu 120 0 0 920 0 0 0 0 0 0"), (50., 0.));
    }
}
