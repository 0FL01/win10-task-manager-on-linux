/* src/fan/mod.rs
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

//! hwmon fans and temperature sensors. Fan and temperature channels are independent: a shared
//! channel number does not mean the sensor measures what the fan cools.

use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use magpie_platform::fan::{Fan, HwmonChip, TemperatureSensor};

use crate::config_dir;

/// Hotplugged chips (USB controllers, GPUs) appear without restarting the daemon.
const RESCAN_INTERVAL: Duration = Duration::from_secs(10);

struct Channel {
    chip: usize,
    index: u32,
    has_fault: bool,
}

struct Chip {
    dir: PathBuf,
    /// Per-driver overrides in `<config>/fans/<name>/`
    config_path: Option<Box<Path>>,
    info: HwmonChip,
}

impl Chip {
    fn read(&self, file: &str) -> Option<String> {
        let path = match &self.config_path {
            Some(config) if config.join(file).exists() => config.join(file),
            _ => self.dir.join(file),
        };
        Some(read_to_string(path).ok()?.trim().to_owned())
    }

    fn label(&self, file: &str) -> Option<String> {
        // do not change the case here. we report how it is, not how it ought to be.
        self.read(file).filter(|label| !label.is_empty())
    }

    fn number<T: std::str::FromStr>(&self, file: &str) -> Option<T> {
        self.read(file)?.parse().ok()
    }

    fn faulted(&self, channel: &Channel, kind: &str) -> bool {
        channel.has_fault
            && self
                .read(&format!("{kind}{}_fault", channel.index))
                .as_deref()
                == Some("1")
    }
}

pub struct FanCache {
    root: PathBuf,
    /// A complete virtual hwmon directory replacing discovery (`<config>/hwmon0`)
    virtual_chip: Option<PathBuf>,
    chips: Vec<Chip>,
    fan_channels: Vec<Channel>,
    temp_channels: Vec<Channel>,
    fans: Vec<Fan>,
    temperatures: Vec<TemperatureSensor>,
    scanned: Option<Instant>,
}

fn hwmon_index(dir: &Path) -> Option<u32> {
    let name = dir.file_name()?.to_str()?;
    let digits = name.strip_prefix("hwmon")?;
    (!digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// Indices of exactly `<prefix><digits>_input`; drivers may leave gaps (`temp1`, `temp3`).
fn channel_indices(dir: &Path, prefix: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut indices: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let digits = name
                .to_str()?
                .strip_prefix(prefix)?
                .strip_suffix("_input")?
                .to_owned();
            (!digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()))
                .then(|| digits.parse().ok())
                .flatten()
        })
        .collect();
    indices.sort_unstable();
    indices
}

fn is_pci_address(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 12
        && bytes[4] == b':'
        && bytes[7] == b':'
        && bytes[10] == b'.'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, c)| matches!(i, 4 | 7 | 10) || c.is_ascii_hexdigit())
}

fn identify(dir: &Path, hwmon_index: u32) -> HwmonChip {
    let name = read_to_string(dir.join("name"))
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    let device = std::fs::canonicalize(dir.join("device")).ok();
    let pci_slot = device.as_ref().and_then(|device| {
        device
            .ancestors()
            .filter_map(|path| path.file_name()?.to_str())
            .find(|name| is_pci_address(name))
            .map(str::to_owned)
    });
    let drm_card = device.as_ref().and_then(|device| {
        std::fs::read_dir(device.join("drm"))
            .ok()?
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| {
                name.strip_prefix("card").is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())
                })
            })
            .min()
    });
    let device_model = device.as_ref().and_then(|device| {
        read_to_string(device.join("model"))
            .ok()
            .map(|model| model.trim().to_owned())
            .filter(|model| !model.is_empty())
    });
    HwmonChip {
        hwmon_index,
        name,
        device_path: device.map(|path| path.display().to_string()),
        pci_slot,
        drm_card,
        device_model,
    }
}

/// Signed millidegrees Celsius; values outside the wire range are not temperatures.
fn millicelsius(chip: &Chip, file: &str) -> Option<i32> {
    chip.number::<i64>(file)
        .and_then(|value| i32::try_from(value).ok())
}

impl FanCache {
    fn with_root(root: PathBuf, virtual_chip: Option<PathBuf>) -> Self {
        Self {
            root,
            virtual_chip,
            chips: vec![],
            fan_channels: vec![],
            temp_channels: vec![],
            fans: vec![],
            temperatures: vec![],
            scanned: None,
        }
    }

    /// A failed listing keeps the previous inventory: unreadable is not "no sensors".
    fn scan(&mut self) -> bool {
        let mut dirs: Vec<(u32, PathBuf)> = match &self.virtual_chip {
            Some(dir) => vec![(hwmon_index(dir).unwrap_or(0), dir.clone())],
            None => match std::fs::read_dir(&self.root) {
                Ok(entries) => entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter_map(|dir| Some((hwmon_index(&dir)?, dir)))
                    .collect(),
                Err(e) => {
                    log::warn!("Failed to read {}: {e}", self.root.display());
                    return false;
                }
            },
        };
        dirs.sort();

        self.chips.clear();
        self.fan_channels.clear();
        self.temp_channels.clear();
        self.fans.clear();
        self.temperatures.clear();
        for (index, dir) in dirs {
            let info = identify(&dir, index);
            let config_path = info
                .name
                .as_ref()
                .and_then(|name| config_dir::build_path(&format!("fans/{name}")));
            let chip = Chip {
                dir,
                config_path,
                info,
            };
            let chip_index = self.chips.len();
            for fan in channel_indices(&chip.dir, "fan") {
                // A header without a readable input is not a fan
                if chip.number::<u32>(&format!("fan{fan}_input")).is_none() {
                    continue;
                }
                self.fan_channels.push(Channel {
                    chip: chip_index,
                    index: fan,
                    has_fault: chip.dir.join(format!("fan{fan}_fault")).exists(),
                });
                self.fans.push(Fan {
                    fan_label: chip.label(&format!("fan{fan}_label")),
                    temp_name: None,
                    fan_index: fan,
                    hwmon_index: index,
                    rpm: 0,
                    temp_amount: None,
                    pwm_percent: None,
                    max_rpm: chip
                        .number::<u32>(&format!("fan{fan}_max"))
                        .filter(|max| *max > 0),
                    chip: Some(chip.info.clone()),
                    rpm_valid: Some(false),
                });
            }
            for temp in channel_indices(&chip.dir, "temp") {
                // Absent probes commonly fail with ENODATA/EINVAL rather than disappearing
                if chip.number::<i64>(&format!("temp{temp}_input")).is_none() {
                    continue;
                }
                self.temp_channels.push(Channel {
                    chip: chip_index,
                    index: temp,
                    has_fault: chip.dir.join(format!("temp{temp}_fault")).exists(),
                });
                self.temperatures.push(TemperatureSensor {
                    temp_index: temp,
                    chip: Some(chip.info.clone()),
                    label: chip.label(&format!("temp{temp}_label")),
                    input_millicelsius: None,
                    max_millicelsius: millicelsius(&chip, &format!("temp{temp}_max")),
                    crit_millicelsius: millicelsius(&chip, &format!("temp{temp}_crit")),
                });
            }
            self.chips.push(chip);
        }
        true
    }

    fn sample(&mut self) {
        for (fan, channel) in self.fans.iter_mut().zip(&self.fan_channels) {
            let chip = &self.chips[channel.chip];
            let rpm = chip
                .number::<u32>(&format!("fan{}_input", channel.index))
                .filter(|_| !chip.faulted(channel, "fan"));
            fan.rpm = rpm.unwrap_or_default();
            fan.rpm_valid = Some(rpm.is_some());
            fan.pwm_percent = chip
                .number::<u64>(&format!("pwm{}", channel.index))
                .filter(|pwm| *pwm <= 255)
                .map(|pwm| pwm as f32 / 255.);
        }
        for (sensor, channel) in self.temperatures.iter_mut().zip(&self.temp_channels) {
            let chip = &self.chips[channel.chip];
            sensor.input_millicelsius = millicelsius(chip, &format!("temp{}_input", channel.index))
                .filter(|_| !chip.faulted(channel, "temp"));
        }
    }
}

impl magpie_platform::fan::FanCache for FanCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self::with_root(
            PathBuf::from("/sys/class/hwmon"),
            config_dir::build_path("hwmon0").map(PathBuf::from),
        )
    }

    fn refresh(&mut self) {
        if self
            .scanned
            .is_none_or(|scanned| scanned.elapsed() >= RESCAN_INTERVAL)
            && self.scan()
        {
            self.scanned = Some(Instant::now());
        }
        self.sample();
    }

    fn cached_entries(&self) -> &[Fan] {
        &self.fans
    }

    fn cached_temperatures(&self) -> &[TemperatureSensor] {
        &self.temperatures
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use magpie_platform::fan::FanCache as _;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "taskmanager-fan-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(dir: &Path, files: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        for (name, value) in files {
            std::fs::write(dir.join(name), value).unwrap();
        }
    }

    #[test]
    fn host_cache_refreshes() {
        let mut cache = super::FanCache::new();
        cache.refresh();
    }

    #[test]
    fn independent_channels_identity_and_validity() {
        let dir = TempDir::new();
        let root = dir.0.join("class");
        let gpu = dir.0.join("devices/pci0000:00/0000:00:03.1/0000:2d:00.0");
        let nvme = dir.0.join("devices/pci0000:00/0000:01:00.0/nvme/nvme0");
        write(&gpu.join("drm/card1"), &[]);
        write(&gpu.join("drm/renderD128"), &[]);
        write(&nvme, &[("model", "SAMSUNG MZVL21T0HCLR-00BL2 \n")]);

        write(
            &root.join("hwmon4"),
            &[
                ("name", "nct6687\n"),
                ("fan1_input", "742\n"),
                ("fan1_label", "CPU Fan\n"),
                ("pwm1", "38\n"),
                ("fan2_input", "0\n"),
                ("fan2_label", "Pump Fan\n"),
                ("pwm2", "255\n"),
                ("fan3_input", "garbage"),
                ("temp1_input", "50000\n"),
                ("temp1_label", "CPU\n"),
                ("temp2_input", "-1500\n"),
                ("temp2_label", "System\n"),
                ("temp7_input", "0\n"),
                ("temp7_label", "M2_1\n"),
                ("temp7_max", "65262000000\n"),
            ],
        );
        write(
            &root.join("hwmon1"),
            &[
                ("name", "amdgpu\n"),
                ("fan1_input", "565\n"),
                ("fan1_max", "3300\n"),
                ("pwm1", "24\n"),
                ("temp1_input", "36000\n"),
                ("temp1_label", "edge\n"),
                ("temp1_crit", "100000\n"),
            ],
        );
        std::os::unix::fs::symlink(&gpu, root.join("hwmon1/device")).unwrap();
        write(
            &root.join("hwmon0"),
            &[("name", "nvme\n"), ("temp1_input", "44850\n")],
        );
        std::os::unix::fs::symlink(&nvme, root.join("hwmon0/device")).unwrap();
        write(
            &root.join("hwmon2"),
            &[
                ("name", "k10temp\n"),
                ("temp1_input", "51000\n"),
                ("temp3_input", "51750\n"),
                ("temp3_label", "Tccd1\n"),
                ("temp3_fault", "0\n"),
            ],
        );
        write(
            &root.join("not-hwmon"),
            &[("name", "x"), ("fan1_input", "1")],
        );

        let mut cache = super::FanCache::with_root(root.clone(), None);
        cache.refresh();
        let fans = cache.cached_entries();
        // The unparsable header is not a fan; order is hwmon then channel
        assert_eq!(
            fans.iter()
                .map(|f| (f.hwmon_index, f.fan_index, f.rpm, f.rpm_valid))
                .collect::<Vec<_>>(),
            [
                (1, 1, 565, Some(true)),
                (4, 1, 742, Some(true)),
                (4, 2, 0, Some(true))
            ]
        );
        assert!(fans
            .iter()
            .all(|f| f.temp_name.is_none() && f.temp_amount.is_none()));
        assert_eq!(fans[0].max_rpm, Some(3300));
        assert_eq!(fans[1].fan_label.as_deref(), Some("CPU Fan"));
        assert!((fans[2].pwm_percent.unwrap() - 1.).abs() < f32::EPSILON);
        let gpu_chip = fans[0].chip.as_ref().unwrap();
        assert_eq!(gpu_chip.name.as_deref(), Some("amdgpu"));
        assert_eq!(gpu_chip.pci_slot.as_deref(), Some("0000:2d:00.0"));
        assert_eq!(gpu_chip.drm_card.as_deref(), Some("card1"));
        assert_eq!(fans[1].chip.as_ref().unwrap().pci_slot, None);

        let temps = cache.cached_temperatures();
        let summary: Vec<_> = temps
            .iter()
            .map(|t| {
                (
                    t.chip.as_ref().unwrap().hwmon_index,
                    t.temp_index,
                    t.label.clone(),
                    t.input_millicelsius,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (0, 1, None, Some(44850)),
                (1, 1, Some("edge".into()), Some(36000)),
                (2, 1, None, Some(51000)),
                (2, 3, Some("Tccd1".into()), Some(51750)),
                (4, 1, Some("CPU".into()), Some(50000)),
                (4, 2, Some("System".into()), Some(-1500)),
                (4, 7, Some("M2_1".into()), Some(0)),
            ]
        );
        let nvme_chip = temps[0].chip.as_ref().unwrap();
        assert_eq!(
            nvme_chip.device_model.as_deref(),
            Some("SAMSUNG MZVL21T0HCLR-00BL2")
        );
        assert_eq!(nvme_chip.pci_slot.as_deref(), Some("0000:01:00.0"));
        assert_eq!(temps[1].crit_millicelsius, Some(100000));
        // Out of the wire range: not a temperature
        assert_eq!(temps[6].max_millicelsius, None);

        // Faults and unreadable inputs are unknown, never zero
        write(
            &root.join("hwmon2"),
            &[("temp3_fault", "1\n"), ("temp1_input", "x")],
        );
        std::fs::write(root.join("hwmon4/fan1_input"), "").unwrap();
        cache.refresh();
        assert_eq!(cache.cached_temperatures()[2].input_millicelsius, None);
        assert_eq!(cache.cached_temperatures()[3].input_millicelsius, None);
        assert_eq!(cache.cached_entries()[1].rpm_valid, Some(false));
        assert_eq!(cache.cached_entries()[2].rpm_valid, Some(true));
    }

    #[test]
    fn rescan_finds_hotplug_and_failed_listing_keeps_inventory() {
        let dir = TempDir::new();
        let root = dir.0.join("class");
        write(
            &root.join("hwmon0"),
            &[("name", "a"), ("temp1_input", "1000")],
        );
        let mut cache = super::FanCache::with_root(root.clone(), None);
        cache.refresh();
        assert_eq!(cache.cached_temperatures().len(), 1);

        write(
            &root.join("hwmon5"),
            &[("name", "b"), ("fan2_input", "900")],
        );
        cache.refresh();
        assert!(
            cache.cached_entries().is_empty(),
            "rescans are rate limited"
        );
        cache.scanned = None;
        cache.refresh();
        assert_eq!(cache.cached_entries()[0].fan_index, 2);

        std::fs::remove_dir_all(&root).unwrap();
        cache.scanned = None;
        cache.refresh();
        assert_eq!(cache.cached_entries().len(), 1);
        assert_eq!(cache.cached_entries()[0].rpm_valid, Some(false));
        assert_eq!(cache.cached_temperatures()[0].input_millicelsius, None);
    }

    #[test]
    fn virtual_chip_replaces_discovery() {
        let dir = TempDir::new();
        let root = dir.0.join("class");
        write(
            &root.join("hwmon3"),
            &[("name", "real"), ("fan1_input", "1")],
        );
        let fake = dir.0.join("config/hwmon0");
        write(&fake, &[("name", "fake"), ("fan4_input", "1200")]);
        let mut cache = super::FanCache::with_root(root, Some(fake));
        cache.refresh();
        let fans = cache.cached_entries();
        assert_eq!(fans.len(), 1);
        assert_eq!(
            (fans[0].hwmon_index, fans[0].fan_index, fans[0].rpm),
            (0, 4, 1200)
        );
    }
}
