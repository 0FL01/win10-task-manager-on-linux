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

use super::{GraphPanel, PageView, over};
use egui::Ui;
use mc_core::config::Config;
use mc_core::format::{DataType, UnitSettings, to_human_readable_nice};
use mc_core::i18n::n_;
use mc_core::i18n::{i18n, i18n_f, strip_mnemonic};
use mc_core::sidebar::DeviceType;
use mc_graph::{
    AnimationFrame, DatasetGroup, FillingSettings, Graph, GraphSettings, RoundingSettings,
    ScalingSettings,
};
use mc_magpie::magpie_types::gpus::OpenGlVariant;
use mc_magpie::{Gpu, Readings};

use super::cpu::{TEMPERATURE_HIGH_WATERMARK, TEMPERATURE_LOW_WATERMARK};
use super::{DevicePage, SummaryGraph, UpdateEnv};
use crate::cx::Cx;
const COLOR: egui::Color32 = crate::win10::palette::GPU.line;

const GRAPH_NONE: i32 = 0;
const GRAPH_ENCODE_DECODE: i32 = 1;
const GRAPH_MEMORY: i32 = 2;
const GRAPH_POWER: i32 = 3;
const GRAPH_CLOCKS: i32 = 4;
const GRAPH_TEMPERATURE: i32 = 5;

const ENCODE: usize = 0;
const DECODE: usize = 1;
const VRAM: usize = 2;
const GTT: usize = 3;
const POWER: usize = 4;
const CLOCK_GPU: usize = 5;
const CLOCK_MEM: usize = 6;
const TEMPERATURE: usize = 7;

pub fn page_name(id: &str) -> String {
    format!("gpu-{id}")
}

fn aux_graph() -> Graph {
    let mut g = Graph::new();
    g.set_base_color(COLOR);
    let mut encode = DatasetGroup::new();
    encode.dataset_settings.fill = FillingSettings::None;
    encode.dataset_settings.dashed = true;
    let mut gtt = DatasetGroup::new();
    gtt.dataset_settings.fill = FillingSettings::None;
    gtt.dataset_settings.dashed = true;
    let mut power = DatasetGroup::new();
    power.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
    power.dataset_settings.high_watermark = 0.;
    power.dataset_settings.rounding_settings = RoundingSettings::Integer;
    let mut clock = DatasetGroup::new();
    clock.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
    clock.dataset_settings.high_watermark = 0.;
    let mut clock_mem = DatasetGroup::new();
    clock_mem.dataset_settings.fill = FillingSettings::None;
    clock_mem.dataset_settings.dashed = true;
    clock_mem.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
    clock_mem.dataset_settings.high_watermark = 0.;
    let mut temp = DatasetGroup::new();
    temp.dataset_settings.scaling_settings = ScalingSettings::StickyUpDown;
    temp.dataset_settings.high_watermark = TEMPERATURE_HIGH_WATERMARK;
    temp.dataset_settings.low_watermark = TEMPERATURE_LOW_WATERMARK;
    for d in [
        encode,
        DatasetGroup::new(),
        DatasetGroup::new(),
        gtt,
        power,
        clock,
        clock_mem,
        temp,
    ] {
        g.add_dataset(d);
    }
    g.connect_datasets(CLOCK_GPU, CLOCK_MEM);
    g.connect_datasets(CLOCK_MEM, CLOCK_GPU);
    g
}

#[derive(Default, Clone, Copy)]
struct Support {
    enc_dec: bool,
    memory: bool,
    power: bool,
    clocks: bool,
    temp: bool,
}

impl Support {
    fn has(&self, graph: i32) -> bool {
        match graph {
            GRAPH_NONE => true,
            GRAPH_ENCODE_DECODE => self.enc_dec,
            GRAPH_MEMORY => self.memory,
            GRAPH_POWER => self.power,
            GRAPH_CLOCKS => self.clocks,
            GRAPH_TEMPERATURE => self.temp,
            _ => false,
        }
    }
}

#[derive(Default)]
struct Labels {
    utilization: String,
    clock_current: Option<String>,
    clock_max: Option<String>,
    power_current: Option<String>,
    power_max: Option<String>,
    memory_title: String,
    memory_current: Option<String>,
    memory_max: Option<String>,
    gtt_title: String,
    gtt_current: Option<String>,
    gtt_max: Option<String>,
    mem_speed_current: Option<String>,
    mem_speed_max: Option<String>,
    encode: Option<String>,
    decode: Option<String>,
    temperature: Option<String>,
    pcie: Option<String>,
    max_pcie: Option<String>,
}

