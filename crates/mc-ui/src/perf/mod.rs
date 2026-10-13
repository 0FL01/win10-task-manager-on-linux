/* crates/mc-ui/src/perf/mod.rs
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

//! The Performance page: device sidebar with summary graphs, the selected device page and its
//! details panel.

use std::collections::{HashMap, HashSet};

use egui::{Color32, Ui};
use mc_core::config::Config;
use mc_core::format::to_short_human_readable_time;
use mc_core::gpu::GpuSnapshot;
use mc_core::i18n::i18n;
use mc_core::sidebar::{
    self, DeviceOverride, DeviceType, parse_device_overrides, resolve_device_visibility,
    serialize_device_overrides,
};
use mc_graph::{DEFAULT_DATA_POINTS, DatasetGroup, Graph, ScalingSettings};
use mc_magpie::{INTERVAL_STEP, Readings};

use crate::cx::Cx;

pub mod battery;
pub mod cpu;
pub mod disk;
pub mod fan;
pub mod gpu;
pub mod mem_composition;
pub mod mem_slots;
pub mod memory;
pub mod network;
pub mod swap_compression;
pub mod system;

pub const MK_TO_0_C: i32 = -273150;

/// Values every device page needs while updating.
pub struct UpdateEnv<'a> {
    pub config: &'a Config,
    /// e.g. "60 seconds"
    pub max_duration: String,
}

impl UpdateEnv<'_> {
    pub fn new(config: &Config) -> UpdateEnv<'_> {
        UpdateEnv {
            config,
            max_duration: max_duration(config),
        }
    }
}

pub fn max_duration(config: &Config) -> String {
    let seconds = (config.app_update_interval as f64 * INTERVAL_STEP * DEFAULT_DATA_POINTS as f64)
        .round() as u32;
    to_short_human_readable_time(seconds)
}

/// Common interface of the CPU, memory, disk, network, GPU, fan and battery pages.
pub trait DevicePage {
    /// Sidebar/page identifier (`cpu`, `disk-sda`, `net-eth0`, ...)
    fn name(&self) -> &str;
    fn device_type(&self) -> DeviceType;
    fn summary(&self) -> &SummaryGraph;
    fn summary_mut(&mut self) -> &mut SummaryGraph;
    /// Feeds a new snapshot. Returns `false` if the device disappeared from the readings.
    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool;
    /// Normalized GPU telemetry, delivered before the ordinary device update.
    fn update_gpu(&mut self, _snapshot: &GpuSnapshot) {}
    fn update_about(&mut self, _about: &mc_magpie::About) {}
    fn update_spd(
        &mut self,
        _privileged: bool,
        _result: &Result<Vec<mc_magpie::SpdEeprom>, String>,
    ) {
    }
    fn clear_privileged_spd(&mut self) {}
    fn set_spd_pending(&mut self, _pending: bool) {}
    /// Metadata-only invalidation: never append another graph sample.
    fn clear_privileged_hardware(&mut self) {}
    fn extra_ui(&mut self, _ui: &mut Ui, _rect: egui::Rect, _module_height: f32, _cx: &mut Cx) {}
    /// Returns `true` if a repaint is needed for the animation.
    fn set_grid_offset(&mut self, offset: u32) -> bool;
    fn data_summary(&self) -> String;
    /// Content of the Win10-style page: title, graphs, statistic blocks and key/value rows.
    fn win10_view(&mut self, config: &Config) -> PageView<'_>;
    /// Page specific entries of the Win10 context menu.
    fn win10_menu(&mut self, _ui: &mut Ui, _cx: &mut Cx) {}
    /// Optional per-tile captions for the first graph panel. Other device layouts are unchanged.
    fn win10_graph_tiles(&mut self, _config: &Config) -> Vec<GraphTile> {
        vec![]
    }
    fn win10_graph_menu(&mut self, _ui: &mut Ui, _cx: &mut Cx, _slot: usize) {}
    /// Shown sidebar index for devices with siblings ("Disk 0", "GPU 1"...).
    fn set_index(&mut self, _index: Option<usize>) {}
}

/// A captioned block of graphs on a Win10 performance page.
pub struct GraphPanel<'a> {
    pub caption: String,
    pub right: String,
    pub graphs: Vec<&'a mut Graph>,
    pub columns: usize,
    pub tooltips: &'a [String],
}

/// GPU engine captions and selection targets, kept outside `PageView` so existing page literals
/// and the reference CPU geometry remain compatible.
#[derive(Clone)]
pub struct GraphTile {
    pub caption: String,
    pub right: String,
    pub tooltip: String,
    pub slot: usize,
}

impl<'a> GraphPanel<'a> {
    pub fn grid(
        caption: String,
        right: String,
        graphs: Vec<&'a mut Graph>,
        columns: usize,
    ) -> Self {
        Self {
            caption,
            right,
            graphs,
            columns,
            tooltips: &[],
        }
    }

    pub fn single(caption: String, right: String, graph: &'a mut Graph) -> Self {
        Self {
            caption,
            right,
            graphs: vec![graph],
            columns: 1,
            tooltips: &[],
        }
    }
}

/// Everything a Win10 performance page shows (docs/win10-spec/spec.md, "Performance tab").
pub struct PageView<'a> {
    pub title: String,
    pub model: String,
    pub panels: Vec<GraphPanel<'a>>,
    /// Rows of label/value statistic blocks
    pub stats: Vec<Vec<(String, String)>>,
    /// Key/value rows right of the statistics
    pub info: Vec<(String, String)>,
    pub composition: Option<&'a mem_composition::MemComposition>,
    pub swap_compression: Option<&'a swap_compression::SwapCompression>,
}

/// `"<label><duration>"`, e.g. "Utilization over 1 minute".
pub fn over(label: &str, duration: &str) -> String {
    format!("{}{duration}", i18n(label))
}

/// One row of the device sidebar.
pub struct SummaryGraph {
    pub graph: Graph,
    pub heading: String,
    pub info1: String,
    pub info2: String,
    pub opacity: f32,
}

impl SummaryGraph {
    pub fn new(color: Color32, heading: String) -> Self {
        let mut graph = Graph::new();
        graph.set_base_color(color);
        graph.set_grid_visible(false);
        graph.set_data_points(60);
        Self {
            graph,
            heading,
            info1: String::new(),
            info2: String::new(),
            opacity: 1.,
        }
    }

    /// Single dataset, fixed 0..100.
    pub fn percent(color: Color32, heading: String) -> Self {
        let mut s = Self::new(color, heading);
        s.graph.add_dataset(DatasetGroup::new());
        s
    }
}

/// Order in which device kinds are created (and their default sidebar order).
fn device_prefix_index(name: &str) -> u8 {
    sidebar::canonical_key(name).0
}

pub struct PerformancePage {
    pub(crate) devices: Vec<Box<dyn DevicePage>>,
    /// Display order (all present devices)
    order: Vec<String>,
    pub selected: String,
    pub edit_mode: bool,
    pub summary_mode: bool,
    pub(crate) loaded: bool,
    overrides_raw: String,
    overrides: HashMap<String, DeviceOverride>,
}

impl PerformancePage {
    pub fn new(config: &Config) -> Self {
        Self {
            devices: vec![],
            order: vec![],
            selected: config.performance_selected_page.clone(),
            edit_mode: false,
            summary_mode: false,
            loaded: false,
            overrides_raw: String::new(),
            overrides: HashMap::new(),
        }
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub(crate) fn device_index(&self, name: &str) -> Option<usize> {
        self.devices.iter().position(|d| d.name() == name)
    }

    fn create_missing_devices(&mut self, readings: &Readings, config: &Config) {
        let mut present: Vec<String> = vec!["cpu".into(), "memory".into(), "system".into()];
        present.extend(
            readings
                .disks_info
                .iter()
                .filter(|d| d.capacity_bytes > 0)
                .map(|d| disk::page_name(&d.id)),
        );
        present.extend(
            readings
                .network_connections
                .keys()
                .map(|id| network::page_name(id)),
        );
        present.extend(readings.gpus.keys().map(|id| gpu::page_name(id)));
        present.extend(readings.fans.iter().map(fan::page_name));
        present.extend(readings.batteries.iter().map(battery::page_name));

        let existing: HashSet<String> = self.devices.iter().map(|d| d.name().to_owned()).collect();

        for name in &present {
            if existing.contains(name) {
                continue;
            }
            let device: Option<Box<dyn DevicePage>> = match name.as_str() {
                "cpu" => Some(Box::new(cpu::CpuPage::new(readings, config))),
                "memory" => Some(Box::new(memory::MemoryPage::new(readings, config))),
                "system" => Some(Box::new(system::SystemPage::new())),
                n if n.starts_with("disk-") => readings
                    .disks_info
                    .iter()
                    .find(|d| disk::page_name(&d.id) == n)
                    .map(|d| Box::new(disk::DiskPage::new(d, config)) as Box<dyn DevicePage>),
                n if n.starts_with("net-") => readings
                    .network_connections
                    .values()
                    .find(|c| network::page_name(&c.id) == n)
                    .map(|c| Box::new(network::NetworkPage::new(c, config)) as Box<dyn DevicePage>),
                n if n.starts_with("gpu-") => readings
                    .gpus
                    .values()
                    .find(|g| gpu::page_name(&g.id) == n)
                    .map(|g| Box::new(gpu::GpuPage::new(g, config)) as Box<dyn DevicePage>),
                n if n.starts_with("fan-") => readings
                    .fans
                    .iter()
                    .find(|f| fan::page_name(f) == n)
                    .map(|f| Box::new(fan::FanPage::new(f, config)) as Box<dyn DevicePage>),
                n if n.starts_with("battery-") => readings
                    .batteries
                    .iter()
                    .find(|b| battery::page_name(b) == n)
                    .map(|b| Box::new(battery::BatteryPage::new(b, config)) as Box<dyn DevicePage>),
                _ => None,
            };
            if let Some(device) = device {
                self.devices.push(device);
            }
        }

        self.devices.sort_by(|a, b| {
            sidebar::canonical_key(a.name()).cmp(&sidebar::canonical_key(b.name()))
        });
    }

    fn assign_indices(&mut self) {
        let mut counts: HashMap<u8, usize> = HashMap::new();
        for d in &self.devices {
            *counts.entry(device_prefix_index(d.name())).or_default() += 1;
        }
        let mut seen: HashMap<u8, usize> = HashMap::new();
        for d in self.devices.iter_mut() {
            // GPU indices belong to the normalized snapshot, including GPU 0 for one adapter.
            if d.device_type() == DeviceType::Gpu {
                continue;
            }
            let kind = device_prefix_index(d.name());
            let i = seen.entry(kind).or_default();
            let index = (counts[&kind] > 1).then_some(*i);
            *i += 1;
            d.set_index(index);
        }
    }

    /// Applies a new snapshot. `config` may be updated (sidebar order, migrated keys).
    pub fn update(&mut self, readings: &Readings, gpu: &GpuSnapshot, config: &mut Config) {
        migrate_hidden_graphs(config);
        self.create_missing_devices(readings, config);

        let env_config = config.clone();
        let env = UpdateEnv::new(&env_config);

        let before = self.devices.len();
        self.devices.retain_mut(|d| {
            d.update_gpu(gpu);
            d.update(readings, &env)
        });
        if self.devices.len() != before || !self.loaded {
            self.assign_indices();
        }

        let present: Vec<String> = self.devices.iter().map(|d| d.name().to_owned()).collect();
        self.order = sidebar::rebuild_order(config, &present);

        let shown = self.shown_names(config);
        self.keep_selection_shown(&shown, config);
        self.loaded = true;
    }

    pub fn set_grid_offset(&mut self, offset: u32) -> bool {
        let mut repaint = false;
        for d in self.devices.iter_mut() {
            repaint |= d.set_grid_offset(offset);
            repaint |= d.summary_mut().graph.set_grid_offset(offset);
        }
        repaint
    }

    pub fn update_about(&mut self, about: &mc_magpie::About) {
        for device in &mut self.devices {
            device.update_about(about);
        }
    }

    pub fn update_spd(
        &mut self,
        privileged: bool,
        result: &Result<Vec<mc_magpie::SpdEeprom>, String>,
    ) {
        for device in &mut self.devices {
            device.update_spd(privileged, result);
        }
    }
    pub fn clear_privileged_spd(&mut self) {
        for device in &mut self.devices {
            device.clear_privileged_spd();
        }
    }
    pub fn set_spd_pending(&mut self, pending: bool) {
        for device in &mut self.devices {
            device.set_spd_pending(pending);
        }
    }
    pub fn clear_privileged_hardware(&mut self) {
        for device in &mut self.devices {
            device.clear_privileged_hardware();
            device.clear_privileged_spd();
        }
    }

    fn refresh_overrides(&mut self, config: &Config) {
        if self.overrides_raw != config.performance_sidebar_device_overrides {
            self.overrides_raw = config.performance_sidebar_device_overrides.clone();
            self.overrides = parse_device_overrides(&self.overrides_raw);
        }
    }

    pub fn device_visible(&self, device: &dyn DevicePage, config: &Config) -> bool {
        let category = device.device_type().category_visible(config);
        resolve_device_visibility(device.name(), &self.overrides, category)
    }

    pub(crate) fn shown_names(&mut self, config: &Config) -> Vec<String> {
        self.refresh_overrides(config);
        self.order
            .iter()
            .filter(|name| {
                self.edit_mode
                    || self
                        .devices
                        .iter()
                        .find(|d| d.name() == name.as_str())
                        .is_some_and(|d| self.device_visible(d.as_ref(), config))
            })
            .cloned()
            .collect()
    }

    pub fn select(&mut self, name: &str, config: &mut Config) {
        if self.device_index(name).is_some() {
            self.selected = name.to_owned();
            config.performance_selected_page = name.to_owned();
        }
    }

    /// Moves the selection to the first shown device when the selected one is gone or hidden.
    pub(crate) fn keep_selection_shown(&mut self, shown: &[String], config: &mut Config) {
        if !shown.contains(&self.selected)
            && let Some(first) = shown.first()
        {
            self.select(first, config);
        }
    }

    pub fn select_category(&mut self, action: &str, config: &mut Config) {
        let shown = self.shown_names(config);
        if let Some(name) = shown
            .iter()
            .find(|n| sidebar::view_action_name(n) == Some(action))
            .cloned()
        {
            self.select(&name, config);
        }
    }

    pub fn select_nth_shown(&mut self, nth: usize, config: &mut Config) {
        let shown = self.shown_names(config);
        if let Some(name) = shown.get(nth).cloned() {
            self.select(&name, config);
        }
    }

    pub fn data_summary(&self) -> Option<String> {
        let index = self.device_index(&self.selected)?;
        Some(self.devices[index].data_summary())
    }

    // Edit mode actions
}

/// Moves names from the deprecated `performance-sidebar-hidden-graphs` key into the overrides.
fn migrate_hidden_graphs(config: &mut Config) {
    if config.performance_sidebar_hidden_graphs.is_empty() {
        return;
    }
    let mut overrides = parse_device_overrides(&config.performance_sidebar_device_overrides);
    for name in config
        .performance_sidebar_hidden_graphs
        .split(';')
        .filter(|s| !s.is_empty())
    {
        overrides
            .entry(sidebar::decode_device_name(name))
            .or_insert(DeviceOverride::Hide);
    }
    config.performance_sidebar_device_overrides = serialize_device_overrides(&overrides);
    config.performance_sidebar_hidden_graphs.clear();
}

/// Default dataset group with a fixed upper bound.
pub fn fixed_group(max: f32) -> DatasetGroup {
    let mut g = DatasetGroup::new();
    g.dataset_settings.scaling_settings = ScalingSettings::Fixed;
    g.dataset_settings.high_watermark = max;
    g
}

#[cfg(test)]
mod tests {
    use mc_magpie::magpie_types::network::ConnectionKind;

    use super::*;

    fn with_veth() -> Readings {
        let mut r = mc_magpie::synthetic_readings(0, 8);
        let mut veth = r.network_connections["wlan0"].clone();
        veth.id = "veth0".into();
        veth.kind = ConnectionKind::Virtual as i32;
        veth.wireless_connection = None;
        r.network_connections.insert("veth0".into(), veth);
        r
    }

    #[test]
    fn hidden_virtual_adapter_loses_selection() {
        let veth = network::page_name("veth0");
        let mut config = Config {
            performance_selected_page: veth.clone(),
            performance_show_network_virtual: true,
            ..Config::default()
        };
        let mut page = PerformancePage::new(&config);
        page.update(&with_veth(), &GpuSnapshot::default(), &mut config);
        assert_eq!(page.selected, veth, "shown while the group is enabled");

        config.performance_show_network_virtual = false;
        page.update(&with_veth(), &GpuSnapshot::default(), &mut config);
        assert!(!page.shown_names(&config).contains(&veth));
        assert_ne!(page.selected, veth);
        assert_eq!(config.performance_selected_page, page.selected);
    }
}
