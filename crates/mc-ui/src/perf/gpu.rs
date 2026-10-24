/* crates/mc-ui/src/perf/gpu.rs
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

use std::collections::HashMap;

use egui::Ui;
use mc_core::config::Config;
use mc_core::format::{DataType, to_human_readable_nice};
use mc_core::gpu::{GpuDevice, GpuSnapshot, engine_name, engine_note};
use mc_core::i18n::{i18n, i18n_f, n_, strip_mnemonic};
use mc_core::sidebar::DeviceType;
use mc_graph::{DEFAULT_DATA_POINTS, DatasetGroup, FillingSettings, Graph, ScalingSettings};
use mc_magpie::magpie_types::gpus::{ApiVersion, EngineKind, OpenGlVariant, TelemetryState};
use mc_magpie::{Gpu, Readings};

use super::cpu::{TEMPERATURE_HIGH_WATERMARK, TEMPERATURE_LOW_WATERMARK};
use super::{
    DevicePage, GraphPanel, GraphTile, PageView, SummaryGraph, UpdateEnv, fixed_group, over,
};
use crate::cx::Cx;
use crate::win10::menu;

const COLOR: egui::Color32 = crate::win10::palette::GPU.line;
const OVERALL: &str = "@overall";
const GRAPH_NONE: i32 = 0;
const GRAPH_POWER: i32 = 3;
const GRAPH_CLOCKS: i32 = 4;
const GRAPH_TEMPERATURE: i32 = 5;
const POWER: usize = 0;
const CLOCK_GPU: usize = 1;
const CLOCK_MEM: usize = 2;
const TEMPERATURE: usize = 3;

pub fn page_name(id: &str) -> String {
    format!("gpu-{id}")
}

fn graph(group: DatasetGroup) -> Graph {
    let mut graph = Graph::new();
    graph.set_base_color(COLOR);
    graph.add_dataset(group);
    graph
}

fn missing_group(high: f32) -> DatasetGroup {
    let mut group = DatasetGroup::new_with_fill(f32::NAN);
    group.dataset_settings = fixed_group(high).dataset_settings;
    group
}

fn memory_graph() -> Graph {
    let mut group = missing_group(1.);
    group.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
    graph(group)
}

fn aux_graph() -> Graph {
    let mut graph = Graph::new();
    graph.set_base_color(COLOR);
    for index in 0..4 {
        let mut group = missing_group(1.);
        group.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
        if index == CLOCK_MEM {
            group.dataset_settings.fill = FillingSettings::None;
            group.dataset_settings.dashed = true;
        }
        if index == TEMPERATURE {
            group.dataset_settings.scaling_settings = ScalingSettings::StickyUpDown;
            group.dataset_settings.low_watermark = TEMPERATURE_LOW_WATERMARK;
            group.dataset_settings.high_watermark = TEMPERATURE_HIGH_WATERMARK;
        }
        graph.add_dataset(group);
    }
    graph.connect_datasets(CLOCK_GPU, CLOCK_MEM);
    graph.connect_datasets(CLOCK_MEM, CLOCK_GPU);
    graph
}

fn state_name(state: TelemetryState) -> String {
    i18n(match state {
        TelemetryState::Unknown => n_("Unknown"),
        TelemetryState::Available => n_("Available"),
        TelemetryState::Partial => n_("Partial"),
        TelemetryState::Unsupported => n_("Unsupported"),
        TelemetryState::PermissionDenied => n_("Permission denied"),
        TelemetryState::Warmup => n_("Collecting data"),
        TelemetryState::Stale => n_("Stale"),
        TelemetryState::DriverUnavailable => n_("Driver unavailable"),
    })
}

fn measured(value: Option<f32>, state: TelemetryState) -> Option<f32> {
    matches!(state, TelemetryState::Available | TelemetryState::Partial)
        .then_some(value)
        .flatten()
        .filter(|value| value.is_finite())
}

fn percent(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.0}%"))
}

fn api_version(version: &Option<ApiVersion>) -> String {
    version.as_ref().map_or_else(
        || "—".into(),
        |version| match version.patch {
            Some(patch) => format!("{}.{}.{patch}", version.major, version.minor),
            None => format!("{}.{}", version.major, version.minor),
        },
    )
}

#[derive(Default)]
struct Support {
    power: bool,
    clocks: bool,
    temperature: bool,
}

pub struct GpuPage {
    name: String,
    gpu_id: String,
    index: usize,
    device_name: String,
    device: Option<GpuDevice>,
    last_sample: Option<(u64, u64)>,
    repeated: bool,
    histories: HashMap<String, DatasetGroup>,
    choices: Vec<(String, String)>,
    tile_cache: Vec<GraphTile>,
    slots: [Graph; 4],
    selected: Vec<String>,
    single: bool,
    history_dirty: bool,
    summary: SummaryGraph,
    dedicated: Graph,
    shared: Graph,
    middle: Graph,
    bottom: Graph,
    support: Support,
    max_duration: String,
    stats: Vec<Vec<(String, String)>>,
    info: Vec<(String, String)>,
}

impl GpuPage {
    pub fn new(gpu: &Gpu, config: &Config) -> Self {
        let device_name = gpu.device_name.clone().unwrap_or_else(|| i18n("Unknown"));
        let mut summary = SummaryGraph::percent(COLOR, i18n_f("GPU {}", &["0"]));
        // A new group starts at the minimum length; the sidebar graph shows 60 samples
        let mut group = missing_group(100.);
        group.update_data_points(DEFAULT_DATA_POINTS as usize);
        *summary.graph.groups_mut() = vec![group];
        summary.info1 = device_name.clone();
        Self {
            name: page_name(&gpu.id),
            gpu_id: gpu.id.clone(),
            index: 0,
            device_name,
            device: None,
            last_sample: None,
            repeated: false,
            histories: HashMap::new(),
            choices: vec![],
            tile_cache: vec![],
            slots: std::array::from_fn(|_| graph(missing_group(100.))),
            selected: vec![],
            single: false,
            history_dirty: true,
            summary,
            dedicated: memory_graph(),
            shared: memory_graph(),
            middle: aux_graph(),
            bottom: aux_graph(),
            support: Support::default(),
            max_duration: super::max_duration(config),
            stats: vec![],
            info: vec![],
        }
    }

    fn title(&self) -> String {
        i18n_f("GPU {}", &[&self.index.to_string()])
    }

    fn state(&self, state: TelemetryState) -> TelemetryState {
        if self.repeated && matches!(state, TelemetryState::Available | TelemetryState::Partial) {
            TelemetryState::Stale
        } else {
            state
        }
    }

    fn build_engine_choices(&self) -> Vec<(String, String)> {
        let Some(device) = &self.device else {
            return vec![];
        };
        let mut engines: Vec<_> = device.engines.iter().collect();
        engines.sort_by_key(|engine| {
            let priority = match engine.kind() {
                EngineKind::Render => 0,
                EngineKind::Copy => 1,
                EngineKind::Encode => 2,
                EngineKind::Decode => 3,
                // A shared encode/decode engine takes the video slot before Compute
                EngineKind::Video => 3,
                EngineKind::Compute => 4,
                EngineKind::VideoEnhance => 6,
                EngineKind::Other => 7,
            };
            (priority, engine_name(engine), engine.id.clone())
        });
        let mut choices: Vec<_> = engines
            .into_iter()
            .map(|engine| (engine.id.clone(), engine_name(engine)))
            .collect();
        choices.push((OVERALL.into(), i18n("Overall utilization")));
        choices
    }

    fn engine_choices(&self) -> Vec<(String, String)> {
        self.choices.clone()
    }

    fn refresh_selection(&mut self, config: &Config) {
        let choices = self.engine_choices();
        let defaults: Vec<_> = choices
            .iter()
            .filter(|(id, _)| id != OVERALL)
            .take(4)
            .map(|(id, _)| id.clone())
            .collect();
        let defaults = if defaults.is_empty() {
            vec![OVERALL.into()]
        } else {
            defaults
        };
        let settings = config.performance_page_gpu_graphs.get(&self.gpu_id);
        let single = settings.is_some_and(|settings| settings.single);
        let configured = settings.map(|settings| &settings.engines);
        let mut selected = configured
            .filter(|ids| !ids.is_empty())
            .cloned()
            .unwrap_or_else(|| defaults.clone());
        selected.truncate(4);
        for (slot, id) in selected.iter_mut().enumerate() {
            if !choices.iter().any(|(available, _)| available == id) {
                *id = defaults[slot % defaults.len()].clone();
            }
        }
        let tiles_dirty = self.selected != selected || self.history_dirty || self.single != single;
        self.single = single;
        if self.selected != selected || self.history_dirty {
            self.selected = selected;
            for (graph, id) in self.slots.iter_mut().zip(&self.selected) {
                let group = self.histories.get(id).cloned().unwrap_or_else(|| {
                    let mut group = missing_group(100.);
                    group.update_data_points(DEFAULT_DATA_POINTS as usize);
                    group
                });
                *graph.groups_mut() = vec![group];
            }
            self.history_dirty = false;
        }
        if tiles_dirty {
            self.tile_cache = self.build_tiles();
        }
    }

    fn engine_value(&self, id: &str) -> (Option<f32>, TelemetryState) {
        let Some(device) = &self.device else {
            return (None, TelemetryState::Unknown);
        };
        let (value, state) = if id == OVERALL {
            (device.usage.percent, device.usage.state)
        } else if let Some(engine) = device.engines.iter().find(|engine| engine.id == id) {
            (engine.usage_percent, engine.state())
        } else {
            (None, TelemetryState::Unsupported)
        };
        let state = self.state(state);
        (measured(value, state), state)
    }

    fn append_histories(&mut self) {
        let choices = self.engine_choices();
        self.histories
            .retain(|id, _| choices.iter().any(|(available, _)| id == available));
        for (id, _) in choices {
            let (value, _) = self.engine_value(&id);
            let history = self.histories.entry(id).or_insert_with(|| {
                let mut group = missing_group(100.);
                group.update_data_points(DEFAULT_DATA_POINTS as usize);
                group
            });
            history.add_data(&[value.unwrap_or(f32::NAN)]);
        }
        self.history_dirty = true;
    }

    fn effective(&self, value: i32) -> i32 {
        match value {
            GRAPH_POWER if self.support.power => value,
            GRAPH_CLOCKS if self.support.clocks => value,
            GRAPH_TEMPERATURE if self.support.temperature => value,
            // Legacy encode/decode and memory selections are already represented by the main panels.
            _ => GRAPH_NONE,
        }
    }

    fn configure_aux(graph: &mut Graph, selection: i32) {
        for (index, visible) in [
            (POWER, selection == GRAPH_POWER),
            (CLOCK_GPU, selection == GRAPH_CLOCKS),
            (CLOCK_MEM, selection == GRAPH_CLOCKS),
            (TEMPERATURE, selection == GRAPH_TEMPERATURE),
        ] {
            if graph.groups()[index].dataset_settings.visible != visible {
                graph.set_data_visible(index, visible);
            }
        }
    }

    fn aux_caption(&self, selection: i32) -> String {
        over(
            match selection {
                GRAPH_POWER => n_("Power draw over "),
                GRAPH_CLOCKS => n_("Clock speed over "),
                GRAPH_TEMPERATURE => n_("Temperature over "),
                _ => "",
            },
            &self.max_duration,
        )
    }

    fn aux_total(&self, graph: &Graph, selection: i32) -> String {
        match selection {
            GRAPH_POWER => self.human(graph.dataset_max_scale(POWER), DataType::Watts),
            GRAPH_CLOCKS => self.human(graph.dataset_max_scale(CLOCK_GPU), DataType::Hertz),
            GRAPH_TEMPERATURE => format!("{:.0} °C", graph.dataset_max_scale(TEMPERATURE)),
            _ => String::new(),
        }
    }

    fn human(&self, value: f32, kind: DataType) -> String {
        to_human_readable_nice(value, &kind)
    }

    fn bytes(&self, value: Option<u64>) -> String {
        value.map_or_else(
            || "—".into(),
            |value| self.human(value as f32, DataType::MemoryBytes),
        )
    }

    fn memory_pair(&self, used: Option<u64>, total: Option<u64>) -> String {
        format!("{} / {}", self.bytes(used), self.bytes(total))
    }

    fn aux_menu(&self, ui: &mut Ui, cx: &mut Cx, middle: bool) {
        let current = if middle {
            cx.config.performance_page_gpu_graph_middle
        } else {
            cx.config.performance_page_gpu_graph_bottom
        };
        let other = if middle {
            cx.config.performance_page_gpu_graph_bottom
        } else {
            cx.config.performance_page_gpu_graph_middle
        };
        let title = if middle {
            n_("Change Middle Graph To")
        } else {
            n_("Change Bottom Graph To")
        };
        menu::submenu(ui, &i18n(title), |ui| {
            for (label, value, supported) in [
                (n_("_Power Draw"), GRAPH_POWER, self.support.power),
                (n_("_Clock Speeds"), GRAPH_CLOCKS, self.support.clocks),
                (
                    n_("_Temperature"),
                    GRAPH_TEMPERATURE,
                    self.support.temperature,
                ),
                (n_("_None"), GRAPH_NONE, true),
            ] {
                if supported
                    && (value == GRAPH_NONE || value != self.effective(other))
                    && menu::radio(
                        ui,
                        &strip_mnemonic(&i18n(label)),
                        self.effective(current) == value,
                    )
                {
                    if middle {
                        cx.config.performance_page_gpu_graph_middle = value;
                    } else {
                        cx.config.performance_page_gpu_graph_bottom = value;
                    }
                }
            }
        });
    }
}

impl DevicePage for GpuPage {
    fn name(&self) -> &str {
        &self.name
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Gpu
    }
    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }
    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }

    fn update_gpu(&mut self, snapshot: &GpuSnapshot) {
        self.device = snapshot.devices.get(&self.gpu_id).cloned();
        self.choices = self.build_engine_choices();
        if let Some(device) = &self.device {
            self.index = device.index;
            self.summary.heading = self.title();
        }
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let Some(gpu) = readings.gpus.values().find(|gpu| gpu.id == self.gpu_id) else {
            return false;
        };
        let Some(device) = self.device.clone() else {
            return false;
        };
        self.max_duration = env.max_duration.clone();
        self.device_name = gpu.device_name.clone().unwrap_or_else(|| i18n("Unknown"));
        let sample = (device.epoch, device.sequence);
        // Legacy fixture fields have no hardware sample identity. Their intake is
        // still a new (partial) replay observation, not a repeated daemon sample.
        self.repeated = sample != (0, 0) && self.last_sample == Some(sample);
        if self
            .last_sample
            .is_some_and(|(epoch, _)| epoch != device.epoch)
        {
            self.histories.clear();
            for graph in [
                &mut self.dedicated,
                &mut self.shared,
                &mut self.middle,
                &mut self.bottom,
                &mut self.summary.graph,
            ] {
                for group in graph.groups_mut() {
                    let settings = group.dataset_settings.clone();
                    *group = DatasetGroup::new_with_fill(f32::NAN);
                    group.dataset_settings = settings;
                    group.update_data_points(DEFAULT_DATA_POINTS as usize);
                }
            }
        }
        self.last_sample = Some(sample);
        self.append_histories();
        self.refresh_selection(env.config);
        let (utilization, state) = self.engine_value(OVERALL);
        self.summary
            .graph
            .add_single_data_point(0, vec![utilization.unwrap_or(f32::NAN)]);
        self.summary.info1 = self.device_name.clone();
        let fresh = !self.repeated && state != TelemetryState::Stale;
        let sensor = |value: Option<f32>| value.filter(|value| fresh && value.is_finite());
        let temperature = sensor(gpu.temperature_c);
        self.summary.info2 = percent(utilization);
        if let Some(temperature) = temperature {
            self.summary
                .info2
                .push_str(&format!(" ({temperature:.0} °C)"));
        }
        if state != TelemetryState::Available {
            self.summary
                .info2
                .push_str(&format!(" · {}", state_name(state)));
        }

        let memory = &device.memory;
        let dedicated_state = self.state(memory.dedicated_state());
        let shared_state = self.state(memory.shared_state());
        let memory_used = |value, state| {
            if matches!(state, TelemetryState::Available | TelemetryState::Partial) {
                value
            } else {
                None
            }
        };
        let dedicated_used = memory_used(memory.dedicated_used_bytes, dedicated_state);
        let shared_used = memory_used(memory.shared_used_bytes, shared_state);
        for (graph, used, total) in [
            (
                &mut self.dedicated,
                dedicated_used,
                memory.dedicated_total_bytes,
            ),
            (&mut self.shared, shared_used, memory.shared_total_bytes),
        ] {
            let scaling = if total.is_some() {
                ScalingSettings::Fixed
            } else {
                ScalingSettings::StickyUp
            };
            if graph.groups()[0].dataset_settings.scaling_settings != scaling {
                graph.set_dataset_scaling(0, scaling);
            }
            if let Some(total) = total {
                let scale = (total as f32).max(1.);
                if graph.dataset_max_scale(0) != scale {
                    graph.set_dataset_max_scale(0, scale);
                }
            }
            graph.add_single_data_point(0, vec![used.map_or(f32::NAN, |value| value as f32)]);
        }

        self.support = Support {
            power: gpu.power_draw_watts.is_some(),
            clocks: gpu.clock_speed_mhz.is_some() || gpu.memory_speed_mhz.is_some(),
            temperature: gpu.temperature_c.is_some(),
        };
        let power = sensor(gpu.power_draw_watts);
        let clock = sensor(gpu.clock_speed_mhz.map(|value| value as f32 * 1e6));
        let memory_clock = sensor(gpu.memory_speed_mhz.map(|value| value as f32 * 1e6));
        for graph in [&mut self.middle, &mut self.bottom] {
            graph.add_data_point(vec![
                vec![power.unwrap_or(f32::NAN)],
                vec![clock.unwrap_or(f32::NAN)],
                vec![memory_clock.unwrap_or(f32::NAN)],
                vec![temperature.unwrap_or(f32::NAN)],
            ]);
        }
        let total_used = dedicated_used
            .zip(shared_used)
            .and_then(|(dedicated, shared)| dedicated.checked_add(shared));
        let total_capacity = memory
            .dedicated_total_bytes
            .zip(memory.shared_total_bytes)
            .and_then(|(dedicated, shared)| dedicated.checked_add(shared));
        self.stats = vec![
            vec![
                (i18n("Utilization"), percent(utilization)),
                (
                    i18n("Temperature"),
                    temperature.map_or_else(|| "—".into(), |value| format!("{value:.0} °C")),
                ),
            ],
            vec![(
                i18n("Dedicated GPU memory"),
                self.memory_pair(dedicated_used, memory.dedicated_total_bytes),
            )],
            vec![
                (
                    i18n("Shared GPU memory"),
                    self.memory_pair(shared_used, memory.shared_total_bytes),
                ),
                (
                    i18n("GPU memory"),
                    self.memory_pair(total_used, total_capacity),
                ),
            ],
        ];
        let mut aux_stats = vec![];
        for (label, value, kind) in [
            (n_("Clock Speed"), clock, DataType::Hertz),
            (n_("Memory Speed"), memory_clock, DataType::Hertz),
            ("Power Draw", power, DataType::Watts),
        ] {
            if let Some(value) = value {
                aux_stats.push((i18n(label), self.human(value, kind)));
            }
        }
        if !aux_stats.is_empty() {
            self.stats.push(aux_stats);
        }
        self.info = vec![
            (i18n("Status:"), state_name(state)),
            (
                i18n("Dedicated memory status:"),
                state_name(dedicated_state),
            ),
            (i18n("Shared memory status:"), state_name(shared_state)),
            (i18n("Sample age:"), format!("{} ms", device.age_ms)),
        ];
        if let Some(voltage) = gpu.voltage_v {
            self.info
                .push((i18n("GPU voltage:"), format!("{voltage:.3} V")));
        }
        if let Some(engine) = &device.usage.engine {
            self.info
                .push((i18n("Busiest engine:"), engine.name.clone()));
        }
        for (label, value) in [
            ("Kernel driver:", &device.driver.kernel_driver),
            ("Driver version:", &device.driver.version),
            ("Driver date:", &device.driver.date),
            ("Driver model:", &device.driver.model),
            ("Vulkan driver:", &device.driver.vulkan_driver),
        ] {
            if let Some(value) = value.as_ref().filter(|value| !value.is_empty()) {
                self.info.push((i18n(label), value.clone()));
            }
        }
        let opengl = api_version(&gpu.opengl_version);
        self.info.push((
            i18n("OpenGL version:"),
            if gpu.opengl_variant() == OpenGlVariant::OpenGles && gpu.opengl_version.is_some() {
                format!("ES {opengl}")
            } else {
                opengl
            },
        ));
        self.info
            .push((i18n("Vulkan version:"), api_version(&gpu.vulkan_version)));
        for (label, version) in [
            ("OpenCL version:", &gpu.opencl_version),
            ("CUDA version:", &gpu.cuda_version),
            ("DirectX version:", &gpu.directx_version),
            ("Metal version:", &gpu.metal_version),
        ] {
            if version.is_some() {
                self.info.push((i18n(label), api_version(version)));
            }
        }
        if let (Some(generation), Some(lanes)) = (gpu.pcie_gen, gpu.pcie_lanes) {
            self.info.push((
                i18n("PCI Express speed:"),
                format!("PCIe Gen {generation} x{lanes}"),
            ));
        }
        if let (Some(generation), Some(lanes)) = (gpu.max_pcie_gen, gpu.max_pcie_lanes) {
            self.info.push((
                i18n("Max PCI Express speed:"),
                format!("PCIe Gen {generation} x{lanes}"),
            ));
        }
        self.info
            .push((i18n("Device address:"), self.gpu_id.clone()));
        if let Some(reserved) = memory.reserved_bytes {
            self.info
                .push((i18n("Driver-reserved memory:"), self.bytes(Some(reserved))));
        }
        if let Some(integrated) = device.driver.integrated {
            self.info.push((
                i18n("Integrated GPU:"),
                i18n(if integrated { "Yes" } else { "No" }),
            ));
        }
        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        let mut repaint = false;
        for graph in self.slots.iter_mut().chain([
            &mut self.dedicated,
            &mut self.shared,
            &mut self.middle,
            &mut self.bottom,
        ]) {
            repaint |= graph.set_grid_offset(offset);
        }
        repaint
    }

    fn win10_graph_tiles(&mut self, config: &Config) -> Vec<GraphTile> {
        self.refresh_selection(config);
        self.tile_cache.clone()
    }

    fn win10_view(&mut self, config: &Config) -> PageView<'_> {
        self.refresh_selection(config);
        let middle = self.effective(config.performance_page_gpu_graph_middle);
        let bottom = self.effective(config.performance_page_gpu_graph_bottom);
        Self::configure_aux(&mut self.middle, middle);
        Self::configure_aux(&mut self.bottom, bottom);
        let middle_caption = self.aux_caption(middle);
        let middle_total = self.aux_total(&self.middle, middle);
        let bottom_caption = self.aux_caption(bottom);
        let bottom_total = self.aux_total(&self.bottom, bottom);
        let dedicated_total = self.bytes(
            self.device
                .as_ref()
                .and_then(|device| device.memory.dedicated_total_bytes),
        );
        let shared_total = self.bytes(
            self.device
                .as_ref()
                .and_then(|device| device.memory.shared_total_bytes),
        );
        let title = self.title();
        let count = if self.single { 1 } else { self.selected.len() }.clamp(1, 4);
        let mut panels = vec![
            GraphPanel::grid(
                over(n_("Utilization over "), &self.max_duration),
                String::new(),
                self.slots.iter_mut().take(count).collect(),
                if count > 1 { 2 } else { 1 },
            ),
            GraphPanel::single(
                i18n("Dedicated GPU memory usage"),
                dedicated_total,
                &mut self.dedicated,
            ),
            GraphPanel::single(
                i18n("Shared GPU memory usage"),
                shared_total,
                &mut self.shared,
            ),
        ];
        if middle != GRAPH_NONE {
            panels.push(GraphPanel::single(
                middle_caption,
                middle_total,
                &mut self.middle,
            ));
        }
        if bottom != GRAPH_NONE {
            panels.push(GraphPanel::single(
                bottom_caption,
                bottom_total,
                &mut self.bottom,
            ));
        }
        PageView {
            model_tooltip: None,
            title,
            model: self.device_name.clone(),
            panels,
            stats: self.stats.clone(),
            info: self.info.clone(),
            composition: None,
            swap_compression: None,
        }
    }

    fn win10_menu(&mut self, ui: &mut Ui, cx: &mut Cx) {
        menu::submenu(ui, &i18n("Change graph to"), |ui| {
            let settings = cx
                .config
                .performance_page_gpu_graphs
                .entry(self.gpu_id.clone())
                .or_default();
            if menu::radio(ui, &i18n("Multiple engines"), !settings.single) {
                settings.single = false;
            }
            if menu::radio(ui, &i18n("Single engine"), settings.single) {
                settings.single = true;
            }
        });
        for slot in 0..self.selected.len().min(if self.single { 1 } else { 4 }) {
            menu::submenu(
                ui,
                &i18n_f("Engine graph {}", &[&(slot + 1).to_string()]),
                |ui| self.win10_graph_menu(ui, cx, slot),
            );
        }
        self.aux_menu(ui, cx, true);
        self.aux_menu(ui, cx, false);
    }

    fn win10_graph_menu(&mut self, ui: &mut Ui, cx: &mut Cx, slot: usize) {
        for (id, label) in self.engine_choices() {
            if menu::radio(ui, &label, self.selected.get(slot) == Some(&id)) {
                let settings = cx
                    .config
                    .performance_page_gpu_graphs
                    .entry(self.gpu_id.clone())
                    .or_default();
                settings.engines = self.selected.clone();
                if slot < settings.engines.len() {
                    settings.engines[slot] = id;
                }
            }
        }
    }

    fn data_summary(&self) -> String {
        let mut text = format!("{}\n\n    {}\n", self.title(), self.device_name);
        for row in &self.stats {
            for (label, value) in row {
                text.push_str(&format!("\n    {label}: {value}"));
            }
        }
        text.push('\n');
        for (id, label) in self.engine_choices() {
            let (value, state) = self.engine_value(&id);
            text.push_str(&format!(
                "\n    {label}: {} ({})",
                percent(value),
                state_name(state)
            ));
        }
        text.push('\n');
        for (label, value) in &self.info {
            text.push_str(&format!("\n    {label} {value}"));
        }
        text
    }
}

impl GpuPage {
    fn build_tiles(&self) -> Vec<GraphTile> {
        let choices = self.engine_choices();
        self.selected
            .iter()
            .take(if self.single { 1 } else { 4 })
            .enumerate()
            .map(|(slot, id)| {
                let caption = choices
                    .iter()
                    .find(|(available, _)| available == id)
                    .map_or_else(|| i18n("Unknown"), |(_, name)| name.clone());
                let (value, state) = self.engine_value(id);
                let mut tooltip = format!("{caption}: {}\n{}", percent(value), state_name(state));
                if let Some(engine) = self
                    .device
                    .as_ref()
                    .and_then(|device| device.engines.iter().find(|engine| engine.id == *id))
                {
                    if let Some(note) = engine_note(engine) {
                        tooltip.push_str(&format!("\n{note}"));
                    }
                    if engine.client_derived == Some(true) {
                        tooltip
                            .push_str(&format!("\n{}", i18n("Measured from visible DRM clients")));
                    }
                }
                GraphTile {
                    caption,
                    right: format!("{} · 100%", percent(value)),
                    tooltip,
                    slot,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::gpu::GpuUsage;
    use mc_magpie::magpie_types::gpus::{DriverInfo, EngineTelemetry, MemoryTelemetry};

    fn setup() -> (GpuPage, Config, Readings, GpuSnapshot) {
        let config = Config::default();
        let gpu = Gpu {
            id: "test-adapter".into(),
            device_name: Some("Test GPU".into()),
            utilization_percent: Some(99.),
            ..Gpu::default()
        };
        let page = GpuPage::new(&gpu, &config);
        let readings = Readings {
            gpus: HashMap::from([(gpu.id.clone(), gpu)]),
            ..Readings::default()
        };
        let device = GpuDevice {
            index: 0,
            usage: GpuUsage {
                percent: Some(37.),
                engine: None,
                state: TelemetryState::Available,
            },
            engines: vec![
                EngineTelemetry {
                    id: "render".into(),
                    name: "render".into(),
                    kind: EngineKind::Render as i32,
                    usage_percent: Some(80.),
                    state: TelemetryState::Available as i32,
                    client_derived: Some(false),
                },
                EngineTelemetry {
                    id: "copy".into(),
                    name: "copy".into(),
                    kind: EngineKind::Copy as i32,
                    usage_percent: Some(40.),
                    state: TelemetryState::Partial as i32,
                    client_derived: Some(true),
                },
            ],
            memory: MemoryTelemetry {
                dedicated_used_bytes: Some(1024),
                dedicated_total_bytes: Some(8192),
                shared_used_bytes: Some(2048),
                shared_total_bytes: None,
                dedicated_state: TelemetryState::Available as i32,
                shared_state: TelemetryState::Partial as i32,
                ..MemoryTelemetry::default()
            },
            driver: DriverInfo {
                kernel_driver: Some("test-kernel-driver".into()),
                version: Some("test-version".into()),
                ..DriverInfo::default()
            },
            epoch: 1,
            sequence: 1,
            age_ms: 0,
        };
        let mut snapshot = GpuSnapshot::default();
        snapshot.devices.insert("test-adapter".into(), device);
        (page, config, readings, snapshot)
    }

    fn feed(page: &mut GpuPage, config: &Config, readings: &Readings, snapshot: &GpuSnapshot) {
        page.update_gpu(snapshot);
        assert!(page.update(readings, &UpdateEnv::new(config)));
    }

    #[test]
    fn normalized_usage_and_unknown_capacity_are_preserved() {
        let (mut page, config, readings, snapshot) = setup();
        let values = page.summary.graph.groups()[0].datas[0].get_data();
        assert_eq!(values.len(), DEFAULT_DATA_POINTS as usize);
        assert!(values.iter().all(|value| value.is_nan()));
        feed(&mut page, &config, &readings, &snapshot);
        assert_eq!(page.summary.heading, i18n_f("GPU {}", &["0"]));
        let values = page.summary.graph.groups()[0].datas[0].get_data();
        assert_eq!(values.len(), DEFAULT_DATA_POINTS as usize);
        assert_eq!(values[0], 37.);
        assert!(values[1..].iter().all(|value| value.is_nan()));
        let view = page.win10_view(&config);
        assert_eq!(view.panels.len(), 3); // The legacy 1/2 defaults do not duplicate the main graphs.
        assert_eq!(view.panels[2].right, "—");
        assert!(view.stats[2][1].1.ends_with(" / —"));
        let copy = page.data_summary();
        assert!(copy.contains("37%"));
        assert!(copy.contains("test-kernel-driver"));
        assert!(!copy.contains("WDDM"));
    }

    #[test]
    fn repeated_sample_creates_gaps_and_epoch_restart_clears_history() {
        let (mut page, config, readings, mut snapshot) = setup();
        feed(&mut page, &config, &readings, &snapshot);
        feed(&mut page, &config, &readings, &snapshot);
        let values = page.histories["render"].datas[0].get_data();
        assert!(values[0].is_nan());
        assert_eq!(values[1], 80.);
        assert!(page.dedicated.groups()[0].datas[0].get_data()[0].is_nan());
        assert_eq!(page.engine_value("render").1, TelemetryState::Stale);
        let device = snapshot.devices.get_mut("test-adapter").unwrap();
        device.epoch = 2;
        device.engines[0].usage_percent = Some(12.);
        feed(&mut page, &config, &readings, &snapshot);
        let values = page.histories["render"].datas[0].get_data();
        assert_eq!(values[0], 12.);
        assert!(values[1].is_nan());
        assert!(!values.contains(&80.));
    }

    #[test]
    fn duplicate_slots_share_bounded_history_without_idle_rebuilds() {
        let (mut page, mut config, readings, mut snapshot) = setup();
        config
            .performance_page_gpu_graphs
            .entry("test-adapter".into())
            .or_default()
            .engines = vec![
            "copy".into(),
            "copy".into(),
            "render".into(),
            OVERALL.into(),
        ];
        for sequence in 1..DEFAULT_DATA_POINTS as u64 + 5 {
            snapshot.devices.get_mut("test-adapter").unwrap().sequence = sequence;
            feed(&mut page, &config, &readings, &snapshot);
        }
        assert_eq!(
            page.histories["copy"].datas[0].get_data().len(),
            DEFAULT_DATA_POINTS as usize
        );
        assert_eq!(
            page.slots[0].groups()[0].datas[0].get_data(),
            page.slots[1].groups()[0].datas[0].get_data()
        );
        let generations: Vec<_> = page.slots.iter().map(Graph::generation).collect();
        for _ in 0..3 {
            let tiles = page.win10_graph_tiles(&config);
            assert_eq!(tiles.len(), 4);
            assert_eq!(page.win10_view(&config).panels[0].columns, 2);
        }
        assert_eq!(
            generations,
            page.slots.iter().map(Graph::generation).collect::<Vec<_>>()
        );
        config
            .performance_page_gpu_graphs
            .get_mut("test-adapter")
            .unwrap()
            .single = true;
        assert_eq!(page.win10_graph_tiles(&config).len(), 1);
        assert_eq!(page.win10_view(&config).panels[0].graphs.len(), 1);
    }

    #[test]
    fn unified_video_engine_fills_the_slot_before_compute() {
        let (mut page, config, readings, mut snapshot) = setup();
        let engine = |id: &str, name: &str, kind: EngineKind| EngineTelemetry {
            id: id.into(),
            name: name.into(),
            kind: kind as i32,
            usage_percent: Some(0.),
            state: TelemetryState::Available as i32,
            client_derived: Some(true),
        };
        snapshot.devices.get_mut("test-adapter").unwrap().engines = vec![
            engine("compute", "Compute", EngineKind::Compute),
            engine("enc", "Video Encode/Decode", EngineKind::Video),
            engine("gfx", "3D", EngineKind::Render),
            engine("dma", "Copy", EngineKind::Copy),
        ];
        feed(&mut page, &config, &readings, &snapshot);
        assert_eq!(page.selected, ["gfx", "dma", "enc", "compute"]);
        let tiles = page.win10_graph_tiles(&config);
        assert_eq!(tiles[2].caption, i18n("Video Encode/Decode"));
        assert!(tiles[2].tooltip.contains(&i18n(
            "Encoding and decoding share one hardware queue; the driver does not count them separately"
        )));
        assert!(!tiles[3].tooltip.contains("hardware queue"));
    }

    #[test]
    fn overall_fallback_is_not_a_fabricated_engine() {
        let (mut page, config, readings, mut snapshot) = setup();
        let device = snapshot.devices.get_mut("test-adapter").unwrap();
        device.engines.clear();
        device.usage.percent = None;
        device.usage.state = TelemetryState::PermissionDenied;
        feed(&mut page, &config, &readings, &snapshot);
        let tiles = page.win10_graph_tiles(&config);
        assert_eq!(page.selected, [OVERALL]);
        assert!(tiles[0].caption.starts_with(&i18n("Overall utilization")));
        assert!(tiles[0].right.starts_with('—'));
        assert!(page.histories[OVERALL].datas[0].get_data()[0].is_nan());
        assert_eq!(
            page.engine_value(OVERALL).1,
            TelemetryState::PermissionDenied
        );
    }
}
