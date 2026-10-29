/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Read-only, unprivileged hardware counters. Unknown labels are never guessed.
use std::path::{Path, PathBuf};

pub fn number<T: std::str::FromStr>(path: impl AsRef<Path>) -> Option<T> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

pub fn voltage(root: &Path, labels: &[&str]) -> Option<f32> {
    let mut values = Vec::new();
    let mut sensors: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    sensors.sort();
    for sensor in sensors {
        let Ok(entries) = std::fs::read_dir(&sensor) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(channel) = name.strip_suffix("_input").filter(|s| s.starts_with("in")) else {
                continue;
            };
            let label = std::fs::read_to_string(sensor.join(format!("{channel}_label")))
                .unwrap_or_default();
            if labels
                .iter()
                .any(|expected| label.trim().eq_ignore_ascii_case(expected))
            {
                if let Some(mv) =
                    number::<f32>(entry.path()).filter(|v| v.is_finite() && *v > 0. && *v <= 5000.)
                {
                    values.push(mv / 1000.);
                }
            }
        }
    }
    // A single field cannot faithfully represent several independent voltage rails.
    (values.len() == 1).then(|| values[0])
}

pub fn gpu_voltage(device: &Path) -> Option<f32> {
    let hwmon = device.join("hwmon");
    for entry in std::fs::read_dir(&hwmon).ok()?.flatten() {
        let sensor = entry.path();
        let driver = std::fs::read_to_string(sensor.join("name")).unwrap_or_default();
        if matches!(driver.trim(), "amdgpu" | "radeon" | "nouveau") {
            // These kernel drivers define in0_input as the actual GPU voltage, in mV.
            return number::<f32>(sensor.join("in0_input"))
                .filter(|v| v.is_finite() && *v > 0. && *v <= 5000.)
                .map(|v| v / 1000.);
        }
    }
    None
}

/// Device-bound (VRAM, hottest-die) temperatures, separate from the ordinary GPU sensor.
pub fn gpu_temperatures(device: &Path) -> (Option<f32>, Option<f32>) {
    let Ok(entries) = std::fs::read_dir(device.join("hwmon")) else {
        return (None, None);
    };
    for entry in entries.flatten() {
        let sensor = entry.path();
        let driver = std::fs::read_to_string(sensor.join("name")).unwrap_or_default();
        let temperature = |channel: &str, label: &str| {
            let actual = std::fs::read_to_string(sensor.join(format!("{channel}_label"))).ok()?;
            if actual.trim() != label {
                return None;
            }
            number::<i64>(sensor.join(format!("{channel}_input")))
                .map(|millidegrees| millidegrees as f32 / 1000.)
        };
        match driver.trim() {
            "amdgpu" => {
                return (
                    temperature("temp3", "mem"),
                    temperature("temp2", "junction"),
                );
            }
            // xe's temp2/pkg is a package reading, not a hottest-die sensor.
            "xe" => return (temperature("temp3", "vram"), None),
            _ => {}
        }
    }
    (None, None)
}

/// Standard hwmon frequency inputs are in Hz. Only explicitly labelled counters are accepted.
pub fn clock_mhz(root: &Path, labels: &[&str]) -> Option<f32> {
    let mut values = Vec::new();
    for sensor in std::fs::read_dir(root).ok()?.flatten() {
        let Ok(entries) = std::fs::read_dir(sensor.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(channel) = name.strip_suffix("_input").filter(|s| {
                s.strip_prefix("freq").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit())
                })
            }) else {
                continue;
            };
            let label = std::fs::read_to_string(sensor.path().join(format!("{channel}_label")))
                .unwrap_or_default();
            if labels
                .iter()
                .any(|expected| label.trim().eq_ignore_ascii_case(expected))
            {
                if let Some(hz) = number::<f32>(entry.path())
                    .filter(|v| v.is_finite() && *v > 0. && *v < 20_000_000_000.)
                {
                    values.push(hz / 1_000_000.);
                }
            }
        }
    }
    (values.len() == 1).then(|| values[0])
}

pub fn memory_errors(root: &Path) -> (Option<u64>, Option<u64>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return (None, None);
    };
    memory_errors_from(entries.map(|entry| entry.map(|entry| entry.path())))
}

