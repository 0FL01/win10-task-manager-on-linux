/* src/cpu/mod.rs
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

use magpie_platform::cpu::Cpu;

use crate::cpu_count;

use power_draw::PowerDraw;
use ticks::Ticks;

mod cache;
#[allow(clippy::module_inception)]
mod cpu;
mod cpufreq;
mod power_draw;
mod system;
mod ticks;
mod topology;
mod virt;

pub struct CpuCache {
    info: Cpu,
    ticks_prev: Ticks,
    core_ticks_prev: Vec<Ticks>,
    core_ids_prev: Vec<u32>,
    power_draw: Option<PowerDraw>,

    refresh_fn: fn(&mut Self),
}

impl CpuCache {
    fn update_static_info(&mut self) {
        let cpu_name = cpu::name();

        let info = &mut self.info;

        info.name = if cpu_name.is_empty() {
            None
        } else {
            Some(cpu_name.to_owned())
        };
        info.socket_count = cpu::socket_count();
        info.base_freq_khz = cpufreq::base();
        info.virtualization_technology = virt::technology();
        info.is_virtual_machine = virt::is_virtual_machine();

        let cache_info = cache::info();
        info.l1_combined_cache_bytes = cache_info[1];
        info.l2_cache_bytes = cache_info[2];
        info.l3_cache_bytes = cache_info[3];
        info.l4_cache_bytes = cache_info[4];

        self.refresh_fn = Self::refresh;
        self.refresh();
    }

    fn refresh(&mut self) {
        let info = &mut self.info;

        let proc_stat = std::fs::read_to_string("/proc/stat").unwrap_or_else(|e| {
            log::error!("Failed to read /proc/stat: {e}");
            String::new()
        });

        let mut line_iter = proc_stat
            .lines()
            .map(|l| l.trim())
            .skip_while(|l| !l.starts_with("cpu"));
        if let Some(cpu_overall_line) = line_iter.next() {
            (info.total_usage_percent, info.kernel_usage_percent) =
                self.ticks_prev.update(cpu_overall_line);

            let lines: Vec<_> = line_iter
                .take_while(|l| l.starts_with("cpu"))
                .filter_map(|line| {
                    let id = line
                        .split_whitespace()
                        .next()?
                        .strip_prefix("cpu")?
                        .parse::<u32>()
                        .ok()?;
                    Some((id, line))
                })
                .collect();
            let ids: Vec<_> = lines.iter().map(|(id, _)| *id).collect();
            if !topology::matches(info, &ids) {
                info.logical_cpus = ids.iter().map(|id| topology::read(*id)).collect();
            }
            if self.core_ids_prev != ids {
                self.core_ticks_prev = vec![Ticks::default(); ids.len()];
                for (ticks, (_, line)) in self.core_ticks_prev.iter_mut().zip(&lines) {
                    ticks.update(line);
                }
                info.core_usage_percent.resize(ids.len(), 0.);
                info.core_kernel_usage_percent.resize(ids.len(), 0.);
                self.core_ids_prev = ids;
            }
            for (i, (_, line)) in lines.iter().enumerate() {
                (
                    info.core_usage_percent[i],
                    info.core_kernel_usage_percent[i],
                ) = self.core_ticks_prev[i].update(line);
            }
        } else {
            info.total_usage_percent = 0.;
            info.kernel_usage_percent = 0.;
            info.core_usage_percent.fill(0.);
            info.core_kernel_usage_percent.fill(0.);
        }

        info.current_frequency_mhz = cpufreq::current();
        info.bclk_mhz = crate::hardware::clock_mhz(
            std::path::Path::new("/sys/class/hwmon"),
            &["BCLK", "Bus Clock"],
        );
        info.voltage_v = crate::hardware::voltage(
            std::path::Path::new("/sys/class/hwmon"),
            &["Vcore", "CPU Vcore", "SVI2_Core", "CPU Core Voltage"],
        );
        info.temperature_celsius = cpu::temperature();
        info.total_process_count = system::process_count();
        info.total_thread_count = system::thread_count();
        info.total_handle_count = system::handle_count();
        info.uptime_seconds = system::uptime().as_secs();
        info.frequency_driver = cpufreq::driver();
        info.frequency_governor = cpufreq::governor();
        info.power_preference = cpufreq::power_preference();
        info.power_draw_w = if let Some(power_draw) = &mut self.power_draw {
            power_draw.update()
        } else {
            None
        };
    }
}

impl magpie_platform::cpu::CpuCache for CpuCache {
    fn new() -> Self {
        let cpu_count = cpu_count();

        let mut info = Cpu::default();
        info.core_usage_percent.resize(cpu_count, 0.0);
        info.core_kernel_usage_percent.resize(cpu_count, 0.0);
        let mut core_ticks = Vec::with_capacity(cpu_count);
        core_ticks.resize(cpu_count, Ticks::default());

        let mut cache = Self {
            info,
            ticks_prev: Ticks::default(),
            core_ticks_prev: core_ticks,
            core_ids_prev: vec![],
            power_draw: PowerDraw::default(),

            refresh_fn: Self::update_static_info,
        };
        // Start the aggregate counter interval now, rather than reporting the machine's
        // lifetime average in the first visible sample.
        if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
            let mut lines = stat.lines();
            if let Some(line) = lines.next() {
                cache.ticks_prev.update(line);
            }
            cache.core_ticks_prev.clear();
            for line in lines.take_while(|line| line.starts_with("cpu")) {
                if let Some(id) = line
                    .split_whitespace()
                    .next()
                    .and_then(|name| name.strip_prefix("cpu"))
                    .and_then(|id| id.parse().ok())
                {
                    let mut ticks = Ticks::default();
                    ticks.update(line);
                    cache.core_ids_prev.push(id);
                    cache.core_ticks_prev.push(ticks);
                }
            }
            if !cache.core_ids_prev.is_empty() {
                cache
                    .info
                    .core_usage_percent
                    .resize(cache.core_ids_prev.len(), 0.);
                cache
                    .info
                    .core_kernel_usage_percent
                    .resize(cache.core_ids_prev.len(), 0.);
            }
        }
        cache
    }

    fn refresh(&mut self) {
        (self.refresh_fn)(self);
    }

    fn cached(&self) -> &Cpu {
        &self.info
    }
}

#[cfg(test)]
mod tests {
    use magpie_platform::cpu::CpuCache as _;

    use super::*;

    /// mission-center#542
    #[test]
    fn a_fresh_cache_is_sized_for_the_real_core_count() {
        let cpu = CpuCache::new();

        assert_eq!(cpu.cached().core_usage_percent.len(), cpu_count());
        assert_eq!(cpu.cached().core_kernel_usage_percent.len(), cpu_count());
    }
}
