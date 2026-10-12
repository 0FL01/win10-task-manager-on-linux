/* crates/mc-ui/src/perf/disk.rs
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

use super::{GraphPanel, PageView, over};
use egui::Ui;
use mc_core::config::Config;
use mc_core::format::{DataType, to_human_readable_nice};
use mc_core::i18n::n_;
use mc_core::i18n::{i18n, i18n_f};
use mc_core::sidebar::DeviceType;
use mc_graph::{DatasetGroup, FillingSettings, Graph, RoundingSettings, ScalingSettings};
use mc_magpie::magpie_types::disks::PartitionInfo;
use mc_magpie::{Disk, DiskKind, Readings, Request};

use super::{DevicePage, MK_TO_0_C, SummaryGraph, UpdateEnv};
use crate::cx::{Command, Cx, Dialog};
const COLOR: egui::Color32 = crate::win10::palette::DISK.line;

pub fn page_name(disk_id: &str) -> String {
    format!("disk-{disk_id}")
}

pub fn kind_name(kind: Option<DiskKind>) -> Option<String> {
    Some(i18n(match kind? {
        DiskKind::Hdd => n_("HDD"),
        DiskKind::Ssd => n_("SSD"),
        DiskKind::NvMe => n_("NVMe"),
        DiskKind::EMmc => n_("eMMC"),
        DiskKind::Sd => n_("SD"),
        DiskKind::IScsi => n_("iSCSI"),
        DiskKind::Optical => n_("Optical"),
        DiskKind::Floppy => n_("Floppy"),
        DiskKind::ThumbDrive => n_("Thumb Drive"),
    }))
}

fn disk_kind(disk: &Disk) -> Option<DiskKind> {
    disk.kind.and_then(|k| DiskKind::try_from(k).ok())
}

#[derive(Default)]
struct Labels {
    read_speed: String,
    write_speed: String,
    total_read: String,
    total_write: String,
    active_time: String,
    avg_response_time: String,
    formatted: String,
    rotation: Option<String>,
    max_y: String,
}

struct Partition {
    devname: String,
    filesystem: String,
    mountpoints: Vec<String>,
    used: Option<String>,
    total: Option<String>,
    fraction: f32,
    size: u64,
}

pub struct DiskPage {
    name: String,
    disk_id: String,
    index: Option<usize>,
    summary: SummaryGraph,
    usage: Graph,
    transfer: Graph,
    title: String,
    model: String,
    capacity: String,
    system_disk: String,
    disk_type: String,
    wwn: Option<String>,
    serial: Option<String>,
    has_smart: bool,
    ejectable: bool,
    partitions: Vec<Partition>,
    labels: Labels,
    max_duration: String,
    graph_tooltips: [String; 2],
}

impl DiskPage {
    pub fn new(disk: &Disk, config: &Config) -> Self {
        let mut summary = SummaryGraph::percent(COLOR, String::new());
        summary.info1 = disk.model.clone().unwrap_or_default();

        let mut usage = Graph::new();
        usage.set_base_color(COLOR);
        usage.add_dataset(DatasetGroup::new());

        let mut transfer = Graph::new();
        transfer.set_base_color(COLOR);
        let mut rx = DatasetGroup::new();
        rx.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
        rx.dataset_settings.rounding_settings = RoundingSettings::Pow2;
        let mut tx = DatasetGroup::new();
        tx.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
        tx.dataset_settings.rounding_settings = RoundingSettings::Pow2;
        tx.dataset_settings.fill = FillingSettings::None;
        tx.dataset_settings.dashed = true;
        transfer.add_dataset(rx);
        transfer.add_dataset(tx);
        transfer.connect_datasets(0, 1);

        let trimmed = |v: &Option<String>| {
            v.as_ref()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };

        let mut page = Self {
            name: page_name(&disk.id),
            disk_id: disk.id.clone(),
            index: None,
            summary,
            usage,
            transfer,
            title: String::new(),
            model: disk.model.clone().unwrap_or_else(|| i18n("Unknown")),
            capacity: if disk.capacity_bytes > 0 {
                to_human_readable_nice(disk.capacity_bytes as f32, &DataType::MemoryBytes)
            } else {
                i18n("Unknown")
            },
            system_disk: if disk.is_system {
                i18n("Yes")
            } else {
                i18n("No")
            },
            disk_type: kind_name(disk_kind(disk)).unwrap_or_else(|| i18n("Unknown")),
            wwn: trimmed(&disk.world_wide_name),
            serial: trimmed(&disk.serial_number),
            has_smart: disk.smart_interface.is_some(),
            ejectable: disk.ejectable,
            partitions: vec![],
            labels: Labels::default(),
            max_duration: super::max_duration(config),
            graph_tooltips: [
                i18n("Percentage of time the drive is processing read or write requests"),
                i18n("Read and write activity on this drive"),
            ],
        };
        // Throughput in binary bytes (MiB/s)
        page.transfer
            .set_all_datasets_rounding(RoundingSettings::Pow2);
        page.transfer.reset_auto_scaling();
        page.update_titles(disk);
        page
    }

    fn update_titles(&mut self, disk: &Disk) {
        self.title = match self.index {
            Some(i) => i18n_f("Drive {} ({})", &[&i.to_string(), &disk.id]),
            None => i18n_f("Drive ({})", &[&disk.id]),
        };
        let kind = kind_name(disk_kind(disk)).unwrap_or_else(|| i18n("Drive"));
        self.summary.heading = match self.index {
            Some(i) => i18n_f("{} {} ({})", &[&kind, &i.to_string(), &disk.id]),
            None => kind,
        };
    }

    fn update_partitions(&mut self, map: &std::collections::HashMap<String, PartitionInfo>) {
        let mut parts: Vec<Partition> = map
            .values()
            .map(|p| {
                let size = p.size.unwrap_or(0);
                let fraction = match (p.used, p.size) {
                    (Some(u), Some(s)) if s > 0 => (u as f32 / s as f32).clamp(0., 1.),
                    _ => 0.,
                };
                Partition {
                    devname: p.devname.clone(),
                    filesystem: p.filesystem.clone().unwrap_or_else(|| i18n("Unknown")),
                    mountpoints: p.mountpoints.clone(),
                    used: p
                        .used
                        .map(|u| to_human_readable_nice(u as f32, &DataType::DriveBytes)),
                    total: p
                        .size
                        .map(|s| to_human_readable_nice(s as f32, &DataType::DriveBytes)),
                    fraction,
                    size,
                }
            })
            .collect();
        parts.sort_by(|a, b| b.size.cmp(&a.size).then(a.devname.cmp(&b.devname)));
        self.partitions = parts;
    }
}

impl DevicePage for DiskPage {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Disk
    }

    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }

    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }

    fn set_index(&mut self, index: Option<usize>) {
        self.index = index;
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let Some(disk) = readings
            .disks_info
            .iter()
            .find(|d| d.id == self.disk_id && d.capacity_bytes > 0)
        else {
            return false;
        };
        self.max_duration = env.max_duration.clone();
        let h = |v: f32, t: DataType| to_human_readable_nice(v, &t);

        self.update_titles(disk);
        self.usage.add_data_point(vec![vec![disk.busy_percent]]);
        self.transfer.add_data_point(vec![
            vec![disk.rx_speed_bytes_ps as f32],
            vec![disk.tx_speed_bytes_ps as f32],
        ]);

        let l = &mut self.labels;
        l.max_y = h(
            self.transfer.dataset_max_scale(0),
            DataType::DriveBytesPerSecond,
        );
        l.formatted = disk
            .formatted_bytes
            .map(|b| h(b as f32, DataType::MemoryBytes))
            .unwrap_or_else(|| i18n("Unknown"));
        l.active_time = format!("{}%", disk.busy_percent.round() as u8);
        l.rotation = disk
            .rotation_rate
            .map(|r| i18n_f("{} RPM", &[&r.to_string()]));
        l.avg_response_time = format!("{:.2} ms", disk.response_time_ms);
        l.read_speed = h(disk.rx_speed_bytes_ps as f32, DataType::DriveBytesPerSecond);
        l.write_speed = h(disk.tx_speed_bytes_ps as f32, DataType::DriveBytesPerSecond);
        l.total_read = h(disk.rx_bytes_total as f32, DataType::DriveBytes);
        l.total_write = h(disk.tx_bytes_total as f32, DataType::DriveBytes);
        self.ejectable = disk.ejectable;
        self.update_partitions(&disk.partitions);

        self.summary
            .graph
            .add_data_point(vec![vec![disk.busy_percent]]);
        self.summary.info2 = match disk.temperature_milli_k {
            Some(mk) => format!(
                "{:.0}% ({:.0} °C)",
                disk.busy_percent,
                (mk as i32 + MK_TO_0_C) as f64 / 1000.
            ),
            None => format!("{:.0}%", disk.busy_percent),
        };
        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        self.usage.set_grid_offset(offset) | self.transfer.set_grid_offset(offset)
    }

    fn win10_view(&mut self, _config: &Config) -> PageView<'_> {
        let l = &self.labels;
        let stats = vec![
            vec![
                (i18n("Active time"), l.active_time.clone()),
                (i18n("Avg. response time"), l.avg_response_time.clone()),
            ],
            vec![
                (i18n("Read speed"), l.read_speed.clone()),
                (i18n("Write speed"), l.write_speed.clone()),
            ],
            vec![
                (i18n("Total Read"), l.total_read.clone()),
                (i18n("Total Written"), l.total_write.clone()),
            ],
        ];
        let mut info = vec![
            (i18n("Capacity:"), self.capacity.clone()),
            (i18n("Formatted:"), l.formatted.clone()),
            (i18n("System disk:"), self.system_disk.clone()),
            (i18n("Type:"), self.disk_type.clone()),
        ];
        if let Some(wwn) = &self.wwn {
            info.push((i18n("WWN:"), wwn.clone()));
        }
        if let Some(serial) = &self.serial {
            info.push((i18n("Serial:"), serial.clone()));
        }
        if let Some(rotation) = &l.rotation {
            info.push((i18n("Rotation Rate"), rotation.clone()));
        }
        for p in &self.partitions {
            let mut value = format!("{} {:.0}%", p.filesystem, p.fraction * 100.);
            if let (Some(used), Some(total)) = (&p.used, &p.total) {
                value.push_str(&format!(" ({used} / {total})"));
            }
            if let Some(mount) = p.mountpoints.first() {
                value.push_str(&format!(" {mount}"));
            }
            info.push((format!("{}:", p.devname), value));
        }
        let max_y = l.max_y.clone();
        let mut panels = vec![
            GraphPanel::single(
                over(n_("Active time over "), &self.max_duration),
                i18n("100%"),
                &mut self.usage,
            ),
            GraphPanel::single(
                over(n_("Throughput over "), &self.max_duration),
                max_y,
                &mut self.transfer,
            ),
        ];
        panels[0].tooltips = &self.graph_tooltips[..1];
        panels[1].tooltips = &self.graph_tooltips[1..];
        PageView {
            title: self.title.clone(),
            model: self.model.clone(),
            panels,
            stats,
            info,
            composition: None,
            swap_compression: None,
        }
    }

    fn win10_menu(&mut self, ui: &mut Ui, cx: &mut Cx) {
        use crate::win10::menu;
        if self.ejectable && menu::item(ui, &i18n("Eject Disk"), "", true) {
            cx.send(Command::Magpie(Request::EjectDisk(self.disk_id.clone())));
        }
        if self.has_smart && menu::item(ui, &i18n("Show SMART Data"), "", true) {
            cx.send(Command::OpenDialog(Dialog::SmartData(self.disk_id.clone())));
        }
    }

    fn data_summary(&self) -> String {
        let l = &self.labels;
        format!(
            r#"{}

    {}

    Capacity:    {}
    Formatted:   {}
    System disk: {}
    Type:        {}

    Read speed:            {}
    Total read:            {}
    Write speed:           {}
    Total written          {}
    Active time:           {}
    Average response time: {}"#,
            self.title,
            self.model,
            self.capacity,
            l.formatted,
            self.system_disk,
            self.disk_type,
            l.read_speed,
            l.total_read,
            l.write_speed,
            l.total_write,
            l.active_time,
            l.avg_response_time,
        )
    }
}
