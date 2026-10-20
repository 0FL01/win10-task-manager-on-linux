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
use super::swap_compression::{Layer, SwapCompression};
use super::{DevicePage, SummaryGraph, UpdateEnv};
use crate::cx::Cx;
const COLOR: egui::Color32 = crate::win10::palette::MEMORY.line;

#[derive(Default)]
struct Labels {
    in_use: String,
    available: String,
    free: String,
    installed: Option<String>,
    reserved: Option<String>,
    capacity_tooltip: String,
    capacity_heading: String,
    committed: String,
    cached: String,
    swap_total: Option<String>,
    swap_used: Option<String>,
    swap_free: Option<String>,
    usage_tooltip: String,
    hardware: Vec<(String, String)>,
    unprivileged_hardware: Vec<(String, String)>,
}

pub struct MemoryPage {
    summary: SummaryGraph,
    usage: Graph,
    swap: Graph,
    composition: MemComposition,
    swap_compression: SwapCompression,
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
    slots: super::mem_slots::MemorySlots,
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
        summary.graph.set_data_visible(0, false);
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
        // One RAM occupation curve; commit is virtual accounting, not extra physical RAM.
        usage.set_data_visible(0, false);
        usage.set_data_visible(1, false);
        usage.connect_datasets(0, 1);
        usage.connect_datasets(1, 2);
        usage.connect_datasets(2, 0);
        usage.set_all_datasets_max_scale(mem.mem_total as f32);

        let mut swap = Graph::new();
        swap.set_base_color(COLOR);
        swap.add_dataset(DatasetGroup::new());
        swap.set_all_datasets_max_scale(mem.swap_total as f32);

        let occupied: Vec<_> = readings
            .mem_devices
            .iter()
            .filter(|d| {
                !super::mem_slots::slot_is_empty(d) && (d.size > 0 || d.present == Some(true))
            })
            .collect();
        let devices = &occupied;
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
            swap_compression: SwapCompression::default(),
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
            slots: super::mem_slots::MemorySlots::new(readings),
        }
    }
}

fn memory_divider(uclk: f32, mclk: f32) -> Option<u32> {
    if !uclk.is_finite() || !mclk.is_finite() || uclk <= 0. || mclk <= 0. {
        return None;
    }
    let ratio = mclk / uclk;
    (1..=3).find(|divider| (ratio - *divider as f32).abs() < 0.03)
}

/// Use reported technology/package only: never infer LPDDR's X suffix.
fn memory_kind_label(modules: &[mc_magpie::MemoryDevice]) -> String {
    let mut kinds = std::collections::BTreeSet::new();
    for module in modules
        .iter()
        .filter(|m| !super::mem_slots::slot_is_empty(m) && (m.size > 0 || m.present == Some(true)))
    {
        let technology = module.ram_type.trim().to_ascii_uppercase();
        let technology = match technology.as_str() {
            "" | "UNKNOWN" | "OTHER" | "NONE" | "NOT SPECIFIED" => "",
            value => value,
        };
        let package = module.form_factor.trim().to_ascii_uppercase();
        let package = match package.as_str() {
            "SODIMM" | "SO-DIMM" | "SO DIMM" => "SO-DIMM",
            "DIMM" => "DIMM",
            _ => "",
        };
        let label = match (package, technology) {
            ("", "") => i18n("Unknown"),
            (_, kind) if kind.starts_with("LPDDR") => kind.to_owned(),
            ("", kind) => kind.to_owned(),
            (package, "") => package.to_owned(),
            (package, kind) => format!("{package} {kind}"),
        };
        kinds.insert(label);
    }
    kinds.into_iter().collect::<Vec<_>>().join(" / ")
}

