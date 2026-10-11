/* src/cpu/cpufreq.rs
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

use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::read_u64;

// Track whether we're using BogoMIPS estimate for frequency
static USING_BOGOMIPS: AtomicBool = AtomicBool::new(false);

pub fn base() -> Option<u64> {
    if let Some(freq) = read_u64(
        "/sys/devices/system/cpu/cpu0/cpufreq/base_frequency",
        "base frequency",
    ) {
        return Some(freq);
    }

    if let Some(freq) = read_u64(
        "/sys/devices/system/cpu/cpu0/cpufreq/bios_limit",
        "base frequency",
    ) {
        return Some(freq);
    }

    if let Some(freq) = read_u64(
        "/sys/devices/system/cpu/cpu0/acpi_cppc/nominal_freq",
        "base frequency",
    ) {
        return Some(freq * 1000);
    }

    None
}

// Adapted from `sysinfo` crate, linux/cpu.rs:415
pub fn current() -> u64 {
    #[inline(always)]
    fn read_sys_cpufreq() -> Option<u64> {
        let mut result = 0_u64;

        let sys_dev_cpu = match std::fs::read_dir("/sys/devices/system/cpu/cpufreq") {
            Ok(d) => d,
            Err(e) => {
                log::debug!(
                    "Failed to read frequency: Failed to open `/sys/devices/system/cpu/cpufreq`: {e}"
                );
                return None;
            }
        };

        let cpu_entries = sys_dev_cpu.filter_map(|d| d.ok()).filter(|d| {
            d.file_name().as_bytes().starts_with(b"policy")
                && d.file_type().is_ok_and(|ty| ty.is_dir())
        });

        let mut buffer = String::new();
        for cpu in cpu_entries {
            buffer.clear();

            let mut path = cpu.path();
            path.push("scaling_cur_freq");

            let Some(freq) = read_u64(path.to_string_lossy().as_ref(), "current frequency") else {
                continue;
            };

            result = result.max(freq);
        }

        if result > 0 {
            Some(result / 1000)
        } else {
            None
        }
    }

    #[inline(always)]
    fn read_proc_cpuinfo() -> Option<u64> {
        let cpuinfo = match std::fs::read_to_string("/proc/cpuinfo") {
            Ok(s) => s,
            Err(e) => {
                log::debug!("Failed to read frequency: Failed to open `/proc/cpuinfo`: {e}");
                return None;
            }
        };

        let mut result = 0;
        for line in cpuinfo.split('\n').filter(|line| {
            line.starts_with("cpu MHz\t")
                || line.starts_with("CPU MHz\t")
                || line.starts_with("clock\t")
        }) {
            let freq = line
                .split(':')
                .next_back()
                .and_then(|val| val.replace("MHz", "").trim().parse::<f64>().ok())
                .map(|speed| speed as u64)
                .unwrap_or_default();
            result = result.max(freq);
        }

        if result > 0 {
            return Some(result);
        }

        // Last resort fallback: Use raw BogoMIPS value when real frequency data is unavailable.
        // This commonly occurs in VMs where the hypervisor doesn't expose CPU frequency scaling,
        // containers, cloud instances, or embedded systems across all architectures.
        // BogoMIPS is a delay loop calibration value, not a true frequency measurement.
        let mut bogomips_result = 0.0_f64;
        for line in cpuinfo
            .split('\n')
            .filter(|line| line.starts_with("BogoMIPS\t"))
        {
            if let Some(val) = line.split(':').next_back() {
                if let Ok(bogo) = val.trim().parse::<f64>() {
                    bogomips_result = bogomips_result.max(bogo);
                }
            }
        }

        if bogomips_result > 0.0 {
            log::debug!(
                "Using BogoMIPS value as frequency fallback: {:.2}",
                bogomips_result
            );
            return Some(bogomips_result as u64);
        }

        log::debug!("No CPU frequency or BogoMIPS information found in /proc/cpuinfo");
        None
    }

    if let Some(freq) = read_sys_cpufreq() {
        USING_BOGOMIPS.store(false, Ordering::Relaxed);
        return freq;
    }

    if let Some(freq) = read_proc_cpuinfo() {
        // Check if we used BogoMIPS by seeing if the value is suspiciously low
        // Real CPU frequencies are typically > 100 MHz, BogoMIPS values are typically < 100
        USING_BOGOMIPS.store(freq < 100, Ordering::Relaxed);
        return freq;
    }

    USING_BOGOMIPS.store(false, Ordering::Relaxed);
    0
}

pub fn driver() -> Option<String> {
    if USING_BOGOMIPS.load(Ordering::Relaxed) {
        return Some("bogomips".to_string());
    }

    match std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver") {
        Ok(content) => Some(content.trim().to_string()),
        Err(e) => {
            log::debug!("Could not read cpufreq driver from `/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver`: {e}");
            None
        }
    }
}

pub fn governor() -> Option<String> {
    match std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor") {
        Ok(content) => Some(content.trim().to_string()),
        Err(e) => {
            log::debug!("Could not read cpufreq governor from `/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor`: {e}");
            None
        }
    }
}

pub fn power_preference() -> Option<String> {
    match std::fs::read_to_string(
        "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
    ) {
        Ok(content) => Some(content.trim().to_string()),
        Err(e) => {
            log::debug!("Could not read power preference from `/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference`: {e}");
            None
        }
    }
}
