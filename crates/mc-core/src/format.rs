/* crates/mc-core/src/format.rs
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

//! Human readable formatting, ported from the GTK application's `main.rs`.

use crate::i18n::{i18n, i18n_f, ni18n_f};

// tysm gdu
pub fn to_long_human_readable_time(seconds: u64) -> String {
    const USEC_PER_YEAR: u64 = 60 * 60 * 6 * 1461; // ((60 * 60 * 24) as f32 * 365.25);
    const USEC_PER_MONTH: u64 = 60 * 30 * 1461; // ((60 * 60 * 24) as f32 * 365.25 / 12.0);
    const USEC_PER_DAY: u64 = 60 * 60 * 24;
    const USEC_PER_HOUR: u64 = 60 * 60;
    const USEC_PER_MINUTE: u64 = 60;

    fn years_to_string(time: u32) -> String {
        ni18n_f("{} year", "{} years", time, &[&time.to_string()])
    }

    fn months_to_string(time: u32) -> String {
        ni18n_f("{} month", "{} months", time, &[&time.to_string()])
    }

    fn days_to_string(time: u32) -> String {
        ni18n_f("{} day", "{} days", time, &[&time.to_string()])
    }

    fn hours_to_string(time: u32) -> String {
        ni18n_f("{} hour", "{} hours", time, &[&time.to_string()])
    }

    fn minutes_to_string(time: u32) -> String {
        ni18n_f("{} minute", "{} minutes", time, &[&time.to_string()])
    }

    fn seconds_to_string(time: u32) -> String {
        ni18n_f("{} second", "{} seconds", time, &[&time.to_string()])
    }

    let mut t = seconds;
    let years = t / USEC_PER_YEAR;
    t -= years * USEC_PER_YEAR;

    let months = t / USEC_PER_MONTH;
    t -= months * USEC_PER_MONTH;

    let days = t / USEC_PER_DAY;
    t -= days * USEC_PER_DAY;

    let hours = t / USEC_PER_HOUR;
    t -= hours * USEC_PER_HOUR;

    let minutes = t / USEC_PER_MINUTE;
    t -= minutes * USEC_PER_MINUTE;

    let seconds = t;

    let years_str = years_to_string(years as u32);
    let months_str = months_to_string(months as u32);
    let days_str = days_to_string(days as u32);
    let hours_str = hours_to_string(hours as u32);
    let minutes_str = minutes_to_string(minutes as u32);
    let seconds_str = seconds_to_string(seconds as u32);

    if years > 0 {
        /* Translators: Used for duration greater than one year. First %s is number of years, second %s is months, third %s is days */
        if days != 0 {
            i18n_f("{}, {} and {}", &[&years_str, &months_str, &days_str])
        } else if months != 0 {
            i18n_f("{} and {}", &[&years_str, &months_str])
        } else {
            years_str
        }
    } else if months > 0 {
        /* Translators: Used for durations less than one year but greater than one month. First %s is number of months, second %s is days */
        if days != 0 {
            i18n_f("{} and {}", &[&months_str, &days_str])
        } else {
            months_str
        }
    } else if days > 0 {
        /* Translators: Used for durations less than one month but greater than one day. First %s is number of days, second %s is hours */
        if hours != 0 {
            i18n_f("{} and {}", &[&days_str, &hours_str])
        } else {
            days_str
        }
    } else if hours > 0 {
        /* Translators: Used for durations less than one day but greater than one hour. First %s is number of hours, second %s is minutes */
        if minutes != 0 {
            i18n_f("{} and {}", &[&hours_str, &minutes_str])
        } else {
            hours_str
        }
    } else if minutes > 0 {
        minutes_str
    } else if seconds == 0 {
        seconds_str
    } else {
        i18n("Less than a minute")
    }
}

pub fn to_short_human_readable_time(seconds: u32) -> String {
    let mins = seconds / 60;
    let seconds = seconds % 60;

    let seconds_string = ni18n_f("{} second", "{} seconds", seconds, &[&seconds.to_string()]);

    let mins_string = ni18n_f("{} minute", "{} minutes", mins, &[&(mins).to_string()]);

    if mins > 0 {
        if seconds != 0 {
            format!("{mins_string} {seconds_string}")
        } else {
            mins_string
        }
    } else {
        seconds_string
    }
}

/// Uptime as `DD:HH:MM:SS`, as shown on the CPU page.
pub fn to_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    format!("{days:02}:{hours:02}:{minutes:02}:{seconds:02}")
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DataType {
    MemoryBytes,
    DriveBytes,
    DriveBytesPerSecond,
    NetworkBytes,
    NetworkBytesPerSecond,
    Hertz,
    Watts,
    Volts,
    WattHours,
}

/// The "bytes vs bits" and "base 2 vs base 10" unit preferences.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnitSettings {
    pub memory_use_bytes: bool,
    pub memory_use_base2: bool,
    pub drive_use_bytes: bool,
    pub drive_use_base2: bool,
    pub network_use_bytes: bool,
    pub network_use_base2: bool,
}

impl Default for UnitSettings {
    fn default() -> Self {
        Self {
            memory_use_bytes: true,
            memory_use_base2: true,
            drive_use_bytes: true,
            drive_use_base2: true,
            network_use_bytes: false,
            network_use_base2: false,
        }
    }
}