fn module_model_label(modules: &[mc_magpie::MemoryDevice], max_devices: u64) -> String {
    if !super::mem_slots::inventory_complete(modules, max_devices) {
        return i18n("Unknown");
    }
    let known = |value: &str| {
        let value = value.trim();
        !value.is_empty()
            && ![
                "unknown",
                "none",
                "not specified",
                "default string",
                "to be filled by o.e.m.",
            ]
            .iter()
            .any(|placeholder| value.eq_ignore_ascii_case(placeholder))
    };
    let occupied: Vec<_> = modules
        .iter()
        .filter(|m| !super::mem_slots::slot_is_empty(m))
        .collect();
    if occupied.is_empty()
        || occupied.iter().any(|m| {
            (m.size == 0 && m.present != Some(true))
                || m.part_number.as_deref().is_none_or(|s| !known(s))
        })
    {
        return i18n("Unknown");
    }
    let models: std::collections::BTreeSet<_> = occupied
        .iter()
        .filter_map(|m| m.part_number.as_deref().map(str::trim).filter(|s| known(s)))
        .collect();
    if models.len() > 1 {
        return i18n("Mixed modules");
    }
    let manufacturer = occupied[0]
        .manufacturer
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let consistent = occupied
        .iter()
        .all(|m| m.manufacturer.as_deref().map(str::trim).unwrap_or_default() == manufacturer);
    let model = models.first().copied().unwrap_or_default();
    if consistent && known(manufacturer) {
        format!("{manufacturer} {model}")
    } else {
        model.to_owned()
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
        self.slots.update(readings);
        let mem = &readings.mem_info;
        self.max_duration = env.max_duration.clone();
        let h = |v: u64| to_human_readable_nice(v as f32, &DataType::MemoryBytes);

        let accounting = mc_core::memory::MemoryUsage::from_memory(mem);
        let used = accounting.occupied;

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
        self.swap_compression.update(mem);

        let l = &mut self.labels;
        l.hardware.clear();
        l.unprivileged_hardware.clear();
        for (label, clock) in [
            (n_("FCLK:"), mem.fclk_mhz),
            (n_("MCLK:"), mem.mclk_mhz),
            (n_("UCLK:"), mem.uclk_mhz),
        ] {
            if let Some(clock) = clock {
                l.unprivileged_hardware
                    .push((i18n(label), format!("{clock:.0} MHz")));
            }
        }
        if let (Some(uclk), Some(mclk)) = (mem.uclk_mhz, mem.mclk_mhz)
            && let Some(divider) = memory_divider(uclk, mclk)
        {
            l.unprivileged_hardware
                .push((i18n("UCLK:MCLK:"), format!("1:{divider}")));
        }
        if let Some(voltage) = mem.voltage_v {
            l.unprivileged_hardware
                .push((i18n("RAM voltage:"), format!("{voltage:.3} V")));
        }
        let clocks = readings
            .hardware
            .as_ref()
            .filter(|data| data.clock_source.is_some());
        let (fclk, mclk, uclk) = if let Some(data) = clocks {
            // Keep a register/table sample coherent instead of mixing independently sampled drivers.
            (data.fclk_mhz, data.mclk_mhz, data.uclk_mhz)
        } else {
            (mem.fclk_mhz, mem.mclk_mhz, mem.uclk_mhz)
        };
        for (label, clock) in [
            (n_("FCLK:"), fclk),
            (n_("MCLK:"), mclk),
            (n_("UCLK:"), uclk),
        ] {
            if let Some(clock) = clock {
                l.hardware.push((i18n(label), format!("{clock:.0} MHz")));
            }
        }
        if let (Some(uclk), Some(mclk)) = (uclk, mclk)
            && let Some(divider) = memory_divider(uclk, mclk)
        {
            l.hardware
                .push((i18n("UCLK:MCLK:"), format!("1:{divider}")));
        }
        if let Some(data) = &readings.hardware {
            if let Some(gear) = data.intel_gear {
                l.hardware.push((i18n("Intel Gear:"), gear.to_string()));
            }
            for (label, value) in [
                (n_("Clock source:"), &data.clock_source),
                (n_("Gear source:"), &data.gear_source),
            ] {
                if let Some(value) = value {
                    l.hardware.push((i18n(label), value.clone()));
                }
            }
        }
        if let Some(voltage) = mem.voltage_v.or_else(|| {
            readings
                .hardware
                .as_ref()
                .and_then(|data| data.ram_voltage_v)
        }) {
            l.hardware
                .push((i18n("RAM voltage:"), format!("{voltage:.3} V")));
        }
        for (label, count) in [
            (
                n_("Corrected errors (EDAC):"),
                readings.mem_info.corrected_errors,
            ),
            (
                n_("Uncorrected errors (EDAC):"),
                readings.mem_info.uncorrected_errors,
            ),
        ] {
            if let Some(count) = count {
                let row = (i18n(label), count.to_string());
                l.unprivileged_hardware.push(row.clone());
                l.hardware.push(row);
            }
        }
        if readings.mem_info.hardware_corrupted > 0 {
            let row = (
                i18n("Hardware-corrupted memory:"),
                to_human_readable_nice(
                    readings.mem_info.hardware_corrupted as f32,
                    &DataType::MemoryBytes,
                ),
            );
            l.unprivileged_hardware.push(row.clone());
            l.hardware.push(row);
        }
        l.in_use = h(used);
        l.free = h(accounting.free);
        l.available = h(mem.mem_available);
        l.committed = h(mem.committed);
        l.cached = h(accounting.cache);
        let installed = readings
            .mem_devices
            .iter()
            .try_fold(0_u64, |sum, module| {
                sum.checked_add(super::mem_slots::known_capacity(module)?)
            })
            .filter(|total| *total >= mem.mem_total && *total > 0)
            .filter(|_| {
                super::mem_slots::inventory_complete(&readings.mem_devices, mem.max_devices)
            });
        l.installed = installed.map(h);
        l.reserved = installed.map(|total| h(total - mem.mem_total));
        let memory_kind = memory_kind_label(&readings.mem_devices);
        let capacity = installed.unwrap_or(mem.mem_total);
        let capacity = if capacity > 0 && capacity.is_multiple_of(1024 * 1024 * 1024) {
            format!("{} GiB", capacity / (1024 * 1024 * 1024))
        } else {
            h(capacity)
        };
        l.capacity_heading = if memory_kind.is_empty() {
            capacity
        } else {
            format!("{capacity} · {memory_kind}")
        };
        l.capacity_tooltip = if let (Some(installed), Some(reserved)) = (&l.installed, &l.reserved)
        {
            i18n_f(
                "Installed: {}\nOS-managed RAM: {}\nOutside OS-managed RAM: {}\n\nThe difference includes firmware reservations and early kernel allocations.",
                &[installed, &self.total_ram, reserved],
            )
        } else {
            i18n_f(
                "OS-managed RAM: {}\n\nInstalled capacity cannot be determined reliably from the available memory inventory.",
                &[&self.total_ram],
            )
        };
        if !memory_kind.is_empty() {
            l.capacity_tooltip = format!("{}\n\n{}", memory_kind, l.capacity_tooltip);
        }
        self.has_swap = mem.swap_total != 0;
        if self.has_swap {
            l.swap_total = Some(h(mem.swap_total));
            l.swap_used = Some(h(swap_used));
            l.swap_free = Some(h(mem.swap_free));
        } else {
            l.swap_total = None;
            l.swap_used = None;
            l.swap_free = None;
        }
        l.usage_tooltip = i18n_f(
            n_(
                "Occupied RAM, including cache ({})\n\nTotal minus MemFree. Includes programs, kernel, drivers, shared memory, buffers and cache. Cache is part of this value, not an additional allocation. MemAvailable is a separate estimate of reusable memory.",
            ),
            &[&l.in_use],
        );
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
        self.summary.info2 = format!(
            "{}/{} ({}%)",
            h(used),
            h(mem.mem_total),
            (used as f32 / total as f32 * 100.).round()
        );
        self.summary.info1 = module_model_label(&readings.mem_devices, mem.max_devices);

        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        self.usage.set_grid_offset(offset) | self.swap.set_grid_offset(offset)
    }

    fn update_spd(&mut self, privileged: bool, result: &Result<Vec<mc_magpie::SpdEeprom>, String>) {
        self.slots.update_spd(privileged, result);
    }
    fn clear_privileged_spd(&mut self) {
        self.slots.clear_privileged_spd();
    }
    fn set_spd_pending(&mut self, pending: bool) {
        self.slots.set_pending(pending);
    }
    fn clear_privileged_hardware(&mut self) {
        self.labels
            .hardware
            .clone_from(&self.labels.unprivileged_hardware);
    }
    fn extra_ui(&mut self, ui: &mut Ui, rect: egui::Rect, module_height: f32, cx: &mut Cx) {
        self.slots.ui(ui, rect, module_height, cx);
    }

    fn win10_view(&mut self, config: &Config) -> PageView<'_> {
        let l = &self.labels;
        let stats = vec![
            vec![
                (i18n("Used (incl. cache)"), l.in_use.clone()),
                (i18n("Free"), l.free.clone()),
            ],
            vec![
                (i18n("Of used: cache"), l.cached.clone()),
                (i18n("Committed"), l.committed.clone()),
            ],
        ];
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
        info.extend(l.hardware.clone());
        if let Some(installed) = &l.installed {
            info.push((i18n("Installed:"), installed.clone()));
            info.push((
                i18n("Outside OS-managed RAM:"),
                l.reserved.clone().unwrap_or_default(),
            ));
        }
        info.push((i18n("OS-managed RAM:"), self.total_ram.clone()));
        info.push((i18n("Available (estimate):"), l.available.clone()));
        let show_swap = config.performance_page_memory_swap_visible && self.has_swap;
        let mut panels = vec![GraphPanel::single(
            over(
                n_("RAM occupied (including cache) over "),
                &self.max_duration,
            ),
            self.max_graph_ram.clone(),
            &mut self.usage,
        )];
        panels[0].tooltips = std::slice::from_ref(&l.usage_tooltip);
        if show_swap {
            panels.push(GraphPanel::single(
                over(n_("Swap usage over "), &self.max_duration),
                self.total_swap.clone(),
                &mut self.swap,
            ));
        }
        PageView {
            title: i18n("Memory"),
            model: l.capacity_heading.clone(),
            model_tooltip: Some(&l.capacity_tooltip),
            panels,
            stats,
            info,
            composition: Some(&self.composition),
            swap_compression: Some(&self.swap_compression),
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

    Used (incl. cache): {}
    Free:           {}
    Available:      {}
    Committed:      {}
    Of used: cache: {}
    Swap total:     {}
    Swap used:      {}
    Swap free:      {}
    zram:           {}
    zswap:          {}

    Speed:       {}
    Slots used:  {}
    Form factor: {}
    Type:        {}"#,
            self.total_ram,
            l.in_use,
            l.free,
            l.available,
            l.committed,
            l.cached,
            o(&l.swap_total),
            o(&l.swap_used),
            o(&l.swap_free),
            o(&self.swap_compression.summary(Layer::Zram)),
            o(&self.swap_compression.summary(Layer::Zswap)),
            self.speed,
            self.slots_used,
            self.form_factor,
            self.ram_type,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_heading_identifies_package_and_technology_without_guessing() {
        let module = |form: &str, kind: &str| mc_magpie::MemoryDevice {
            size: 8 * 1024 * 1024 * 1024,
            form_factor: form.into(),
            ram_type: kind.into(),
            ..Default::default()
        };
        assert_eq!(memory_kind_label(&[module("DIMM", "DDR4")]), "DIMM DDR4");
        assert_eq!(
            memory_kind_label(&[module("SODIMM", "DDR5")]),
            "SO-DIMM DDR5"
        );
        assert_eq!(
            memory_kind_label(&[module("SO-DIMM", "DDR3")]),
            "SO-DIMM DDR3"
        );
        for kind in ["LPDDR4", "LPDDR4X", "LPDDR5", "LPDDR5X"] {
            assert_eq!(memory_kind_label(&[module("Row of chips", kind)]), kind);
        }
        let mut modules = vec![module("DIMM", "DDR4"), module("DIMM", "DDR4")];
        modules.push(mc_magpie::MemoryDevice {
            present: Some(false),
            form_factor: "SODIMM".into(),
            ram_type: "DDR5".into(),
            ..Default::default()
        });
        assert_eq!(memory_kind_label(&modules), "DIMM DDR4");
        modules[1] = module("SODIMM", "DDR5");
        assert_eq!(memory_kind_label(&modules), "DIMM DDR4 / SO-DIMM DDR5");
        assert_eq!(memory_kind_label(&[module("", "DDR5")]), "DDR5");
        assert_eq!(memory_kind_label(&[module("DIMM", "Unknown")]), "DIMM");
        assert_eq!(
            memory_kind_label(&[module("Unknown", "Unknown")]),
            "Unknown"
        );
        assert!(memory_kind_label(&[]).is_empty());
    }
    #[test]
    fn sidebar_memory_model_never_selects_an_arbitrary_module() {
        let module = |part: &str| mc_magpie::MemoryDevice {
            size: 8 * 1024 * 1024 * 1024,
            manufacturer: Some("G.Skill".into()),
            part_number: Some(part.into()),
            ..Default::default()
        };
        let mut modules = vec![module("F4-3200"), module("F4-3200")];
        assert_eq!(module_model_label(&modules, 2), "G.Skill F4-3200");
        assert_eq!(module_model_label(&modules, 3), "Unknown");
        assert_eq!(module_model_label(&modules, 1), "Unknown");
        assert_eq!(module_model_label(&modules, 0), "Unknown");
        modules.push(mc_magpie::MemoryDevice {
            present: Some(false),
            ..Default::default()
        });
        assert_eq!(module_model_label(&modules, 3), "G.Skill F4-3200");
        modules[2].present = None;
        assert_eq!(module_model_label(&modules, 3), "Unknown");
        modules[2].present = Some(true);
        assert_eq!(module_model_label(&modules, 3), "Unknown");
        modules[2].present = Some(false);
        modules[1].part_number = Some("F4-3600".into());
        assert_eq!(module_model_label(&modules, 3), "Mixed modules");
        modules[1].part_number = None;
        assert_eq!(module_model_label(&modules, 3), "Unknown");
        assert_eq!(module_model_label(&[], 0), "Unknown");
        modules[0].part_number = Some("Default string".into());
        assert_eq!(module_model_label(&modules, 3), "Unknown");
    }

    #[test]
    fn late_inventory_updates_capacity_and_common_or_mixed_identity() {
        let mut r = mc_magpie::synthetic_readings(0, 1);
        r.mem_info.mem_total = 31 * 1024 * 1024 * 1024;
        r.mem_info.max_devices = 2;
        for module in &mut r.mem_devices {
            module.manufacturer = Some("Example".into());
            module.part_number = Some("DDR5-5600".into());
        }
        let modules = std::mem::take(&mut r.mem_devices);
        let config = Config::default();
        let env = UpdateEnv::new(&config);
        let mut page = MemoryPage::new(&r, &config);
        page.update(&r, &env);
        assert_eq!(page.summary.info1, "Unknown");
        assert_eq!(page.win10_view(&config).model, "31 GiB");
        assert!(page.labels.installed.is_none());

        r.mem_devices = modules;
        page.update(&r, &env);
        assert_eq!(page.summary.info1, "Example DDR5-5600");
        assert_eq!(page.win10_view(&config).model, "32 GiB · SO-DIMM DDR5");
        assert_eq!(page.labels.installed.as_deref(), Some("32.0 GiB"));
        r.mem_devices[1].part_number = Some("DDR5-6000".into());
        page.update(&r, &env);
        assert_eq!(page.summary.info1, "Mixed modules");
        r.mem_devices.pop();
        page.update(&r, &env);
        assert_eq!(page.summary.info1, "Unknown");
        assert!(page.labels.installed.is_none());
        assert_eq!(page.win10_view(&config).model, "31 GiB · SO-DIMM DDR5");
    }

    #[test]
    fn installed_capacity_rejects_unknown_contradictory_or_overflowing_totals() {
        let mut r = mc_magpie::synthetic_readings(0, 1);
        r.mem_info.max_devices = 2;
        let original = r.mem_devices.clone();
        let config = Config::default();
        let env = UpdateEnv::new(&config);
        let mut page = MemoryPage::new(&r, &config);
        for case in 0..8 {
            r.mem_devices.clone_from(&original);
            r.mem_info.max_devices = 2;
            match case {
                0 => r.mem_devices[1].size = 0,
                1 => {
                    r.mem_devices[1].size = 0;
                    r.mem_devices[1].present = Some(true);
                }
                2 => r.mem_devices.push(original[0].clone()),
                3 => r.mem_info.max_devices = 0,
                4 => r.mem_devices[0].size = u64::MAX,
                5 => r.mem_devices[1].size = 1,
                6 => {
                    r.mem_devices[0].slot_index = Some(0);
                    r.mem_devices[1].slot_index = Some(2);
                }
                7 => r.mem_devices[0].slot_index = Some(0),
                _ => unreachable!(),
            }
            page.update(&r, &env);
            assert!(page.labels.installed.is_none(), "case {case}");
            assert!(page.labels.reserved.is_none(), "case {case}");
            assert!(
                page.labels
                    .capacity_tooltip
                    .contains("cannot be determined reliably")
            );
            assert!(
                !page
                    .labels
                    .capacity_tooltip
                    .contains("Outside OS-managed RAM:")
            );
        }
        r.mem_devices = original;
        r.mem_devices[1].present = Some(false);
        r.mem_info.mem_total = 15 * 1024 * 1024 * 1024;
        page.update(&r, &env);
        assert_eq!(page.labels.installed.as_deref(), Some("16.0 GiB"));
        assert_eq!(page.labels.reserved.as_deref(), Some("1.00 GiB"));
    }

    #[test]
    fn heading_formats_capacity_from_bytes_without_string_replacement() {
        let config = Config::default();
        let mut r = mc_magpie::synthetic_readings(0, 1);
        r.mem_devices.clear();
        let mut page = MemoryPage::new(&r, &config);
        for (bytes, expected) in [
            (8 * 1024 * 1024 * 1024, "8 GiB"),
            (16 * 1024 * 1024 * 1024, "16 GiB"),
            (32 * 1024 * 1024 * 1024, "32 GiB"),
            (1536 * 1024 * 1024, "1.50 GiB"),
        ] {
            r.mem_info.mem_total = bytes;
            page.update(&r, &UpdateEnv::new(&config));
            assert_eq!(page.win10_view(&config).model, expected);
        }
    }
    #[test]
    fn sidebar_stats_graph_and_process_header_share_physical_occupation() {
        let mut r = mc_magpie::synthetic_readings(0, 1);
        let gib = 1024 * 1024 * 1024;
        r.mem_info.mem_total = 32 * gib;
        r.mem_info.mem_free = 16 * gib;
        r.mem_info.mem_available = 26 * gib;
        r.mem_info.cached = 9 * gib;
        r.mem_info.sh_mem = gib;
        r.mem_info.s_reclaimable = gib;
        let config = Config::default();
        let mut page = MemoryPage::new(&r, &config);
        page.update(&r, &UpdateEnv::new(&config));
        assert_eq!(page.summary.info2, "16.0 GiB/32.0 GiB (50%)");
        assert_eq!(page.labels.in_use, "16.0 GiB");
        assert_eq!(page.labels.free, "16.0 GiB");
        assert_eq!(page.labels.cached, "9.00 GiB");
        assert_eq!(
            page.usage.groups()[2].datas[0].get_data()[0],
            (16 * gib) as f32
        );
        assert_eq!(
            page.summary.graph.groups()[1].datas[0].get_data()[0],
            (16 * gib) as f32
        );
        assert!(!page.usage.groups()[0].dataset_settings.visible);
        assert!(!page.usage.groups()[1].dataset_settings.visible);
        assert!(page.usage.groups()[2].dataset_settings.visible);
        assert!(!page.summary.graph.groups()[0].dataset_settings.visible);
        assert!(page.summary.graph.groups()[1].dataset_settings.visible);
        r.mem_info.mem_available = 30 * gib;
        r.mem_info.committed = 100 * gib;
        r.mem_info.dirty = gib;
        page.update(&r, &UpdateEnv::new(&config));
        assert_eq!(
            page.usage.groups()[2].datas[0].get_data()[0],
            (16 * gib) as f32
        );
        assert_eq!(
            page.summary.graph.groups()[1].datas[0].get_data()[0],
            (16 * gib) as f32
        );
        assert_eq!(page.usage.dataset_max_scale(2), (32 * gib) as f32);
        let view = page.win10_view(&config);
        assert_eq!(view.stats[0][0].0, "Used (incl. cache)");
        assert_eq!(view.stats[0][1].0, "Free");
        assert!(view.panels[0].tooltips[0].contains("Total minus MemFree"));
        let snapshot = mc_core::gpu::GpuSnapshot::default();
        let totals = mc_core::table::ColumnTotals::from_readings(&r, &snapshot);
        assert_eq!(totals.memory_percent, 50.);
        r.mem_info.mem_free = u64::MAX;
        page.update(&r, &UpdateEnv::new(&config));
        assert_eq!(page.usage.groups()[2].datas[0].get_data()[0], 0.);
        assert_eq!(
            mc_core::table::ColumnTotals::from_readings(&r, &snapshot).memory_percent,
            0.
        );
        r.mem_info.mem_total = 0;
        page.update(&r, &UpdateEnv::new(&config));
        assert_eq!(page.summary.graph.groups()[1].datas[0].get_data()[0], 0.);
        assert_eq!(
            mc_core::table::ColumnTotals::from_readings(&r, &snapshot).memory_percent,
            0.
        );
    }

    #[test]
    fn divider_requires_two_valid_clocks_and_known_ratio() {
        assert_eq!(memory_divider(3000., 3000.), Some(1));
        assert_eq!(memory_divider(1500., 3000.), Some(2));
        assert_eq!(memory_divider(1000., 3000.), Some(3));
        assert_eq!(memory_divider(2000., 3000.), None);
        assert_eq!(memory_divider(0., 3000.), None);
        assert_eq!(memory_divider(f32::NAN, 3000.), None);
    }

    #[test]
    fn clock_metadata_keeps_table_sample_coherent_and_reports_gear_source() {
        let mut readings = mc_magpie::synthetic_readings(0, 1);
        readings.mem_info.uclk_mhz = Some(1900.);
        readings.mem_info.mclk_mhz = Some(1900.);
        readings.hardware = Some(mc_magpie::HardwareTelemetry {
            fclk_mhz: Some(1800.),
            mclk_mhz: Some(1600.),
            uclk_mhz: None,
            clock_source: Some("Linux HSMP GET_FCLK_MCLK".into()),
            intel_gear: Some(2),
            gear_source: Some("MCHBAR SC_GS_CFG".into()),
            ..Default::default()
        });
        let config = Config::default();
        let mut page = MemoryPage::new(&readings, &config);
        page.update(&readings, &UpdateEnv::new(&config));
        let info = page.win10_view(&config).info;
        assert!(info.iter().any(|(k, v)| k == "MCLK:" && v == "1600 MHz"));
        assert!(!info.iter().any(|(k, _)| k == "UCLK:" || k == "UCLK:MCLK:"));
        assert!(info.iter().any(|(k, v)| k == "Intel Gear:" && v == "2"));
        readings.hardware.as_mut().unwrap().uclk_mhz = Some(800.);
        page.update(&readings, &UpdateEnv::new(&config));
        assert!(
            page.win10_view(&config)
                .info
                .iter()
                .any(|(k, v)| k == "UCLK:MCLK:" && v == "1:2")
        );
    }
}