pub struct GpuPage {
    name: String,
    gpu_id: String,
    index: Option<usize>,
    summary: SummaryGraph,
    utilization: Graph,
    middle: Graph,
    bottom: Graph,
    device_name: String,
    support: Support,
    encode_decode_shared: bool,
    gtt_available: bool,
    opengl: String,
    vulkan: String,
    labels: Labels,
    units: UnitSettings,
    max_duration: String,
}

impl GpuPage {
    pub fn new(gpu: &Gpu, config: &Config) -> Self {
        let mut summary = SummaryGraph::percent(COLOR, i18n("GPU"));
        summary.info1 = gpu.device_name.clone().unwrap_or_else(|| i18n("Unknown"));

        let mut utilization = Graph::new();
        utilization.set_base_color(COLOR);
        utilization.add_dataset(DatasetGroup::new());

        let opengl = match &gpu.opengl_version {
            Some(v) => {
                let prefix = if gpu.opengl_variant() == OpenGlVariant::OpenGles {
                    "ES "
                } else {
                    ""
                };
                format!("{prefix}{}.{}", v.major, v.minor)
            }
            None => i18n("Unknown"),
        };
        let vulkan = match &gpu.vulkan_version {
            Some(v) => format!("{}.{}.{}", v.major, v.minor, v.patch.unwrap_or(0)),
            None => i18n("Unsupported"),
        };

        let mut page = Self {
            name: page_name(&gpu.id),
            gpu_id: gpu.id.clone(),
            index: None,
            summary,
            utilization,
            middle: aux_graph(),
            bottom: aux_graph(),
            device_name: gpu.device_name.clone().unwrap_or_else(|| i18n("Unknown")),
            support: Support::default(),
            encode_decode_shared: gpu.encode_decode_shared,
            gtt_available: gpu.total_shared_memory.is_some(),
            opengl,
            vulkan,
            labels: Labels::default(),
            units: config.units(),
            max_duration: super::max_duration(config),
        };
        page.configure_support(gpu);
        page
    }

    fn configure_support(&mut self, gpu: &Gpu) {
        self.support = Support {
            enc_dec: gpu.encoder_percent.is_some() || gpu.decoder_percent.is_some(),
            memory: gpu.total_memory.is_some()
                || gpu.used_memory.is_some()
                || gpu.total_shared_memory.is_some()
                || gpu.used_shared_memory.is_some(),
            power: gpu.power_draw_watts.is_some(),
            clocks: gpu.clock_speed_mhz.is_some() || gpu.memory_speed_mhz.is_some(),
            temp: gpu.temperature_c.is_some(),
        };
        for g in [&mut self.middle, &mut self.bottom] {
            if self.encode_decode_shared {
                g.set_filled(ENCODE, FillingSettings::FillToBottom);
                g.set_dashed(ENCODE, false);
            }
            if let Some(t) = gpu.total_memory {
                g.set_dataset_max_scale(VRAM, t as f32);
            }
            if let Some(t) = gpu.total_shared_memory {
                g.set_dataset_max_scale(GTT, t as f32);
            }
            if let Some(p) = gpu.max_power_draw_watts {
                g.set_dataset_max_scale(POWER, p);
            }
            if let Some(c) = gpu.max_clock_speed_mhz {
                g.set_dataset_max_scale(CLOCK_GPU, c as f32 * 1e6);
            }
            if let Some(c) = gpu.max_memory_speed_mhz {
                g.set_dataset_max_scale(CLOCK_MEM, c as f32 * 1e6);
            }
        }
    }

    fn effective(&self, value: i32) -> i32 {
        if self.support.has(value) {
            value
        } else {
            GRAPH_NONE
        }
    }

    fn configure_graph(graph: &mut Graph, num: i32, gtt_available: bool) {
        graph.set_data_visible(ENCODE, num == GRAPH_ENCODE_DECODE);
        graph.set_data_visible(DECODE, num == GRAPH_ENCODE_DECODE);
        graph.set_data_visible(VRAM, num == GRAPH_MEMORY);
        graph.set_data_visible(GTT, gtt_available && num == GRAPH_MEMORY);
        graph.set_data_visible(POWER, num == GRAPH_POWER);
        graph.set_data_visible(CLOCK_GPU, num == GRAPH_CLOCKS);
        graph.set_data_visible(CLOCK_MEM, num == GRAPH_CLOCKS);
        graph.set_data_visible(TEMPERATURE, num == GRAPH_TEMPERATURE);
    }

