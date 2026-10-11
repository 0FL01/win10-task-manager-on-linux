/* src/gpus/drm.rs
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

use std::os::fd::{AsFd, BorrowedFd};
use std::path::{Path, PathBuf};

use drm::Device as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    Pci,
    Platform,
    Other,
}

impl std::fmt::Display for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bus::Pci => f.write_str("pci"),
            Bus::Platform => f.write_str("platform"),
            Bus::Other => f.write_str("other"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DrmDevice {
    pub card_index: u32,
    pub sysname: String,
    pub bus: Bus,
    pub driver: String,
    pub drm_driver: Option<String>,
    pub sysfs_device: PathBuf,
    pub card_node: PathBuf,
    pub card_dev: (u32, u32),
    pub render_node: Option<PathBuf>,
    pub render_dev: Option<(u32, u32)>,
}

impl DrmDevice {
    pub fn is_pci(&self) -> bool {
        self.bus == Bus::Pci
    }

    pub fn drm_driver(&mut self) -> Option<&str> {
        if self.drm_driver.is_none() {
            let node = self.render_node.as_deref().unwrap_or(&self.card_node);
            self.drm_driver = Some(drm_driver_name(node).unwrap_or_default());
        }

        self.drm_driver.as_deref().filter(|name| !name.is_empty())
    }
}

pub(crate) struct DrmFile(std::fs::File);

impl AsFd for DrmFile {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl From<std::fs::File> for DrmFile {
    fn from(file: std::fs::File) -> Self {
        Self(file)
    }
}

impl drm::Device for DrmFile {}

pub fn enumerate() -> Vec<DrmDevice> {
    enumerate_in(Path::new("/sys/class/drm"), Path::new("/dev/dri"))
}

pub(crate) fn enumerate_in(sys_class_drm: &Path, dev_dri: &Path) -> Vec<DrmDevice> {
    let mut devices = vec![];

    let entries = match std::fs::read_dir(sys_class_drm) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!(
                "Unable to enumerate DRM devices in `{}`: {e}",
                sys_class_drm.display()
            );
            return devices;
        }
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let file_name = entry.file_name();
        let Some(card_name) = file_name.to_str() else {
            continue;
        };
        // Skips the per-connector entries, `card0-DP-1` and friends
        let Some(card_index) = card_name.strip_prefix("card").and_then(|i| i.parse().ok()) else {
            continue;
        };

        let card_path = entry.path();
        let device = match std::fs::canonicalize(card_path.join("device")) {
            Ok(device) => device,
            Err(e) => {
                log::debug!("No backing device for `{}`: {e}", card_path.display());
                continue;
            }
        };
        let Some(card_dev) = device_numbers(&card_path) else {
            continue;
        };

        let (render_node, render_dev) = render_node(&device, dev_dri);

        devices.push(DrmDevice {
            card_index,
            sysname: link_target_name(&device).unwrap_or_default(),
            bus: match link_target_name(&device.join("subsystem")).as_deref() {
                Some("pci") => Bus::Pci,
                Some("platform") => Bus::Platform,
                _ => Bus::Other,
            },
            driver: link_target_name(&device.join("driver")).unwrap_or_default(),
            drm_driver: None,
            sysfs_device: device,
            card_node: dev_dri.join(card_name),
            card_dev,
            render_node,
            render_dev,
        });
    }

    devices.sort_unstable_by_key(|d| d.card_index);
    devices
}

/// Reads a `dev` sysfs attribute, `226:0` into `(226, 0)`.
fn device_numbers(dir: &Path) -> Option<(u32, u32)> {
    let content = std::fs::read_to_string(dir.join("dev")).ok()?;
    let (major, minor) = content.trim().split_once(':')?;

    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn drm_driver_name(node: &Path) -> Option<String> {
    let file = match std::fs::File::open(node) {
        Ok(file) => file,
        Err(e) => {
            log::debug!("Unable to open `{}`: {e}", node.display());
            return None;
        }
    };

    match DrmFile::from(file).get_driver() {
        Ok(driver) => driver.name().to_str().map(str::to_owned),
        Err(e) => {
            log::debug!(
                "Unable to read the DRM driver name of `{}`: {e}",
                node.display()
            );
            None
        }
    }
}

pub(super) fn driver_metadata(device: &DrmDevice) -> Option<magpie_platform::gpus::DriverInfo> {
    let node = device.render_node.as_deref().unwrap_or(&device.card_node);
    let info = DrmFile::from(std::fs::File::open(node).ok()?)
        .get_driver()
        .ok()?;
    let date = info
        .date()
        .to_str()
        .filter(|date| !date.is_empty())
        .map(str::to_owned);
    Some(magpie_platform::gpus::DriverInfo {
        date,
        // The DRM ioctl reports an API version, not a kernel package version.
        version: Some(format!(
            "DRM API {}.{}.{}",
            info.version.0, info.version.1, info.version.2
        )),
        model: Some("Linux DRM".into()),
        ..Default::default()
    })
}

fn link_target_name(link: &Path) -> Option<String> {
    let target = std::fs::canonicalize(link).ok()?;
    Some(target.file_name()?.to_str()?.to_owned())
}

fn render_node(device: &Path, dev_dri: &Path) -> (Option<PathBuf>, Option<(u32, u32)>) {
    let Ok(entries) = std::fs::read_dir(device.join("drm")) else {
        return (None, None);
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.starts_with("renderD") {
            continue;
        }

        return (Some(dev_dri.join(name)), device_numbers(&entry.path()));
    }

    (None, None)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Try to set up a similar structure to a RPi device
    fn v3d_fixture(name: &str) -> Fixture {
        static SEQ: AtomicUsize = AtomicUsize::new(0);

        let unique = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "magpie-drm-test-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        let device = root.join("sys/devices/platform/1002000000.v3d");
        let class = root.join("sys/class/drm");
        let platform_bus = root.join("sys/bus/platform");

        std::fs::create_dir_all(device.join("drm/card1")).unwrap();
        std::fs::create_dir_all(device.join("drm/renderD129")).unwrap();
        std::fs::create_dir_all(device.join("drm/card1/card1-HDMI-A-1")).unwrap();
        std::fs::create_dir_all(platform_bus.join("drivers/v3d")).unwrap();
        std::fs::create_dir_all(&class).unwrap();

        std::fs::write(device.join("drm/card1/dev"), "226:1\n").unwrap();
        std::fs::write(device.join("drm/renderD129/dev"), "226:129\n").unwrap();
        std::fs::write(class.join("version"), "drm 1.1.0 20060810\n").unwrap();

        symlink("../..", device.join("drm/card1/device")).unwrap();
        symlink(&platform_bus, device.join("subsystem")).unwrap();
        symlink(platform_bus.join("drivers/v3d"), device.join("driver")).unwrap();
        symlink(device.join("drm/card1"), class.join("card1")).unwrap();
        symlink(device.join("drm/renderD129"), class.join("renderD129")).unwrap();
        symlink(
            device.join("drm/card1/card1-HDMI-A-1"),
            class.join("card1-HDMI-A-1"),
        )
        .unwrap();

        Fixture(root)
    }

    #[test]
    fn resolves_a_platform_card() {
        let fixture = v3d_fixture("platform-card");
        let devices = enumerate_in(&fixture.0.join("sys/class/drm"), Path::new("/dev/dri"));

        assert_eq!(devices.len(), 1);

        let device = &devices[0];
        assert_eq!(device.card_index, 1);
        assert_eq!(device.sysname, "1002000000.v3d");
        assert_eq!(device.bus, Bus::Platform);
        assert!(!device.is_pci());
        assert_eq!(device.driver, "v3d");
        assert_eq!(device.card_node, Path::new("/dev/dri/card1"));
        assert_eq!(device.card_dev, (226, 1));
        assert_eq!(
            device.render_node.as_deref(),
            Some(Path::new("/dev/dri/renderD129"))
        );
        assert_eq!(device.render_dev, Some((226, 129)));
    }

    #[test]
    fn missing_root_yields_no_devices() {
        assert!(
            enumerate_in(Path::new("/nonexistent/class/drm"), Path::new("/dev/dri")).is_empty()
        );
    }

    #[test]
    fn an_unreadable_drm_driver_name_is_read_once() {
        let fixture = v3d_fixture("drm-driver");
        let mut device = DrmDevice {
            card_index: 1,
            sysname: "1002000000.v3d".to_owned(),
            bus: Bus::Platform,
            driver: "v3d".to_owned(),
            drm_driver: None,
            sysfs_device: fixture.0.join("sys/devices/platform/1002000000.v3d"),
            // A regular file, so the version ioctl fails
            card_node: fixture.0.join("sys/class/drm/version"),
            card_dev: (226, 1),
            render_node: None,
            render_dev: None,
        };

        assert_eq!(device.drm_driver(), None);
        assert_eq!(device.drm_driver, Some(String::new()));
    }

    #[test]
    fn parses_device_numbers() {
        let fixture = v3d_fixture("device-numbers");
        let card = fixture
            .0
            .join("sys/devices/platform/1002000000.v3d/drm/card1");

        assert_eq!(device_numbers(&card), Some((226, 1)));

        std::fs::write(card.join("dev"), "garbage\n").unwrap();
        assert_eq!(device_numbers(&card), None);
    }
}
