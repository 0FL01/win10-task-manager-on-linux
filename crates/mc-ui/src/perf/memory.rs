/* crates/mc-ui/src/perf/memory.rs
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
use mc_graph::{DatasetGroup, FillingSettings, Graph};
use mc_magpie::Readings;

use super::mem_composition::MemComposition;
use super::{DevicePage, SummaryGraph, UpdateEnv};
use crate::cx::Cx;
const COLOR: egui::Color32 = crate::win10::palette::MEMORY.line;
const DISK: egui::Color32 = crate::win10::palette::DISK.line;

const MIN_COMPRESSION_THRESHOLD: u64 = 1024 * 1024;

#[derive(Default)]
struct Labels {
    in_use: String,
    available: String,
    committed: String,
    cached: String,
    swap_available: Option<String>,
    swap_used: Option<String>,
    compressed: Option<String>,
    savings: Option<String>,
    tt_in_use: String,
    tt_modified: String,
    tt_standby: String,
    tt_free: String,
}

pub struct MemoryPage {
    summary: SummaryGraph,
    usage: Graph,
    swap: Graph,
    composition: MemComposition,
    total_ram: String,
    max_graph_ram: String,
    total_swap: String,
    has_swap: bool,
    speed: String,
    slots_used: String,
    form_factor: String,
    ram_type: String,
    devices_failed: bool,
    labels: Labels,
    max_duration: String,
}

impl MemoryPage {
    pub fn new(readings: &Readings, config: &Config) -> Self {
        let mem = &readings.mem_info;
        let h = |v: u64| to_human_readable_nice(v as f32, &DataType::MemoryBytes);

        let mut summary = SummaryGraph::new(COLOR, i18n("Memory"));
        let mut committed = DatasetGroup::new();
        committed.dataset_settings.fill = FillingSettings::None;
        committed.dataset_settings.dashed = true;
        committed.dataset_settings.high_watermark = mem.mem_total as f32;
        let mut used = DatasetGroup::new();
        used.dataset_settings.high_watermark = mem.mem_total as f32;
        summary.graph.add_dataset(committed);
        summary.graph.add_dataset(used);
        summary.info1 = "0/0 GiB".into();
        summary.info2 = "0%".into();

        let mut usage = Graph::new();
        usage.set_base_color(COLOR);
        let mut a = DatasetGroup::new();
        a.dataset_settings.fill = FillingSettings::None;
        a.dataset_settings.dashed = true;
        let mut b = DatasetGroup::new();
        b.dataset_settings.fill = FillingSettings::None;
        usage.add_dataset(a);
        usage.add_dataset(b);
        usage.add_dataset(DatasetGroup::new());
        usage.connect_datasets(0, 1);
        usage.connect_datasets(1, 2);
        usage.connect_datasets(2, 0);
        usage.set_all_datasets_max_scale(mem.mem_total as f32);

        let mut swap = Graph::new();
        swap.set_base_color(DISK);
        swap.add_dataset(DatasetGroup::new());
        swap.set_all_datasets_max_scale(mem.swap_total as f32);

        let devices = &readings.mem_devices;
        let unknown = i18n("Unknown");
        let (speed, slots_used, form_factor, ram_type) = if let Some(first) = devices.first() {
            let speed = if first.speed != 0 {
                format!("{} MT/s", first.speed)
            } else {
                unknown.clone()
            };
            let module_count = devices.len();
            let slots = if mem.max_devices > 0 {
                i18n_f(
                    n_("{} of {}"),
                    &[&module_count.to_string(), &mem.max_devices.to_string()],
                )
            } else {
                module_count.to_string()
            };
            let ff = if first.form_factor.is_empty() {
                unknown.clone()
            } else {
                first.form_factor.clone()
            };
            let rt = if first.ram_type.is_empty() {
                unknown.clone()
            } else {
                first.ram_type.clone()
            };
            (speed, slots, ff, rt)
        } else {
            (unknown.clone(), unknown.clone(), unknown.clone(), unknown)
        };

        Self {
            summary,
            usage,
            swap,
            composition: MemComposition::default(),
            total_ram: h(mem.mem_total),
            max_graph_ram: h(mem.mem_total),
            total_swap: h(mem.swap_total),
            has_swap: mem.swap_total > 0,
            speed,
            slots_used,
            form_factor,
            ram_type,
            devices_failed: devices.is_empty(),
            labels: Labels::default(),
            max_duration: super::max_duration(config),
        }
    }
}

impl DevicePage for MemoryPage {
    fn name(&self) -> &str {
        "memory"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Memory
    }

    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }

    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let mem = &readings.mem_info;
        self.max_duration = env.max_duration.clone();
        let h = |v: u64| to_human_readable_nice(v as f32, &DataType::MemoryBytes);

        let mem_avail = if mem.mem_available > mem.mem_total {
            mem.mem_free
        } else {
            mem.mem_available
        };
        let used = mem.mem_total.saturating_sub(mem_avail);
        let standby = mem.mem_total.saturating_sub(used + mem.mem_free);

        self.usage.set_all_datasets_max_scale(mem.mem_total as f32);
        self.usage.add_data_point(vec![
            vec![mem.committed as f32],
            vec![mem.dirty as f32],
            vec![used as f32],
        ]);
        self.total_ram = h(mem.mem_total);
        self.max_graph_ram =
            to_human_readable_nice(self.usage.dataset_max_scale(0), &DataType::MemoryBytes);

        let swap_used = mem.swap_total.saturating_sub(mem.swap_free);
        self.swap.set_all_datasets_max_scale(mem.swap_total as f32);
        self.swap.add_data_point(vec![vec![swap_used as f32]]);
        self.composition.update(mem);

        let l = &mut self.labels;
        l.in_use = h(used);
        l.available = h(mem.mem_available);
        l.committed = h(mem.committed);
        l.cached = h(mem.cached);
        self.has_swap = mem.swap_total != 0;
        if self.has_swap {
            l.swap_available = Some(h(mem.swap_total));
            l.swap_used = Some(h(swap_used));
        } else {
            l.swap_available = None;
            l.swap_used = None;
        }
        let total_original = mem.zswapped + mem.zram_orig_size;
        let total_compressed = mem.zswap + mem.zram_compr_size;
        if total_original >= MIN_COMPRESSION_THRESHOLD {
            l.compressed = Some(h(total_compressed));
            l.savings = Some(h(total_original.saturating_sub(total_compressed)));
        } else {
            l.compressed = None;
            l.savings = None;
        }
        l.tt_in_use = h(used);
        l.tt_modified = h(mem.dirty);
        l.tt_standby = h(standby);
        l.tt_free = h(mem.mem_free);
        self.total_swap = h(mem.swap_total);

        // Sidebar
        let total = mem.mem_total.max(1);
        self.summary
            .graph
            .set_all_datasets_max_scale(mem.mem_total as f32);
        self.summary
            .graph
            .add_data_point(vec![vec![mem.committed as f32], vec![used as f32]]);
        // Win10 list: "used/total (percent)" on one line
        self.summary.info1 = format!(
            "{}/{} ({}%)",
            h(used),
            h(mem.mem_total),
            (used as f32 / total as f32 * 100.).round()
        );
        self.summary.info2.clear();

        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        self.usage.set_grid_offset(offset) | self.swap.set_grid_offset(offset)
    }

    fn win10_view(&mut self, config: &Config) -> PageView<'_> {
        let l = &self.labels;
        let mut stats = vec![
            vec![
                (i18n("In use"), l.in_use.clone()),
                (i18n("Available"), l.available.clone()),
            ],
            vec![
                (i18n("Committed"), l.committed.clone()),
                (i18n("Cached"), l.cached.clone()),
            ],
        ];
        if let (Some(used), Some(avail)) = (&l.swap_used, &l.swap_available) {
            stats.push(vec![
                (i18n("Swap used"), used.clone()),
                (i18n("Swap available"), avail.clone()),
            ]);
        }
        if let (Some(c), Some(s)) = (&l.compressed, &l.savings) {
            stats.push(vec![
                (i18n("Compressed"), c.clone()),
                (i18n("Savings"), s.clone()),
            ]);
        }
        let mut info = vec![
            (i18n("Speed:"), self.speed.clone()),
            (i18n("Slots used:"), self.slots_used.clone()),
            (i18n("Form factor:"), self.form_factor.clone()),
            (i18n("Type:"), self.ram_type.clone()),
        ];
        if self.devices_failed {
            info.push((
                i18n("Getting additional memory information failed"),
                String::new(),
            ));
        }
        let show_swap = config.performance_page_memory_swap_visible && self.has_swap;
        let mut panels = vec![GraphPanel::single(
            over(n_("Memory usage over "), &self.max_duration),
            self.max_graph_ram.clone(),
            &mut self.usage,
        )];
        if show_swap {
            panels.push(GraphPanel::single(
                over(n_("Swap usage over "), &self.max_duration),
                self.total_swap.clone(),
                &mut self.swap,
            ));
        }
        PageView {
            title: i18n("Memory"),
            model: self.total_ram.clone(),
            panels,
            stats,
            info,
        }
    }

    fn win10_menu(&mut self, ui: &mut Ui, cx: &mut Cx) {
        use crate::win10::menu;
        if self.has_swap
            && menu::check(
                ui,
                &i18n("Show Swap Usage"),
                cx.config.performance_page_memory_swap_visible,
            )
        {
            cx.config.performance_page_memory_swap_visible =
                !cx.config.performance_page_memory_swap_visible;
        }
    }

    fn data_summary(&self) -> String {
        let l = &self.labels;
        let na = i18n("N/A");
        let o = |v: &Option<String>| v.clone().unwrap_or_else(|| na.clone());
        format!(
            r#"Memory

    {}

    In use:         {}
    Available:      {}
    Committed:      {}
    Cached:         {}
    Swap available: {}
    Swap used:      {}
    Compressed:     {}
    Savings:        {}

    Speed:       {}
    Slots used:  {}
    Form factor: {}
    Type:        {}"#,
            self.total_ram,
            l.in_use,
            l.available,
            l.committed,
            l.cached,
            l.swap_available.clone().unwrap_or_default(),
            l.swap_used.clone().unwrap_or_default(),
            o(&l.compressed),
            o(&l.savings),
            self.speed,
            self.slots_used,
            self.form_factor,
            self.ram_type,
        )
    }
}