impl UnitSettings {
    fn for_type(&self, data_type: DataType) -> Option<(bool, bool)> {
        match data_type {
            DataType::MemoryBytes => Some((self.memory_use_bytes, self.memory_use_base2)),
            DataType::DriveBytes | DataType::DriveBytesPerSecond => {
                Some((self.drive_use_bytes, self.drive_use_base2))
            }
            DataType::NetworkBytes | DataType::NetworkBytesPerSecond => {
                Some((self.network_use_bytes, self.network_use_base2))
            }
            _ => None,
        }
    }
}

pub fn to_human_readable_adv_str(
    value_bytes: f32,
    use_bytes: bool,
    use_binary: bool,
    per_second: bool,
    unit_label: &str,
    min_exponent: isize,
) -> String {
    const UNITS: [&str; 9] = ["", "K", "M", "G", "T", "P", "E", "Z", "Y"];

    let divisor = if use_binary { 1024. } else { 1000. };

    let (mut value, label) = if use_bytes {
        (value_bytes, if per_second { "/s" } else { "" })
    } else {
        (value_bytes * 8., if per_second { "ps" } else { "" })
    };

    let mut exponent: usize = 0;

    // Only display bit/byte values in the given range or higher, not individual bits/bytes
    // This sacrifices some precision for the sake of readability
    while (exponent as isize) < min_exponent {
        value /= divisor;
        exponent += 1;
    }

    while value >= divisor && exponent < UNITS.len() - 1 {
        value /= divisor;
        exponent += 1;
    }

    // Calculate number of decimals to display
    // Only display fractional values for bit/byte numbers in the defined range and bigger
    // This sacrifices some precision for the sake of readability
    let dec_to_display = if exponent as isize > min_exponent {
        if value.abs() < 10.0 {
            2
        } else if value.abs() < 100.0 {
            1
        } else {
            0
        }
    } else {
        0
    };

    format!(
        "{0:.1$} {2}{3}{4}{5}",
        value,
        dec_to_display,
        UNITS[exponent],
        if use_binary { "i" } else { "" },
        unit_label,
        label
    )
}

pub fn to_human_readable_nice(
    value_bytes: f32,
    data_type: &DataType,
    units: &UnitSettings,
) -> String {
    let (label, min_dec) = match data_type {
        DataType::Hertz => ("Hz", 0),
        DataType::Watts => ("W", -1),
        DataType::Volts => ("V", -1),
        DataType::WattHours => ("Wh", -1),
        _ => {
            let (bytes, bases) = units.for_type(*data_type).unwrap_or((true, true));

            return to_human_readable_adv_str(
                value_bytes,
                bytes,
                bases,
                *data_type == DataType::NetworkBytesPerSecond
                    || *data_type == DataType::DriveBytesPerSecond,
                if bytes { "B" } else { "b" },
                1,
            );
        }
    };

    to_human_readable_adv_str(value_bytes, true, false, false, label, min_dec)
}

/// Percentage with the precision used across the UI.
pub fn percent(value: f32) -> String {
    format!("{}%", value.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_uses_binary_bytes_by_default() {
        let units = UnitSettings::default();
        assert_eq!(
            to_human_readable_nice(1536. * 1024., &DataType::MemoryBytes, &units),
            "1.50 MiB"
        );
        assert_eq!(
            to_human_readable_nice(8. * 1024. * 1024. * 1024., &DataType::MemoryBytes, &units),
            "8.00 GiB"
        );
    }

    #[test]
    fn small_values_stay_in_the_minimum_unit() {
        let units = UnitSettings::default();
        assert_eq!(
            to_human_readable_nice(512., &DataType::MemoryBytes, &units),
            "0 KiB"
        );
    }

    #[test]
    fn network_defaults_to_decimal_bits_per_second() {
        let units = UnitSettings::default();
        assert_eq!(
            to_human_readable_nice(125_000., &DataType::NetworkBytesPerSecond, &units),
            "1.00 Mbps"
        );
    }

    #[test]
    fn drive_speed_uses_bytes_per_second() {
        let units = UnitSettings::default();
        assert_eq!(
            to_human_readable_nice(10. * 1024. * 1024., &DataType::DriveBytesPerSecond, &units),
            "10.0 MiB/s"
        );
    }

    #[test]
    fn hertz_and_watts() {
        let units = UnitSettings::default();
        assert_eq!(
            to_human_readable_nice(3_500_000_000., &DataType::Hertz, &units),
            "3.50 GHz"
        );
        assert_eq!(
            to_human_readable_nice(15.25, &DataType::Watts, &units),
            "15.2 W"
        );
    }

    #[test]
    fn unit_settings_switch_bases() {
        let units = UnitSettings {
            memory_use_base2: false,
            ..Default::default()
        };
        assert_eq!(
            to_human_readable_nice(1_500_000., &DataType::MemoryBytes, &units),
            "1.50 MB"
        );
    }

    #[test]
    fn short_time() {
        assert_eq!(to_short_human_readable_time(59), "59 seconds");
        assert_eq!(to_short_human_readable_time(61), "1 minute 1 second");
        assert_eq!(to_short_human_readable_time(120), "2 minutes");
    }

    #[test]
    fn long_time() {
        assert_eq!(to_long_human_readable_time(0), "0 seconds");
        assert_eq!(to_long_human_readable_time(30), "Less than a minute");
        assert_eq!(
            to_long_human_readable_time(3600 + 120),
            "1 hour and 2 minutes"
        );
        assert_eq!(to_long_human_readable_time(86400 * 2), "2 days");
    }

    #[test]
    fn uptime() {
        assert_eq!(to_uptime(90061), "01:01:01:01");
    }
}