fn memory_errors_from(
    entries: impl IntoIterator<Item = std::io::Result<PathBuf>>,
) -> (Option<u64>, Option<u64>) {
    let mut corrected = None::<u64>;
    let mut uncorrected = None::<u64>;
    let mut corrected_complete = true;
    let mut uncorrected_complete = true;
    for entry in entries {
        // An enumeration failure may hide a controller, so neither sum is complete.
        let Ok(path) = entry else {
            return (None, None);
        };
        let Some(name) = path.file_name() else {
            continue;
        };
        let name = name.to_string_lossy();
        if !name
            .strip_prefix("mc")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        // Only controller totals, never DIMM totals as well (which would double-count).
        if let Some(v) = number::<u64>(path.join("ce_count")) {
            corrected = Some(corrected.unwrap_or(0).saturating_add(v));
        } else {
            corrected_complete = false;
        }
        if let Some(v) = number::<u64>(path.join("ue_count")) {
            uncorrected = Some(uncorrected.unwrap_or(0).saturating_add(v));
        } else {
            uncorrected_complete = false;
        }
    }
    (
        corrected.filter(|_| corrected_complete),
        uncorrected.filter(|_| uncorrected_complete),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "taskmanager-hardware-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn hwmon_units_labels_and_ambiguous_rails() {
        let dir = TempDir::new();
        let sensor = dir.path().join("hwmon0");
        std::fs::create_dir(&sensor).unwrap();
        std::fs::write(sensor.join("in0_label"), "Vcore\n").unwrap();
        std::fs::write(sensor.join("in0_input"), "1250\n").unwrap();
        assert_eq!(voltage(dir.path(), &["Vcore"]), Some(1.25));
        assert_eq!(voltage(dir.path(), &["DRAM"]), None);
        std::fs::write(sensor.join("in1_label"), "Vcore").unwrap();
        std::fs::write(sensor.join("in1_input"), "1300").unwrap();
        assert_eq!(voltage(dir.path(), &["Vcore"]), None);
        std::fs::write(sensor.join("freq1_label"), "FCLK").unwrap();
        std::fs::write(sensor.join("freq1_input"), "1800000000").unwrap();
        assert_eq!(clock_mhz(dir.path(), &["FCLK"]), Some(1800.));
        assert_eq!(clock_mhz(dir.path(), &["MCLK"]), None);
    }

    #[test]
    fn gpu_temperatures_use_signed_millidegrees_and_fail_independently() {
        let dir = TempDir::new();
        assert_eq!(gpu_temperatures(dir.path()), (None, None));
        let sensor = dir.path().join("hwmon/hwmon37");
        std::fs::create_dir_all(&sensor).unwrap();
        for (name, value) in [
            ("name", "amdgpu\n"),
            ("temp2_label", "junction\n"),
            ("temp2_input", "112500\n"),
            ("temp3_label", "mem\n"),
            ("temp3_input", "0\n"),
        ] {
            std::fs::write(sensor.join(name), value).unwrap();
        }
        assert_eq!(gpu_temperatures(dir.path()), (Some(0.), Some(112.5)));
        std::fs::write(sensor.join("temp2_input"), "-1250").unwrap();
        std::fs::write(sensor.join("temp3_input"), "not a temperature").unwrap();
        assert_eq!(gpu_temperatures(dir.path()), (None, Some(-1.25)));
        std::fs::remove_file(sensor.join("temp2_input")).unwrap();
        std::fs::write(sensor.join("temp3_input"), "63500").unwrap();
        assert_eq!(gpu_temperatures(dir.path()), (Some(63.5), None));
        std::fs::create_dir(sensor.join("temp2_input")).unwrap();
        assert_eq!(gpu_temperatures(dir.path()), (Some(63.5), None));
        std::fs::write(sensor.join("temp3_label"), "vram").unwrap();
        assert_eq!(gpu_temperatures(dir.path()), (None, None));
    }

    #[test]
    fn gpu_temperatures_stay_with_the_device_and_known_driver_labels() {
        let dir = TempDir::new();
        let amd = dir.path().join("amd");
        let intel = dir.path().join("intel");
        for (device, driver, memory_label) in [(&amd, "amdgpu", "mem"), (&intel, "xe", "vram")] {
            let sensor = device.join("hwmon/hwmon21");
            std::fs::create_dir_all(&sensor).unwrap();
            for (name, value) in [
                ("name", driver),
                ("temp2_label", "pkg"),
                ("temp2_input", "80000"),
                ("temp3_label", memory_label),
                ("temp3_input", "45000"),
            ] {
                std::fs::write(sensor.join(name), value).unwrap();
            }
        }
        assert_eq!(gpu_temperatures(&intel), (Some(45.), None));
        assert_eq!(gpu_temperatures(&amd), (Some(45.), None)); // pkg is not junction.
        std::fs::write(amd.join("hwmon/hwmon21/temp2_label"), "junction").unwrap();
        std::fs::write(amd.join("hwmon/hwmon21/name"), "k10temp").unwrap();
        assert_eq!(gpu_temperatures(&amd), (None, None));
        std::fs::remove_file(intel.join("hwmon/hwmon21/temp3_input")).unwrap();
        assert_eq!(gpu_temperatures(&intel), (None, None)); // No other-device fallback.
        assert_eq!(gpu_temperatures(dir.path()), (None, None));
    }

    #[test]
    fn edac_totals_do_not_double_count_dimms_or_invent_zero() {
        let dir = TempDir::new();
        assert_eq!(memory_errors(dir.path()), (None, None));
        let controller = dir.path().join("mc0");
        std::fs::create_dir_all(controller.join("dimm0")).unwrap();
        std::fs::write(controller.join("ce_count"), "7").unwrap();
        std::fs::write(controller.join("ue_count"), "0").unwrap();
        std::fs::write(controller.join("dimm0/ce_count"), "7").unwrap();
        assert_eq!(memory_errors(dir.path()), (Some(7), Some(0)));
        std::fs::create_dir(dir.path().join("mc1")).unwrap();
        std::fs::write(dir.path().join("mc1/ce_count"), "3").unwrap();
        assert_eq!(memory_errors(dir.path()), (Some(10), None));
    }

    #[test]
    fn disappearing_hwmon_directory_does_not_hide_a_readable_clock() {
        let dir = TempDir::new();
        std::os::unix::fs::symlink(dir.path().join("removed"), dir.path().join("hwmon0")).unwrap();
        let sensor = dir.path().join("hwmon1");
        std::fs::create_dir(&sensor).unwrap();
        std::fs::write(sensor.join("freq1_label"), "MCLK").unwrap();
        std::fs::write(sensor.join("freq1_input"), "1600000000").unwrap();
        assert_eq!(clock_mhz(dir.path(), &["MCLK"]), Some(1600.));
    }

    #[test]
    fn edac_iterator_error_invalidates_both_partial_sums() {
        let dir = TempDir::new();
        let controller = dir.path().join("mc0");
        std::fs::create_dir(&controller).unwrap();
        std::fs::write(controller.join("ce_count"), "7").unwrap();
        std::fs::write(controller.join("ue_count"), "2").unwrap();
        assert_eq!(
            memory_errors_from([
                Ok(controller),
                Err(std::io::Error::other("controller enumeration failed")),
            ]),
            (None, None)
        );
    }
}