    fn caption_for(&self, num: i32) -> String {
        match num {
            GRAPH_ENCODE_DECODE => i18n("Video encode/decode utilization over "),
            GRAPH_MEMORY if self.gtt_available => i18n("Dedicated and shared memory usage over "),
            GRAPH_MEMORY => i18n("Memory usage over "),
            GRAPH_POWER => i18n("Power draw over "),
            GRAPH_CLOCKS => i18n("Clock speed over "),
            GRAPH_TEMPERATURE => i18n("Temperature over "),
            _ => String::new(),
        }
    }

    fn total_for(&self, graph: &Graph, num: i32) -> String {
        let u = &self.units;
        match num {
            GRAPH_ENCODE_DECODE => i18n("100%"),
            GRAPH_MEMORY => {
                let vram = to_human_readable_nice(
                    graph.dataset_max_scale(VRAM),
                    &DataType::MemoryBytes,
                    u,
                );
                if self.gtt_available {
                    let gtt = to_human_readable_nice(
                        graph.dataset_max_scale(GTT),
                        &DataType::MemoryBytes,
                        u,
                    );
                    format!("{vram} / {gtt}")
                } else {
                    vram
                }
            }
            GRAPH_POWER => {
                to_human_readable_nice(graph.dataset_max_scale(POWER), &DataType::Watts, u)
            }
            GRAPH_CLOCKS => {
                to_human_readable_nice(graph.dataset_max_scale(CLOCK_GPU), &DataType::Hertz, u)
            }
            GRAPH_TEMPERATURE => format!("{:.0} °C", graph.dataset_max_scale(TEMPERATURE)),
            _ => String::new(),
        }
    }

    fn title(&self) -> String {
        match self.index {
            Some(i) => i18n_f("GPU {}", &[&i.to_string()]),
            None => i18n("GPU"),
        }
    }

