/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Donor-inspired information grouping in the existing Win10 label/value presentation.
use super::mem_slots::{capacity_label, known_capacity, slot_is_empty};
use super::{DevicePage, PageView, SummaryGraph, UpdateEnv};
use mc_core::{
    config::Config,
    format::{DataType, to_human_readable_nice},
    i18n::{i18n, n_},
    sidebar::DeviceType,
};
use mc_magpie::{About, Readings};

pub struct SystemPage {
    summary: SummaryGraph,
    about: Option<About>,
    hardware: Vec<(String, String)>,
    identity: Vec<(String, String)>,
    identity_position: usize,
    rows: Vec<(String, String)>,
}

impl Default for SystemPage {
    fn default() -> Self {
        Self::new()
    }
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || [
            "none",
            "unknown",
            "not specified",
            "default string",
            "to be filled by o.e.m.",
            "to be filled by oem",
        ]
        .iter()
        .any(|placeholder| value.eq_ignore_ascii_case(placeholder))
    {
        None
    } else {
        Some(value.into())
    }
}

impl SystemPage {
    pub fn new() -> Self {
        Self {
            summary: SummaryGraph::new(crate::win10::palette::CPU.line, i18n("System")),
            about: None,
            hardware: vec![],
            identity: vec![],
            identity_position: 0,
            rows: vec![],
        }
    }

    fn build_rows(&self) -> Vec<(String, String)> {
        let mut rows = vec![];
        if let Some(about) = &self.about {
            let d = &about.device_info;
            let mut groups = crate::smart::about_groups(about);
            groups.insert(
                0,
                (
                    i18n("System"),
                    vec![
                        (i18n("Host name"), d.hostname.clone()),
                        (i18n("Manufacturer"), d.vendor.clone()),
                        (i18n("Model"), d.model.clone()),
                        (i18n("Architecture"), about.os_info.os_architecture.clone()),
                    ],
                ),
            );
            groups.push((
                i18n("Motherboard"),
                vec![
                    (i18n("Manufacturer"), d.board_vendor.clone()),
                    (i18n("Model"), d.board_name.clone()),
                    (i18n("Version"), d.board_version.clone()),
                ],
            ));
            groups.push((
                i18n("BIOS"),
                vec![
                    (i18n("Manufacturer"), d.bios_vendor.clone()),
                    (i18n("Version"), d.bios_version.clone()),
                    (i18n("Date"), d.bios_date.clone()),
                ],
            ));
            for (title, group) in groups {
                let group: Vec<_> = group
                    .into_iter()
                    .filter_map(|(key, value)| Some((key, clean(&value?)?)))
                    .collect();
                if !group.is_empty() {
                    rows.push((title, String::new()));
                    rows.extend(group);
                }
            }
        }
        rows.extend_from_slice(&self.hardware[..self.identity_position]);
        rows.extend(self.identity.clone());
        rows.extend_from_slice(&self.hardware[self.identity_position..]);
        rows
    }
}

