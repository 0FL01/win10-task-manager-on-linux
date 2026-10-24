/* crates/mc-ui/src/perf/cpu.rs
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
use mc_core::format::{DataType, to_human_readable_nice, to_uptime};
use mc_core::i18n::n_;
use mc_core::i18n::{i18n, i18n_f, strip_mnemonic};
use mc_core::sidebar::DeviceType;
use mc_graph::{DatasetGroup, FillingSettings, Graph, RoundingSettings, ScalingSettings};
use mc_magpie::Readings;

use super::{DevicePage, SummaryGraph, UpdateEnv, fixed_group};
use crate::cx::Cx;
const COLOR: egui::Color32 = crate::win10::palette::CPU.line;

const GRAPH_SELECTION_OVERALL: i32 = 1;
const GRAPH_SELECTION_ALL: i32 = 2;
const GRAPH_SELECTION_ALL_THREADS: i32 = 3;
const GRAPH_SELECTION_ALL_THREADS_STACKED: i32 = 4;

const GRAPH_NONE: i32 = 0;
const GRAPH_POWER: i32 = 1;
const GRAPH_CLOCK: i32 = 2;
const GRAPH_TEMPERATURE: i32 = 3;

const POWER_DATASET: usize = 0;
const CLOCK_DATASET: usize = 1;
const TEMPERATURE_DATASET: usize = 2;

pub const TEMPERATURE_HIGH_WATERMARK: f32 = 45.;
pub const TEMPERATURE_LOW_WATERMARK: f32 = 35.;

#[derive(Default)]
struct Labels {
    utilization: String,
    speed: String,
    power_draw: Option<String>,
    processes: String,
    threads: String,
    handles: String,
    uptime: String,
    temperature: Option<String>,
    cpufreq_driver: Option<String>,
    cpufreq_governor: Option<String>,
    power_preference: Option<String>,
    hardware: Vec<(String, String)>,
    unprivileged_hardware: Vec<(String, String)>,
}

pub struct CpuPage {
    summary: SummaryGraph,
    cpu_name: String,
    overall: Graph,
    thread_wise: Graph,
    per_core: Vec<Graph>,
    core_tooltips: Vec<String>,
    topology: Vec<mc_magpie::magpie_types::cpu::LogicalCpu>,
    bottom: Graph,
    is_bogomips: bool,
    has_power: bool,
    has_temperature: bool,

    base_speed: String,
    sockets: String,
    virt_proc: String,
    virtualization: String,
    virt_machine: String,
    l1_cache: String,
    l2_cache: String,
    l3_cache: String,
    labels: Labels,

    max_duration: String,
}

fn compute_column_count(n: usize) -> usize {
    if n <= 3 {
        return n.max(1);
    }
    let sqrt = (n as f64).sqrt().round() as usize;
    (sqrt..n.min(sqrt * 2))
        .find(|i| n.is_multiple_of(*i))
        .unwrap_or(sqrt)
}

impl CpuPage {
    pub fn new(readings: &Readings, config: &Config) -> Self {
        let mut summary = SummaryGraph::new(COLOR, i18n("CPU"));
        summary.graph.add_dataset(fixed_group(100.));
        summary.info1 = "0% 0.00 GHz".into();

        let mut bottom = Graph::new();
        bottom.set_base_color(COLOR);
        let mut power = DatasetGroup::new();
        power.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
        power.dataset_settings.rounding_settings = RoundingSettings::Integer;
        power.dataset_settings.high_watermark = 0.;
        let mut clock = DatasetGroup::new();
        clock.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
        clock.dataset_settings.high_watermark = 0.;
        let mut temp = DatasetGroup::new();
        temp.dataset_settings.scaling_settings = ScalingSettings::StickyUpDown;
        temp.dataset_settings.rounding_settings = RoundingSettings::Integer;
        temp.dataset_settings.high_watermark = TEMPERATURE_HIGH_WATERMARK;
        temp.dataset_settings.low_watermark = TEMPERATURE_LOW_WATERMARK;
        bottom.add_dataset(power);
        bottom.add_dataset(clock);
        bottom.add_dataset(temp);

        let mut page = Self {
            summary,
            cpu_name: String::new(),
            overall: Graph::new(),
            thread_wise: Graph::new(),
            per_core: vec![],
            core_tooltips: vec![],
            topology: vec![],
            bottom,
            is_bogomips: false,
            has_power: false,
            has_temperature: false,
            base_speed: String::new(),
            sockets: String::new(),
            virt_proc: String::new(),
            virtualization: String::new(),
            virt_machine: String::new(),
            l1_cache: String::new(),
            l2_cache: String::new(),
            l3_cache: String::new(),
            labels: Labels::default(),
            max_duration: super::max_duration(config),
        };
        page.set_static_information(readings, config);
        page
    }

    fn populate_usage_graphs(&mut self, cpu_count: usize, config: &Config) {
        self.virt_proc = cpu_count.to_string();
        let show_kernel = config.performance_page_kernel_times;

        let mut overall = Graph::new();
        overall.set_base_color(COLOR);
        overall.add_dataset(fixed_group(100.));
        let mut kernel = fixed_group(100.);
        kernel.dataset_settings.fill = FillingSettings::None;
        kernel.dataset_settings.dashed = true;
        kernel.dataset_settings.visible = show_kernel;
        overall.add_dataset(kernel);
        self.overall = overall;

        let mut thread_wise = Graph::new();
        thread_wise.set_base_color(COLOR);
        let mut group = DatasetGroup::new();
        group.set_datasets(cpu_count);
        thread_wise.add_dataset(group);
        self.thread_wise = thread_wise;
        self.configure_thread_wise(config.performance_page_cpu_graph, cpu_count);

        self.per_core = (0..cpu_count)
            .map(|_| {
                let mut g = Graph::new();
                g.set_base_color(COLOR);
                g.add_dataset(fixed_group(100.));
                let mut kernel = fixed_group(100.);
                kernel.dataset_settings.fill = FillingSettings::None;
                kernel.dataset_settings.dashed = true;
                kernel.dataset_settings.visible = show_kernel;
                g.add_dataset(kernel);
                g
            })
            .collect();
        // The backend exposes array ordinals, not kernel CPU IDs or physical core IDs.
        self.core_tooltips = (0..cpu_count)
            .map(|index| i18n_f(n_("CPU {}"), &[&index.to_string()]))
            .collect();
    }

    fn configure_thread_wise(&mut self, selection: i32, cpu_count: usize) {
        if selection == GRAPH_SELECTION_ALL_THREADS_STACKED {
            self.thread_wise
                .set_dataset_scaling(0, ScalingSettings::Stacking);
            self.thread_wise
                .set_dataset_max_scale(0, 100. * cpu_count as f32);
            self.thread_wise
                .set_filled(0, FillingSettings::FillToBottom);
        } else {
            self.thread_wise
                .set_dataset_scaling(0, ScalingSettings::Fixed);
            self.thread_wise.set_dataset_max_scale(0, 100.);
        }
    }

    fn set_static_information(&mut self, readings: &Readings, config: &Config) {
        let cpu = &readings.cpu;
        self.cpu_name = cpu.name.clone().unwrap_or_else(|| i18n("Unknown"));
        self.populate_usage_graphs(cpu.core_usage_percent.len(), config);

        self.is_bogomips = cpu.frequency_driver.as_deref() == Some("bogomips");
        self.base_speed = match cpu.base_freq_khz {
            Some(khz) => {
                if self.is_bogomips {
                    let freq = khz as f32 / 1000.;
                    self.bottom.set_dataset_max_scale(CLOCK_DATASET, freq);
                    format!("{freq:.2} BogoMIPS")
                } else {
                    let hz = khz as f32 * 1000.;
                    self.bottom.set_dataset_max_scale(CLOCK_DATASET, hz);
                    to_human_readable_nice(hz, &DataType::Hertz)
                }
            }
            None => i18n("Unknown"),
        };

        self.virtualization = cpu
            .virtualization_technology
            .clone()
            .unwrap_or_else(|| i18n("Unsupported"));
        self.virt_machine = match cpu.is_virtual_machine {
            Some(true) => i18n("Yes"),
            Some(false) => i18n("No"),
            None => i18n("Unknown"),
        };
        self.sockets = cpu
            .socket_count
            .map(|c| c.to_string())
            .unwrap_or_else(|| i18n("Unknown"));
        let cache = |v: Option<u64>| {
            v.map(|b| to_human_readable_nice(b as f32, &DataType::MemoryBytes))
                .unwrap_or_else(|| i18n("N/A"))
        };
        self.l1_cache = cache(cpu.l1_combined_cache_bytes);
        self.l2_cache = cache(cpu.l2_cache_bytes);
        self.l3_cache = cache(cpu.l3_cache_bytes);

        self.has_power = cpu.power_draw_w.is_some();
        self.has_temperature = cpu.temperature_celsius.is_some();
    }

    /// The bottom graph selection, falling back when the data is unavailable.
    fn bottom_selection(&self, config: &Config) -> i32 {
        let n = config.performance_page_cpu_graph_bottom;
        if n == GRAPH_POWER && !self.has_power {
            GRAPH_TEMPERATURE
        } else if n == GRAPH_TEMPERATURE && !self.has_temperature {
            GRAPH_CLOCK
        } else {
            n
        }
    }

    fn bottom_total(&self, selection: i32) -> String {
        match selection {
            GRAPH_CLOCK if self.is_bogomips => {
                format!(
                    "{:.2} BogoMIPS",
                    self.bottom.dataset_max_scale(CLOCK_DATASET)
                )
            }
            GRAPH_POWER => to_human_readable_nice(
                self.bottom.dataset_max_scale(POWER_DATASET),
                &DataType::Watts,
            ),
            GRAPH_CLOCK => to_human_readable_nice(
                self.bottom.dataset_max_scale(CLOCK_DATASET),
                &DataType::Hertz,
            ),
            GRAPH_TEMPERATURE => {
                format!(
                    "{:.0} °C",
                    self.bottom.dataset_max_scale(TEMPERATURE_DATASET)
                )
            }
            _ => String::new(),
        }
    }
}

impl DevicePage for CpuPage {
    fn name(&self) -> &str {
        "cpu"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Cpu
    }

    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }

    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let cpu = &readings.cpu;
        self.max_duration = env.max_duration.clone();

        let topology_changed = self.topology != cpu.logical_cpus;
        if self.per_core.len() != cpu.core_usage_percent.len() || topology_changed {
            self.set_static_information(readings, env.config);
        }

        self.overall.add_data_point(vec![
            vec![cpu.total_usage_percent],
            vec![cpu.kernel_usage_percent],
        ]);
        self.thread_wise
            .add_data_point(vec![cpu.core_usage_percent.clone()]);
        for (i, g) in self.per_core.iter_mut().enumerate() {
            let usage = cpu.core_usage_percent.get(i).copied().unwrap_or_default();
            let kernel = cpu
                .core_kernel_usage_percent
                .get(i)
                .copied()
                .unwrap_or_default();
            g.add_data_point(vec![vec![usage], vec![kernel]]);
        }

        let l = &mut self.labels;
        l.hardware.clear();
        l.unprivileged_hardware.clear();
        if let Some(bclk) = cpu.bclk_mhz {
            l.unprivileged_hardware
                .push((i18n("BCLK:"), format!("{bclk:.2} MHz")));
        }
        if let Some(voltage) = cpu.voltage_v {
            l.unprivileged_hardware
                .push((i18n("CPU voltage:"), format!("{voltage:.3} V")));
        }
        if let Some(bclk) = cpu
            .bclk_mhz
            .or_else(|| readings.hardware.as_ref().and_then(|data| data.bclk_mhz))
        {
            l.hardware.push((i18n("BCLK:"), format!("{bclk:.2} MHz")));
        }
        if let Some(ratio) = readings
            .hardware
            .as_ref()
            .and_then(|data| data.cpu_multiplier)
        {
            l.hardware
                .push((i18n("CPU multiplier:"), format!("×{ratio:.1}")));
        }
        if let Some(voltage) = cpu.voltage_v.or_else(|| {
            readings
                .hardware
                .as_ref()
                .and_then(|data| data.cpu_voltage_v)
        }) {
            l.hardware
                .push((i18n("CPU voltage:"), format!("{voltage:.3} V")));
        }
        if let Some(data) = &readings.hardware {
            for (label, value) in [
                (n_("Multiplier source:"), &data.multiplier_source),
                (n_("BCLK source:"), &data.bclk_source),
                (n_("Voltage source:"), &data.voltage_source),
            ] {
                if let Some(value) = value {
                    l.hardware.push((i18n(label), value.clone()));
                }
            }
        }
        if !cpu.logical_cpus.is_empty() {
            let cores: std::collections::HashSet<_> = cpu
                .logical_cpus
                .iter()
                .filter_map(|c| Some((c.package_id?, c.core_id?)))
                .collect();
            if !cores.is_empty() {
                let row = (i18n("Cores:"), cores.len().to_string());
                l.unprivileged_hardware.push(row.clone());
                l.hardware.push(row);
            }
            let mut kinds = std::collections::BTreeMap::<&str, std::collections::HashSet<_>>::new();
            for c in &cpu.logical_cpus {
                if let (Some(kind), Some(package), Some(core)) = (&c.kind, c.package_id, c.core_id)
                {
                    kinds.entry(kind).or_default().insert((package, core));
                }
            }
            if !kinds.is_empty() {
                let row = (
                    i18n("Core types:"),
                    kinds
                        .iter()
                        .map(|(kind, cores)| format!("{} {kind}", cores.len()))
                        .collect::<Vec<_>>()
                        .join(" + "),
                );
                l.unprivileged_hardware.push(row.clone());
                l.hardware.push(row);
            }
            if topology_changed {
                self.core_tooltips = cpu
                    .logical_cpus
                    .iter()
                    .map(|c| {
                        let mut label = format!("CPU {}", c.id);
                        if let Some(kind) = &c.kind {
                            label.push_str(&format!(" · {kind}"));
                        }
                        if let (Some(package), Some(core)) = (c.package_id, c.core_id) {
                            label.push_str(&format!(
                                " · {} {package} · {} {core}",
                                i18n("Socket"),
                                i18n("Core")
                            ));
                        }
                        label
                    })
                    .collect();
            }
        }
        if topology_changed {
            self.topology.clone_from(&cpu.logical_cpus);
        }
        l.utilization = format!("{}%", cpu.total_usage_percent.round());
        let freq_text = if self.is_bogomips {
            self.bottom
                .add_single_data_point(CLOCK_DATASET, vec![cpu.current_frequency_mhz as f32]);
            format!("{:.2} BogoMIPS", cpu.current_frequency_mhz as f32)
        } else {
            let hz = cpu.current_frequency_mhz as f32 * 1_000_000.;
            self.bottom.add_single_data_point(CLOCK_DATASET, vec![hz]);
            to_human_readable_nice(hz, &DataType::Hertz)
        };
        l.speed = freq_text.clone();
        l.power_draw = cpu.power_draw_w.map(|w| {
            self.bottom.add_single_data_point(POWER_DATASET, vec![w]);
            to_human_readable_nice(w, &DataType::Watts)
        });
        l.processes = cpu.total_process_count.to_string();
        l.threads = cpu.total_thread_count.to_string();
        l.handles = cpu.total_handle_count.to_string();
        l.uptime = to_uptime(cpu.uptime_seconds);
        l.cpufreq_driver = cpu.frequency_driver.clone();
        l.cpufreq_governor = cpu.frequency_governor.clone();
        l.power_preference = cpu.power_preference.clone();
        l.temperature = cpu.temperature_celsius.map(|t| {
            self.bottom
                .add_single_data_point(TEMPERATURE_DATASET, vec![t]);
            format!("{t:.0} °C")
        });
        self.has_power |= cpu.power_draw_w.is_some();
        self.has_temperature |= cpu.temperature_celsius.is_some();

        // Sidebar
        self.summary
            .graph
            .add_data_point(vec![vec![cpu.total_usage_percent]]);
        // Win10 list: "percent speed" on one line
        let mut info1 = format!("{}% {freq_text}", cpu.total_usage_percent.round());
        if let Some(t) = cpu.temperature_celsius {
            info1.push_str(&format!(" ({t:.0} °C)"));
        }
        self.summary.info1.clone_from(&self.cpu_name);
        self.summary.info2 = info1;

        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        let mut repaint = self.overall.set_grid_offset(offset);
        repaint |= self.thread_wise.set_grid_offset(offset);
        repaint |= self.bottom.set_grid_offset(offset);
        for g in self.per_core.iter_mut() {
            repaint |= g.set_grid_offset(offset);
        }
        repaint
    }

    fn clear_privileged_hardware(&mut self) {
        self.labels
            .hardware
            .clone_from(&self.labels.unprivileged_hardware);
    }
    fn win10_view(&mut self, config: &Config) -> PageView<'_> {
        let selection = config.performance_page_cpu_graph;
        let bottom = self.bottom_selection(config);
        self.bottom
            .set_data_visible(POWER_DATASET, bottom == GRAPH_POWER);
        self.bottom
            .set_data_visible(CLOCK_DATASET, bottom == GRAPH_CLOCK);
        self.bottom
            .set_data_visible(TEMPERATURE_DATASET, bottom == GRAPH_TEMPERATURE);
        let bottom_total = self.bottom_total(bottom);

        let l = &self.labels;
        // Extra values go next to "Up time" (the shortest row) so the statistics stay narrow
        let mut third = vec![(i18n("Up time"), l.uptime.clone())];
        if let Some(temp) = &l.temperature {
            third.push((i18n("Temperature"), temp.clone()));
        }
        if let Some(power) = &l.power_draw {
            third.push((i18n("Power Draw"), power.clone()));
        }
        let stats = vec![
            vec![
                (i18n("Utilization"), l.utilization.clone()),
                (i18n("Speed"), l.speed.clone()),
            ],
            vec![
                (i18n("Processes"), l.processes.clone()),
                (i18n("Threads"), l.threads.clone()),
                (i18n("Handles"), l.handles.clone()),
            ],
            third,
        ];
        let mut info = vec![
            (i18n("Base Speed:"), self.base_speed.clone()),
            (i18n("Sockets:"), self.sockets.clone()),
            (i18n("Virtual processors:"), self.virt_proc.clone()),
            (i18n("Virtualization:"), self.virtualization.clone()),
            (i18n("Virtual machine:"), self.virt_machine.clone()),
            (i18n("L1 cache:"), self.l1_cache.clone()),
            (i18n("L2 cache:"), self.l2_cache.clone()),
            (i18n("L3 cache:"), self.l3_cache.clone()),
        ];
        for (label, value) in [
            (n_("Cpufreq driver:"), &l.cpufreq_driver),
            (n_("Cpufreq governor:"), &l.cpufreq_governor),
            (n_("Power preference:"), &l.power_preference),
        ] {
            if let Some(v) = value {
                info.push((i18n(label), v.clone()));
            }
        }
        info.extend(l.hardware.clone());

        let caption = over(n_("Utilization over "), &self.max_duration);
        let (graphs, columns): (Vec<&mut Graph>, usize) = match selection {
            GRAPH_SELECTION_ALL => {
                let columns = compute_column_count(self.per_core.len().max(1));
                (self.per_core.iter_mut().collect(), columns)
            }
            GRAPH_SELECTION_ALL_THREADS | GRAPH_SELECTION_ALL_THREADS_STACKED => {
                (vec![&mut self.thread_wise], 1)
            }
            _ => (vec![&mut self.overall], 1),
        };
        let mut panels = vec![GraphPanel {
            caption,
            right: i18n("100%"),
            graphs,
            columns,
            tooltips: if selection == GRAPH_SELECTION_ALL {
                &self.core_tooltips
            } else {
                &[]
            },
        }];
        if bottom != GRAPH_NONE {
            let label = match bottom {
                GRAPH_POWER => n_("Power draw over "),
                GRAPH_CLOCK => n_("Clock speed over "),
                _ => n_("Temperature over "),
            };
            panels.push(GraphPanel::single(
                over(label, &self.max_duration),
                bottom_total,
                &mut self.bottom,
            ));
        }
        PageView {
            model_tooltip: None,
            title: i18n("CPU"),
            model: self.cpu_name.clone(),
            panels,
            stats,
            stat_legends: vec![],
            info,
            composition: None,
            swap_compression: None,
        }
    }

    fn win10_menu(&mut self, ui: &mut Ui, cx: &mut Cx) {
        use crate::win10::menu;
        let cpu_count = self.per_core.len();
        menu::submenu(ui, &strip_mnemonic(&i18n("Change _Top Graph To")), |ui| {
            for (label, value) in [
                (n_("Overall U_tilization"), GRAPH_SELECTION_OVERALL),
                (n_("Logical _Processors"), GRAPH_SELECTION_ALL),
                (n_("_All Threads"), GRAPH_SELECTION_ALL_THREADS),
                (
                    n_("All Threads _Stacked"),
                    GRAPH_SELECTION_ALL_THREADS_STACKED,
                ),
            ] {
                if menu::radio(
                    ui,
                    &strip_mnemonic(&i18n(label)),
                    cx.config.performance_page_cpu_graph == value,
                ) {
                    cx.config.performance_page_cpu_graph = value;
                    self.configure_thread_wise(value, cpu_count);
                }
            }
        });
        let current = self.bottom_selection(cx.config);
        menu::submenu(
            ui,
            &strip_mnemonic(&i18n("Change B_ottom Graph To")),
            |ui| {
                for (label, value, enabled) in [
                    (n_("_Power Draw"), GRAPH_POWER, self.has_power),
                    (n_("_Temperature"), GRAPH_TEMPERATURE, self.has_temperature),
                    (n_("_Clock Speed"), GRAPH_CLOCK, true),
                    (n_("_None"), GRAPH_NONE, true),
                ] {
                    if enabled && menu::radio(ui, &strip_mnemonic(&i18n(label)), current == value) {
                        cx.config.performance_page_cpu_graph_bottom = value;
                    }
                }
            },
        );
        if menu::check(
            ui,
            &i18n("Show Kernel Times"),
            cx.config.performance_page_kernel_times,
        ) {
            let kernel = !cx.config.performance_page_kernel_times;
            cx.config.performance_page_kernel_times = kernel;
            self.overall.set_data_visible(1, kernel);
            for g in self.per_core.iter_mut() {
                g.set_data_visible(1, kernel);
            }
        }
    }

    fn data_summary(&self) -> String {
        let l = &self.labels;
        let o = |v: &Option<String>| v.clone().unwrap_or_default();
        format!(
            r#"CPU

    {}

    Base speed:         {}
    Sockets:            {}
    Virtual processors: {}
    Virtualization:     {}
    Virtual machine:    {}
    L1 cache:           {}
    L2 cache:           {}
    L3 cache:           {}
    Cpufreq driver:     {}
    Cpufreq governor:   {}
    Power preference:   {}

    Utilization: {}
    Speed:       {}
    Processes:   {}
    Threads:     {}
    Handles:     {}
    Up time:     {}
    Temperature: {}"#,
            self.cpu_name,
            self.base_speed,
            self.sockets,
            self.virt_proc,
            self.virtualization,
            self.virt_machine,
            self.l1_cache,
            self.l2_cache,
            self.l3_cache,
            o(&l.cpufreq_driver),
            o(&l.cpufreq_governor),
            o(&l.power_preference),
            l.utilization,
            l.speed,
            l.processes,
            l.threads,
            l.handles,
            l.uptime,
            o(&l.temperature),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_count_matches_gtk() {
        assert_eq!(compute_column_count(1), 1);
        assert_eq!(compute_column_count(3), 3);
        assert_eq!(compute_column_count(4), 2);
        assert_eq!(compute_column_count(8), 4);
        assert_eq!(compute_column_count(12), 3);
        assert_eq!(compute_column_count(16), 4);
    }

    #[test]
    fn data_summary_format() {
        let mut r = Readings::new();
        r.cpu.name = Some("Test CPU".into());
        r.cpu.core_usage_percent = vec![10.; 4];
        r.cpu.core_kernel_usage_percent = vec![1.; 4];
        r.cpu.total_usage_percent = 42.4;
        let config = Config::default();
        let mut page = CpuPage::new(&r, &config);
        page.update(&r, &UpdateEnv::new(&config));
        let s = page.data_summary();
        assert!(s.starts_with("CPU\n\n    Test CPU\n"));
        assert!(s.contains("    Utilization: 42%\n"));
        assert!(s.contains("    Virtual processors: 4\n"));
        assert_eq!(page.summary.info1, "Test CPU");
        assert!(page.summary.info2.starts_with("42% "));
    }

    #[test]
    fn logical_processor_tooltips_follow_graph_ordinals_and_count() {
        let mut readings = mc_magpie::synthetic_readings(0, 0);
        let mut config = Config::default();
        let mut page = CpuPage::new(&readings, &config);
        for mode in [1, 2, 3, 4] {
            config.performance_page_cpu_graph = mode;
            let view = page.win10_view(&config);
            let tips = view.panels[0].tooltips;
            if mode == GRAPH_SELECTION_ALL {
                assert_eq!(tips.len(), view.panels[0].graphs.len());
                assert_eq!(tips[0], "CPU 0");
                assert_eq!(tips[1], "CPU 1");
                assert_eq!(tips.last().unwrap(), "CPU 7");
            } else {
                assert!(tips.is_empty());
            }
        }
        readings.cpu.core_usage_percent.truncate(2);
        readings.cpu.core_kernel_usage_percent.truncate(2);
        page.update(&readings, &UpdateEnv::new(&config));
        config.performance_page_cpu_graph = GRAPH_SELECTION_ALL;
        let view = page.win10_view(&config);
        assert_eq!(view.panels[0].graphs.len(), 2);
        assert_eq!(view.panels[0].tooltips, ["CPU 0", "CPU 1"]);
    }

    #[test]
    fn topology_uses_kernel_ids_and_hotplug_relabels_same_size_graphs() {
        let mut readings = mc_magpie::synthetic_readings(0, 0);
        readings.cpu.core_usage_percent = vec![10.; 2];
        readings.cpu.core_kernel_usage_percent = vec![1.; 2];
        readings.cpu.logical_cpus = [2, 7]
            .map(|id| mc_magpie::magpie_types::cpu::LogicalCpu {
                id,
                package_id: Some(0),
                core_id: Some(id),
                kind: Some("E".into()),
            })
            .to_vec();
        let config = Config::default();
        let mut page = CpuPage::new(&readings, &config);
        page.update(&readings, &UpdateEnv::new(&config));
        assert!(page.core_tooltips[0].starts_with("CPU 2 · E"));
        assert!(page.core_tooltips[1].starts_with("CPU 7 · E"));
        readings.cpu.logical_cpus[1].id = 9;
        readings.cpu.logical_cpus[1].kind = Some("LP-E".into());
        page.update(&readings, &UpdateEnv::new(&config));
        assert!(page.core_tooltips[1].starts_with("CPU 9 · LP-E"));
        assert_eq!(page.per_core.len(), 2);
    }
}