    fn graph_menu(&self, ui: &mut Ui, cx: &mut Cx, middle: bool) {
        let (title, current, other) = if middle {
            (
                n_("Change Middle Graph To"),
                self.effective(cx.config.performance_page_gpu_graph_middle),
                self.effective(cx.config.performance_page_gpu_graph_bottom),
            )
        } else {
            (
                n_("Change Bottom Graph To"),
                self.effective(cx.config.performance_page_gpu_graph_bottom),
                self.effective(cx.config.performance_page_gpu_graph_middle),
            )
        };
        crate::win10::menu::submenu(ui, &i18n(title), |ui| {
            for (label, value) in [
                (n_("_Encode/_Decode"), GRAPH_ENCODE_DECODE),
                (n_("_Memory Usage"), GRAPH_MEMORY),
                (n_("_Power Draw"), GRAPH_POWER),
                (n_("_Clock Speeds"), GRAPH_CLOCKS),
                (n_("_Temperature"), GRAPH_TEMPERATURE),
                (n_("_None"), GRAPH_NONE),
            ] {
                let enabled = self.support.has(value) && (value == GRAPH_NONE || value != other);
                if enabled
                    && crate::win10::menu::radio(
                        ui,
                        &strip_mnemonic(&i18n(label)),
                        current == value,
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

    fn set_index(&mut self, index: Option<usize>) {
        self.index = index;
        self.summary.heading = self.title();
    }

    fn apply_graph_settings(&mut self, settings: &GraphSettings) {
        self.utilization.apply_settings(settings);
        self.middle.apply_settings(settings);
        self.bottom.apply_settings(settings);
        self.summary.graph.set_do_animation(settings.sliding_graphs);
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let Some(gpu) = readings.gpus.values().find(|g| g.id == self.gpu_id) else {
            return false;
        };
        self.units = env.units;
        self.max_duration = env.max_duration.clone();
        let units = self.units;
        let h = |v: f32, t: DataType| to_human_readable_nice(v, &t, &units);
        let l = &mut self.labels;

        let util = gpu.utilization_percent.unwrap_or(0.);
        self.utilization.add_data_point(vec![vec![util]]);
        l.utilization = format!("{}%", util.round());

        l.clock_current = gpu.clock_speed_mhz.map(|mhz| {
            for g in [&mut self.middle, &mut self.bottom] {
                g.add_single_data_point(CLOCK_GPU, vec![mhz as f32 * 1e6]);
            }
            h(mhz as f32 * 1e6, DataType::Hertz)
        });
        l.clock_max = gpu
            .max_clock_speed_mhz
            .map(|m| h(m as f32 * 1e6, DataType::Hertz));
        l.power_current = gpu.power_draw_watts.map(|w| {
            for g in [&mut self.middle, &mut self.bottom] {
                g.add_single_data_point(POWER, vec![w]);
            }
            h(w, DataType::Watts)
        });
        l.power_max = gpu.max_power_draw_watts.map(|w| h(w, DataType::Watts));

        l.memory_title = if gpu.used_memory.is_none() && gpu.total_memory.is_some() {
            i18n("Total Memory")
        } else {
            i18n("Memory Usage")
        };
        l.memory_current = gpu.used_memory.map(|m| {
            for g in [&mut self.middle, &mut self.bottom] {
                g.add_single_data_point(VRAM, vec![m as f32]);
            }
            h(m as f32, DataType::MemoryBytes)
        });
        l.memory_max = gpu.total_memory.map(|m| h(m as f32, DataType::MemoryBytes));
        l.gtt_title = if gpu.used_shared_memory.is_none() && gpu.total_shared_memory.is_some() {
            i18n("Total Shared Memory")
        } else {
            i18n("Shared Memory Usage")
        };
        l.gtt_current = gpu.used_shared_memory.map(|m| {
            for g in [&mut self.middle, &mut self.bottom] {
                if let Some(t) = gpu.total_shared_memory {
                    g.set_dataset_max_scale(GTT, t as f32);
                }
                g.add_single_data_point(GTT, vec![m as f32]);
            }
            h(m as f32, DataType::MemoryBytes)
        });
        l.gtt_max = gpu
            .total_shared_memory
            .map(|m| h(m as f32, DataType::MemoryBytes));
        l.mem_speed_current = gpu.memory_speed_mhz.map(|mhz| {
            for g in [&mut self.middle, &mut self.bottom] {
                g.add_single_data_point(CLOCK_MEM, vec![mhz as f32 * 1e6]);
            }
            h(mhz as f32 * 1e6, DataType::Hertz)
        });
        l.mem_speed_max = gpu
            .max_memory_speed_mhz
            .map(|m| h(m as f32 * 1e6, DataType::Hertz));
        l.encode = gpu.encoder_percent.map(|e| {
            for g in [&mut self.middle, &mut self.bottom] {
                g.add_single_data_point(ENCODE, vec![e]);
            }
            format!("{}%", e.round())
        });
        l.decode = if self.encode_decode_shared {
            None
        } else {
            gpu.decoder_percent.map(|d| {
                for g in [&mut self.middle, &mut self.bottom] {
                    g.add_single_data_point(DECODE, vec![d]);
                }
                format!("{}%", d.round())
            })
        };
        l.temperature = gpu.temperature_c.map(|t| {
            for g in [&mut self.middle, &mut self.bottom] {
                g.add_single_data_point(TEMPERATURE, vec![t]);
            }
            format!("{} °C", t.round() as i32)
        });
        l.pcie = match (gpu.pcie_gen, gpu.pcie_lanes) {
            (Some(g), Some(n)) => Some(format!("PCIe Gen {g} x{n} ")),
            _ => None,
        };
        l.max_pcie = match (gpu.max_pcie_gen, gpu.max_pcie_lanes) {
            (Some(g), Some(n)) if Some(g) != gpu.pcie_gen || Some(n) != gpu.pcie_lanes => {
                Some(format!("PCIe Gen {g} x{n} "))
            }
            _ => None,
        };
        self.configure_support(gpu);

        self.summary.graph.add_data_point(vec![vec![util]]);
        let mut info2 = String::new();
        if let Some(u) = gpu.utilization_percent {
            info2.push_str(&format!("{}%", u.round()));
        }
        if let Some(t) = gpu.temperature_c {
            info2.push_str(&format!(" ({} °C)", t.round() as u32));
        }
        self.summary.info2 = info2;
        true
    }

    fn animate(&mut self, frame: AnimationFrame) -> bool {
        self.utilization.update_animation(frame)
            | self.middle.update_animation(frame)
            | self.bottom.update_animation(frame)
    }

    fn win10_view(&mut self, config: &Config) -> PageView<'_> {
        let middle = self.effective(config.performance_page_gpu_graph_middle);
        let bottom = self.effective(config.performance_page_gpu_graph_bottom);
        Self::configure_graph(&mut self.middle, middle, self.gtt_available);
        Self::configure_graph(&mut self.bottom, bottom, self.gtt_available);
        let pair = |cur: &Option<String>, max: &Option<String>| match (cur, max) {
            (Some(c), Some(m)) => format!("{c} / {m}"),
            (Some(c), None) => c.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => String::new(),
        };
        let l = &self.labels;
        let mut first = vec![(i18n("Utilization"), l.utilization.clone())];
        if let Some(temp) = &l.temperature {
            first.push((i18n("Temperature"), temp.clone()));
        }
        let mut second = vec![];
        if l.memory_current.is_some() || l.memory_max.is_some() {
            second.push((
                l.memory_title.clone(),
                pair(&l.memory_current, &l.memory_max),
            ));
        }
        if l.gtt_current.is_some() || l.gtt_max.is_some() {
            second.push((l.gtt_title.clone(), pair(&l.gtt_current, &l.gtt_max)));
        }
        let mut third = vec![];
        if l.clock_current.is_some() {
            third.push((i18n("Clock Speed"), pair(&l.clock_current, &l.clock_max)));
        }
        if l.mem_speed_current.is_some() {
            third.push((
                i18n("Memory Speed"),
                pair(&l.mem_speed_current, &l.mem_speed_max),
            ));
        }
        if l.power_current.is_some() {
            third.push((i18n("Power Draw"), pair(&l.power_current, &l.power_max)));
        }
        if self.support.enc_dec {
            if self.encode_decode_shared {
                third.push((
                    i18n("Video encode/decode"),
                    l.encode.clone().unwrap_or_default(),
                ));
            } else {
                third.push((i18n("Video encode"), l.encode.clone().unwrap_or_default()));
                third.push((i18n("Video decode"), l.decode.clone().unwrap_or_default()));
            }
        }
        let stats: Vec<_> = [first, second, third]
            .into_iter()
            .filter(|r| !r.is_empty())
            .collect();
        let mut info = vec![
            (i18n("OpenGL version:"), self.opengl.clone()),
            (i18n("Vulkan version:"), self.vulkan.clone()),
        ];
        if let Some(p) = &l.pcie {
            info.push((i18n("PCI Express speed:"), p.clone()));
        }
        if let Some(p) = &l.max_pcie {
            info.push((i18n("Max PCI Express speed:"), p.clone()));
        }
        info.push((i18n("Device address:"), self.gpu_id.clone()));

        let middle_panel = (middle != GRAPH_NONE).then(|| {
            (
                over_caption(&self.caption_for(middle), &self.max_duration),
                self.total_for(&self.middle, middle),
            )
        });
        let bottom_panel = (bottom != GRAPH_NONE).then(|| {
            (
                over_caption(&self.caption_for(bottom), &self.max_duration),
                self.total_for(&self.bottom, bottom),
            )
        });
        let title = self.title();
        let model = self.device_name.clone();
        let mut panels = vec![GraphPanel::single(
            over("Utilization over ", &self.max_duration),
            i18n("100%"),
            &mut self.utilization,
        )];
        if let Some((caption, right)) = middle_panel {
            panels.push(GraphPanel::single(caption, right, &mut self.middle));
        }
        if let Some((caption, right)) = bottom_panel {
            panels.push(GraphPanel::single(caption, right, &mut self.bottom));
        }
        PageView {
            title,
            model,
            panels,
            stats,
            info,
        }
    }

    fn win10_menu(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.graph_menu(ui, cx, true);
        self.graph_menu(ui, cx, false);
    }

    fn data_summary(&self) -> String {
        let l = &self.labels;
        let o = |v: &Option<String>| v.clone().unwrap_or_default();
        format!(
            r#"{}

    {}

    OpenGL version:        {}
    Vulkan version:        {}
    PCI Express speed:     {}
    Max PCI Express speed: {}
    PCI bus address:       {}

    Utilization:   {}
    Memory usage:  {} / {}
    GTT usage:     {} / {}
    Clock speed:   {} / {}
    Memory speed:  {} / {}
    Power draw:    {}{}
    Encode/Decode: {} / {}
    Temperature:   {}"#,
            self.title(),
            self.device_name,
            self.opengl,
            self.vulkan,
            o(&l.pcie),
            o(&l.max_pcie),
            self.gpu_id,
            l.utilization,
            o(&l.memory_current),
            o(&l.memory_max),
            o(&l.gtt_current),
            o(&l.gtt_max),
            o(&l.clock_current),
            o(&l.clock_max),
            o(&l.mem_speed_current),
            o(&l.mem_speed_max),
            o(&l.power_current),
            o(&l.power_max),
            o(&l.encode),
            o(&l.decode),
            o(&l.temperature),
        )
    }
}

/// GPU captions end with " over " already ("Clock speed over ").
fn over_caption(caption: &str, duration: &str) -> String {
    format!("{caption}{duration}")
}