impl DevicePage for SystemPage {
    fn name(&self) -> &str {
        "system"
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::System
    }
    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }
    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }
    fn update_about(&mut self, about: &About) {
        self.summary.info1 = about
            .device_info
            .model
            .as_deref()
            .and_then(clean)
            .or_else(|| about.device_info.hostname.as_deref().and_then(clean))
            .unwrap_or_default();
        self.summary.info2 = about.os_info.name.clone().unwrap_or_default();
        self.about = Some(about.clone());
        self.rows = self.build_rows();
    }
    fn update(&mut self, readings: &Readings, _: &UpdateEnv) -> bool {
        let mut rows = vec![];
        let h = |bytes| to_human_readable_nice(bytes as f32, &DataType::MemoryBytes);
        rows.push((i18n("Processor"), String::new()));
        if let Some(name) = readings.cpu.name.as_deref().and_then(clean) {
            rows.push((i18n("Model"), name));
        }
        rows.push((
            i18n("Logical processors"),
            readings.cpu.core_usage_percent.len().to_string(),
        ));
        self.identity_position = rows.len();
        let mut identity = vec![];
        if let Some(data) = &readings.hardware {
            identity.push((i18n("System identity"), String::new()));
            for (label, value) in [
                (n_("System serial number"), &data.product_serial),
                (n_("Motherboard serial number"), &data.board_serial),
                (n_("System UUID"), &data.product_uuid),
            ] {
                if let Some(value) = value.as_deref().and_then(clean) {
                    identity.push((i18n(label), value));
                }
            }
        }
        rows.push((i18n("Memory"), String::new()));
        let installed = readings
            .mem_devices
            .iter()
            .try_fold(0_u64, |total, module| {
                total.checked_add(known_capacity(module).unwrap_or(0))
            });
        let complete = super::mem_slots::inventory_complete(
            &readings.mem_devices,
            readings.mem_info.max_devices,
        ) && readings
            .mem_devices
            .iter()
            .all(|module| known_capacity(module).is_some());
        let installed = installed.unwrap_or(0);
        rows.push(if installed > 0 {
            (
                i18n(if complete {
                    "Installed memory"
                } else {
                    "Known module capacity (partial)"
                }),
                h(installed),
            )
        } else {
            (i18n("Usable memory"), h(readings.mem_info.mem_total))
        });
        if installed > 0 && !complete {
            rows.push((i18n("Usable memory"), h(readings.mem_info.mem_total)));
        }
        for module in readings.mem_devices.iter().filter(|m| !slot_is_empty(m)) {
            let mut values = vec![capacity_label(module)];
            if !module.ram_type.is_empty() {
                values.push(module.ram_type.clone());
            }
            if module.speed > 0 {
                values.push(format!("{} MT/s", module.speed));
            }
            if !module.form_factor.is_empty() {
                values.push(module.form_factor.clone());
            }
            rows.push((
                format!("{} {}", module.locator, module.bank_locator)
                    .trim()
                    .to_owned(),
                values.join(" · "),
            ));
        }
        let mut gpus: Vec<_> = readings.gpus.values().collect();
        gpus.sort_by_key(|g| &g.id);
        if !gpus.is_empty() {
            rows.push((i18n("Graphics"), String::new()));
        }
        for gpu in gpus {
            rows.push((gpu.id.clone(), gpu.device_name.clone().unwrap_or_default()));
            if let Some(total) = gpu.total_memory.filter(|total| *total > 0) {
                rows.push((i18n("Dedicated memory"), h(total)));
            }
        }
        let mut disks: Vec<_> = readings
            .disks_info
            .iter()
            .filter(|d| d.capacity_bytes > 0)
            .collect();
        disks.sort_by_key(|d| &d.id);
        if !disks.is_empty() {
            rows.push((i18n("Storage"), String::new()));
        }
        for disk in disks {
            rows.push((
                disk.id.clone(),
                format!(
                    "{} · {}",
                    disk.model.as_deref().unwrap_or_default(),
                    h(disk.capacity_bytes)
                ),
            ));
        }
        if self.hardware != rows || self.identity != identity {
            self.hardware = rows;
            self.identity = identity;
            self.rows = self.build_rows();
        }
        true
    }
    fn set_grid_offset(&mut self, _: u32) -> bool {
        false
    }
    fn clear_privileged_hardware(&mut self) {
        if !self.identity.is_empty() {
            self.identity.clear();
            self.rows = self.build_rows();
        }
    }
    fn data_summary(&self) -> String {
        self.rows
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn win10_view(&mut self, _: &Config) -> PageView<'_> {
        PageView {
            model_tooltip: None,
            title: i18n("System"),
            model: self.summary.info1.clone(),
            panels: vec![],
            stats: vec![],
            info: self.rows.clone(),
            composition: None,
            swap_compression: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_magpie::MemoryDevice;

    #[test]
    fn installed_total_requires_every_slot_capacity_and_preserves_unknown_metadata() {
        let mut readings = Readings::new();
        readings.mem_info.max_devices = 2;
        readings.mem_info.mem_total = 7 * 1024 * 1024 * 1024;
        readings.mem_devices = vec![
            MemoryDevice {
                size: 8 * 1024 * 1024 * 1024,
                locator: "A1".into(),
                ..Default::default()
            },
            MemoryDevice {
                locator: "A2".into(),
                ram_type: "DDR4".into(),
                ..Default::default()
            },
        ];
        let mut page = SystemPage::new();
        page.update(&readings, &UpdateEnv::new(&Config::default()));
        assert!(
            page.rows
                .iter()
                .any(|(key, _)| key == "Known module capacity (partial)")
        );
        assert!(!page.rows.iter().any(|(key, _)| key == "Installed memory"));
        assert!(
            page.rows
                .iter()
                .any(|(key, value)| key == "A2" && value == "Unknown · DDR4")
        );
        assert!(!page.data_summary().contains("0 MT/s"));

        readings.mem_devices[1].present = Some(false);
        page.update(&readings, &UpdateEnv::new(&Config::default()));
        assert!(
            page.rows
                .iter()
                .any(|(key, value)| key == "Installed memory" && value == "8.00 GiB")
        );
        assert!(!page.rows.iter().any(|(key, _)| key == "A2"));

        readings.mem_info.max_devices = 1;
        page.update(&readings, &UpdateEnv::new(&Config::default()));
        assert!(!page.rows.iter().any(|(key, _)| key == "Installed memory"));
        readings.mem_info.max_devices = 3;
        page.update(&readings, &UpdateEnv::new(&Config::default()));
        assert!(!page.rows.iter().any(|(key, _)| key == "Installed memory"));
        readings.mem_info.max_devices = 0;
        page.update(&readings, &UpdateEnv::new(&Config::default()));
        assert!(!page.rows.iter().any(|(key, _)| key == "Installed memory"));
    }
}
