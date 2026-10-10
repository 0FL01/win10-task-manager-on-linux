/* performance_page/gpu.rs
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

use std::cell::{Cell, RefCell};
use std::fmt::Write;

use adw::{self, subclass::prelude::*};
use arrayvec::ArrayString;
use glib::{g_critical, g_warning, ParamSpec, Properties, Value};
use gtk::{gio, glib, prelude::*};

use magpie_types::gpus::Gpu;
use magpie_types::gpus::OpenGlVariant;

use bitfield::{Bit, BitMut};

use crate::{
    application::INTERVAL_STEP, i18n::*, settings, to_short_human_readable_time, DataType,
};

use crate::performance_page::widgets::{
    AnimationFrame, DatasetGroup, FillingSettings, GraphWidget, GraphWidgetSettingsExt,
    RoundingSettings, ScalingSettings,
};

use crate::performance_page::fan::{TEMPERATURE_HIGH_WATERMARK, TEMPERATURE_LOW_WATERMARK};

use super::{GpuDetails, PageExt};

const GRAPH_NONE: i32 = 0;
const GRAPH_ENCODE_DECODE: i32 = 1;
const GRAPH_MEMORY: i32 = 2;
const GRAPH_POWER: i32 = 3;
const GRAPH_CLOCKS: i32 = 4;
const GRAPH_TEMPERATURE: i32 = 5;

const GRAPH_ENCODE_DATASET: usize = 0;
const GRAPH_DECODE_DATASET: usize = 1;
const GRAPH_VRAM_DATASET: usize = 2;
const GRAPH_GTT_DATASET: usize = 3;
const GRAPH_POWER_DATASET: usize = 4;
const GRAPH_CLOCK_GPU_DATASET: usize = 5;
const GRAPH_CLOCK_MEM_DATASET: usize = 6;
const GRAPH_TEMPERATURE_DATASET: usize = 7;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePageGpu)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/gpu.ui")]
    pub struct PerformancePageGpu {
        #[template_child]
        pub gpu_id: TemplateChild<gtk::Label>,
        #[template_child]
        pub device_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub big_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub graph_utilization: TemplateChild<GraphWidget>,

        #[template_child]
        pub middle_graph_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub middle_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub middle_graph_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub middle_graph_total: TemplateChild<gtk::Label>,

        #[template_child]
        pub bottom_graph_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub bottom_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub bottom_graph_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub bottom_graph_total: TemplateChild<gtk::Label>,

        #[template_child]
        pub context_menu: TemplateChild<gtk::Popover>,
        #[template_child]
        pub graph_max_duration: TemplateChild<gtk::Label>,

        #[property(get = Self::name, set = Self::set_name, type = String)]
        name: RefCell<String>,
        #[property(get, set)]
        base_color: Cell<gtk::gdk::RGBA>,
        #[property(get, set)]
        summary_mode: Cell<bool>,

        #[property(get, set)]
        encode_decode_available: Cell<bool>,

        encode_decode_shared: Cell<bool>,
        gtt_available: Cell<bool>,
        graph_middle_idx: Cell<i32>,
        graph_bottom_idx: Cell<i32>,
        graph_support: Cell<u8>,

        #[property(get = Self::infobar_content, type = Option < gtk::Widget >)]
        pub infobar_content: GpuDetails,

        graph_middle_enc_dec: gio::SimpleAction,
        graph_middle_memory: gio::SimpleAction,
        graph_middle_power: gio::SimpleAction,
        graph_middle_clocks: gio::SimpleAction,
        graph_middle_temperature: gio::SimpleAction,
        graph_middle_none: gio::SimpleAction,

        graph_bottom_enc_dec: gio::SimpleAction,
        graph_bottom_memory: gio::SimpleAction,
        graph_bottom_power: gio::SimpleAction,
        graph_bottom_clocks: gio::SimpleAction,
        graph_bottom_temperature: gio::SimpleAction,
        graph_bottom_none: gio::SimpleAction,

        actions: gio::SimpleActionGroup,
    }

    impl Default for PerformancePageGpu {
        fn default() -> Self {
            Self {
                gpu_id: Default::default(),
                device_name: Default::default(),
                big_box: Default::default(),
                graph_utilization: Default::default(),

                middle_graph_box: Default::default(),
                middle_graph: Default::default(),
                middle_graph_label: Default::default(),
                middle_graph_total: Default::default(),

                bottom_graph_box: Default::default(),
                bottom_graph: Default::default(),
                bottom_graph_label: Default::default(),
                bottom_graph_total: Default::default(),

                context_menu: Default::default(),
                graph_max_duration: Default::default(),

                name: RefCell::new(String::new()),
                base_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                summary_mode: Cell::new(false),

                encode_decode_available: Cell::new(true),

                encode_decode_shared: Cell::new(false),
                gtt_available: Cell::new(false),
                graph_middle_idx: Cell::new(GRAPH_NONE),
                graph_bottom_idx: Cell::new(GRAPH_NONE),
                graph_support: Cell::new(0),

                infobar_content: GpuDetails::new(),

                graph_middle_enc_dec: gio::SimpleAction::new_stateful(
                    "gpu_middle_enc_dec",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_middle_memory: gio::SimpleAction::new_stateful(
                    "gpu_middle_memory",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_middle_power: gio::SimpleAction::new_stateful(
                    "gpu_middle_power",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_middle_clocks: gio::SimpleAction::new_stateful(
                    "gpu_middle_clocks",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_middle_temperature: gio::SimpleAction::new_stateful(
                    "gpu_middle_temperature",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_middle_none: gio::SimpleAction::new_stateful(
                    "gpu_middle_none",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_bottom_enc_dec: gio::SimpleAction::new_stateful(
                    "gpu_bottom_enc_dec",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_bottom_memory: gio::SimpleAction::new_stateful(
                    "gpu_bottom_memory",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_bottom_power: gio::SimpleAction::new_stateful(
                    "gpu_bottom_power",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_bottom_clocks: gio::SimpleAction::new_stateful(
                    "gpu_bottom_clocks",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_bottom_temperature: gio::SimpleAction::new_stateful(
                    "gpu_bottom_temperature",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_bottom_none: gio::SimpleAction::new_stateful(
                    "gpu_bottom_none",
                    None,
                    &glib::Variant::from(true),
                ),

                actions: gio::SimpleActionGroup::new(),
            }
        }
    }

    impl PerformancePageGpu {
        fn name(&self) -> String {
            self.name.borrow().clone()
        }

        fn set_name(&self, name: String) {
            if name == *self.name.borrow() {
                return;
            }

            self.name.replace(name);
        }

        fn infobar_content(&self) -> Option<gtk::Widget> {
            Some(self.infobar_content.clone().upcast())
        }
    }

    impl PerformancePageGpu {
        fn configure_actions(this: &super::PerformancePageGpu) {
            this.insert_action_group("graph", Some(&this.imp().actions));

            let action = gio::SimpleAction::new("copy", None);
            action.connect_activate({
                let this = this.downgrade();
                move |_, _| {
                    if let Some(this) = this.upgrade() {
                        let clipboard = this.clipboard();
                        clipboard.set_text(this.imp().data_summary().as_str());
                    }
                }
            });
            this.imp().actions.add_action(&action);
        }

        fn configure_actions_graph(&self, gpu: &Gpu) {
            let settings = settings!();

            let mut gpu_middle_graph = settings.int("performance-page-gpu-graph-middle");
            let mut gpu_bottom_graph = settings.int("performance-page-gpu-graph-bottom");

            let enc_dec = gpu.encoder_percent.is_some() || gpu.decoder_percent.is_some();
            let memory = gpu.total_memory.is_some()
                || gpu.used_memory.is_some()
                || gpu.total_shared_memory.is_some()
                || gpu.used_shared_memory.is_some();
            let power = gpu.power_draw_watts.is_some();
            let clocks = gpu.clock_speed_mhz.is_some() || gpu.memory_speed_mhz.is_some();
            let temp = gpu.temperature_c.is_some();

            let mut graph_support = 0u8;
            // first field is 0 == disabled
            for (i, b) in [true, enc_dec, memory, power, clocks, temp]
                .iter()
                .enumerate()
            {
                graph_support.set_bit(i, *b)
            }
            self.graph_support.set(graph_support);

            if !graph_support.bit(gpu_middle_graph as usize) {
                gpu_middle_graph = GRAPH_NONE
            }
            self.graph_middle_idx.set(gpu_middle_graph);

            if !graph_support.bit(gpu_bottom_graph as usize) {
                gpu_bottom_graph = GRAPH_NONE
            }
            self.graph_bottom_idx.set(gpu_bottom_graph);

            settings.connect_changed(Some("performance-page-gpu-graph-middle"), {
                let this = self.obj().downgrade();
                move |settings, name| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();

                        let mut gpu_graph = settings.int(name);

                        if !this.graph_support.get().bit(gpu_graph as usize) {
                            gpu_graph = GRAPH_NONE;
                        }

                        this.graph_middle_idx.set(gpu_graph);

                        this.set_middle_graph_states(gpu_graph);
                        this.set_middle_graph(gpu_graph);
                    }
                }
            });

            settings.connect_changed(Some("performance-page-gpu-graph-bottom"), {
                let this = self.obj().downgrade();
                move |settings, name| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();

                        let mut gpu_graph = settings.int(name);

                        if !this.graph_support.get().bit(gpu_graph as usize) {
                            gpu_graph = GRAPH_NONE;
                        }

                        this.graph_bottom_idx.set(gpu_graph);

                        this.set_bottom_graph_states(gpu_graph);
                        this.set_bottom_graph(gpu_graph);
                    }
                }
            });

            let action = &self.graph_middle_none;
            action.set_enabled(true);
            action.connect_activate(move |_, _| Self::set_middle_graph_settings(GRAPH_NONE));
            self.actions.add_action(action);

            let action = &self.graph_middle_enc_dec;
            action.set_enabled(false);
            action
                .connect_activate(move |_, _| Self::set_middle_graph_settings(GRAPH_ENCODE_DECODE));
            self.actions.add_action(action);

            let action = &self.graph_middle_memory;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_middle_graph_settings(GRAPH_MEMORY));
            self.actions.add_action(action);

            let action = &self.graph_middle_power;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_middle_graph_settings(GRAPH_POWER));
            self.actions.add_action(action);

            let action = &self.graph_middle_clocks;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_middle_graph_settings(GRAPH_CLOCKS));
            self.actions.add_action(action);

            let action = &self.graph_middle_temperature;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_middle_graph_settings(GRAPH_TEMPERATURE));
            self.actions.add_action(action);

            let action = &self.graph_bottom_none;
            action.set_enabled(true);
            action.connect_activate(move |_, _| Self::set_bottom_graph_settings(GRAPH_NONE));
            self.actions.add_action(action);

            let action = &self.graph_bottom_enc_dec;
            action.set_enabled(false);
            action
                .connect_activate(move |_, _| Self::set_bottom_graph_settings(GRAPH_ENCODE_DECODE));
            self.actions.add_action(action);

            let action = &self.graph_bottom_memory;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_bottom_graph_settings(GRAPH_MEMORY));
            self.actions.add_action(action);

            let action = &self.graph_bottom_power;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_bottom_graph_settings(GRAPH_POWER));
            self.actions.add_action(action);

            let action = &self.graph_bottom_clocks;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_bottom_graph_settings(GRAPH_CLOCKS));
            self.actions.add_action(action);

            let action = &self.graph_bottom_temperature;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_bottom_graph_settings(GRAPH_TEMPERATURE));
            self.actions.add_action(action);

            if enc_dec {
                self.graph_middle_enc_dec.set_enabled(true);
                self.graph_bottom_enc_dec.set_enabled(true);

                if gpu.encode_decode_shared {
                    self.encode_decode_shared.set(true);
                    self.middle_graph
                        .set_filled(GRAPH_ENCODE_DATASET, FillingSettings::FillToBottom);
                    self.middle_graph.set_dashed(GRAPH_ENCODE_DATASET, false);
                    self.bottom_graph
                        .set_filled(GRAPH_ENCODE_DATASET, FillingSettings::FillToBottom);
                    self.bottom_graph.set_dashed(GRAPH_ENCODE_DATASET, false);
                }
            }

            if memory {
                self.graph_middle_memory.set_enabled(true);
                self.graph_bottom_memory.set_enabled(true);

                if let Some(total_memory) = gpu.total_memory {
                    self.middle_graph
                        .set_dataset_max_scale(GRAPH_VRAM_DATASET, total_memory as f32);
                    self.bottom_graph
                        .set_dataset_max_scale(GRAPH_VRAM_DATASET, total_memory as f32);
                }

                if let Some(total_shared_memory) = gpu.total_shared_memory {
                    self.middle_graph
                        .set_dataset_max_scale(GRAPH_GTT_DATASET, total_shared_memory as f32);
                    self.bottom_graph
                        .set_dataset_max_scale(GRAPH_GTT_DATASET, total_shared_memory as f32);
                }

                self.gtt_available.set(gpu.total_shared_memory.is_some());
            }

            if power {
                self.graph_middle_power.set_enabled(true);
                self.graph_bottom_power.set_enabled(true);

                if let Some(power_max) = gpu.max_power_draw_watts {
                    self.middle_graph
                        .set_dataset_max_scale(GRAPH_POWER_DATASET, power_max);
                    self.bottom_graph
                        .set_dataset_max_scale(GRAPH_POWER_DATASET, power_max);
                }
            }

            if clocks {
                self.graph_middle_clocks.set_enabled(true);
                self.graph_bottom_clocks.set_enabled(true);

                if let Some(clk) = gpu.max_clock_speed_mhz {
                    self.middle_graph
                        .set_dataset_max_scale(GRAPH_CLOCK_GPU_DATASET, clk as f32 * 1_000_000.);
                    self.bottom_graph
                        .set_dataset_max_scale(GRAPH_CLOCK_GPU_DATASET, clk as f32 * 1_000_000.);
                }
                if let Some(clk) = gpu.max_memory_speed_mhz {
                    self.middle_graph
                        .set_dataset_max_scale(GRAPH_CLOCK_MEM_DATASET, clk as f32 * 1_000_000.);
                    self.bottom_graph
                        .set_dataset_max_scale(GRAPH_CLOCK_MEM_DATASET, clk as f32 * 1_000_000.);
                }
            }

            if temp {
                self.graph_middle_temperature.set_enabled(true);
                self.graph_bottom_temperature.set_enabled(true);
            }

            Self::set_middle_graph_states(self, gpu_middle_graph);
            Self::set_bottom_graph_states(self, gpu_bottom_graph);
            Self::set_middle_graph(self, gpu_middle_graph);
            Self::set_bottom_graph(self, gpu_bottom_graph);
        }

        fn configure_context_menu(this: &super::PerformancePageGpu) {
            let right_click_controller = gtk::GestureClick::new();
            right_click_controller.set_button(3); // Secondary click (AKA right click)
            right_click_controller.connect_released({
                let this = this.downgrade();
                move |_click, _n_press, x, y| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    this.context_menu
                        .set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                            x.round() as i32,
                            y.round() as i32,
                            1,
                            1,
                        )));
                    this.context_menu.popup();
                }
            });
            this.add_controller(right_click_controller);
        }
    }

    impl PerformancePageGpu {
        pub fn set_static_information(
            this: &super::PerformancePageGpu,
            index: Option<usize>,
            gpu: &Gpu,
        ) -> bool {
            let this = this.imp();
            this.configure_actions_graph(gpu);

            if let Some(index) = index {
                this.gpu_id.set_text(&format!("GPU {}", index));
            } else {
                this.gpu_id.set_text("GPU");
            }

            if let Some(total_memory) = gpu.total_memory {
                let total_memory = total_memory as f32;
                let total_memory_str =
                    crate::to_human_readable_nice(total_memory, &DataType::MemoryBytes);

                this.infobar_content.set_total_memory_valid(true);

                this.infobar_content
                    .memory_usage_max()
                    .set_text(&total_memory_str);
            } else {
                this.infobar_content.set_total_memory_valid(false);
            }

            this.device_name
                .set_text(gpu.device_name.as_ref().unwrap_or(&i18n("Unknown")));

            this.infobar_content
                .set_encode_decode_shared(gpu.encode_decode_shared);
            if gpu.encode_decode_shared {
                this.infobar_content
                    .encode_label()
                    .set_text(&i18n("Video encode/decode"));
            }

            let mut ogl_version = ArrayString::<64>::new();
            if let Some(ogl_var) = gpu.opengl_variant {
                if ogl_var == OpenGlVariant::OpenGles as i32 {
                    ogl_version.push_str("ES ");
                }
            }

            if let Some(api_ver) = gpu.opengl_version.as_ref() {
                let _ = write!(&mut ogl_version, "{}.{}", api_ver.major, api_ver.minor);
            }

            if ogl_version.is_empty() {
                ogl_version.push_str(&i18n("Unknown"));
            }

            this.infobar_content
                .opengl_version()
                .set_text(ogl_version.as_str());

            let vk_version = if let Some(vulkan_version) = gpu.vulkan_version.as_ref() {
                format!(
                    "{}.{}.{}",
                    vulkan_version.major,
                    vulkan_version.minor,
                    vulkan_version.patch.unwrap_or(0)
                )
            } else {
                i18n("Unsupported")
            };
            this.infobar_content.vulkan_version().set_text(&vk_version);

            if let (Some(pcie_gen), Some(pcie_lanes)) = (gpu.pcie_gen, gpu.pcie_lanes) {
                this.infobar_content.set_pcie_info_visible(true);
                this.infobar_content
                    .pcie_speed()
                    .set_text(&format!("PCIe Gen {} x{} ", pcie_gen, pcie_lanes));
            } else {
                this.infobar_content.set_pcie_info_visible(false);
            }

            if let (Some(max_pcie_gen), Some(max_pcie_lanes)) =
                (gpu.max_pcie_gen, gpu.max_pcie_lanes)
            {
                this.infobar_content.set_max_pcie_info_visible(true);
                this.infobar_content
                    .max_pcie_speed()
                    .set_text(&format!("PCIe Gen {} x{} ", max_pcie_gen, max_pcie_lanes));
            } else {
                this.infobar_content.set_max_pcie_info_visible(false);
            }

            this.infobar_content.pci_addr().set_text(gpu.id.as_ref());

            true
        }

        pub fn update_readings(
            this: &super::PerformancePageGpu,
            gpu: &Gpu,
            index: Option<usize>,
        ) -> bool {
            let this = this.imp();

            if let Some(index) = index {
                this.gpu_id
                    .set_text(&i18n_f("GPU {}", &[&format!("{}", index)]));
            } else {
                this.gpu_id.set_text(&i18n("GPU"));
            }

            this.update_utilization(gpu);
            this.update_clock_speed(gpu);
            this.update_power_draw(gpu);
            this.update_memory_info(gpu);
            this.update_memory_speed(gpu);
            this.update_video_encode_decode(gpu);
            this.update_temperature(gpu);
            this.update_pcie(gpu);
            this.update_middle_graph_total(this.graph_middle_idx.get());
            this.update_bottom_graph_total(this.graph_bottom_idx.get());

            true
        }

        pub(crate) fn update_animations(
            this: &super::PerformancePageGpu,
            ticks: AnimationFrame,
        ) -> bool {
            let this = this.imp();

            this.graph_utilization.update_animation(ticks);
            this.middle_graph.update_animation(ticks);
            this.bottom_graph.update_animation(ticks);

            true
        }

        fn data_summary(&self) -> String {
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
                self.gpu_id.label(),
                self.device_name.label(),
                self.infobar_content.opengl_version().label(),
                self.infobar_content.vulkan_version().label(),
                self.infobar_content.pcie_speed().label(),
                self.infobar_content.max_pcie_speed().label(),
                self.infobar_content.pci_addr().label(),
                self.infobar_content.utilization().label(),
                self.infobar_content.memory_usage_current().label(),
                self.infobar_content.memory_usage_max().label(),
                self.infobar_content.shared_mem_usage_current().label(),
                self.infobar_content.shared_mem_usage_max().label(),
                self.infobar_content.clock_speed_current().label(),
                self.infobar_content.clock_speed_max().label(),
                self.infobar_content.memory_speed_current().label(),
                self.infobar_content.memory_speed_max().label(),
                self.infobar_content.power_draw_current().label(),
                self.infobar_content.power_draw_max().label(),
                self.infobar_content.encode_percent().label(),
                self.infobar_content.decode_percent().label(),
                self.infobar_content.temperature().label(),
            )
        }

        fn update_utilization(&self, gpu: &Gpu) {
            let overall_usage = gpu.utilization_percent.unwrap_or_else(|| {
                g_warning!(
                    "MissionCenter::PerformancePage",
                    "GPU '{}' utilization data is missing",
                    gpu.id
                );
                0.
            });

            self.graph_utilization
                .add_data_point(vec![vec![overall_usage]]);
            self.infobar_content
                .utilization()
                .set_text(&format!("{}%", overall_usage));
        }

        fn update_clock_speed(&self, gpu: &Gpu) {
            let mut clock_speed_available = false;

            if let Some(max_clock_speed) = gpu.max_clock_speed_mhz {
                self.infobar_content
                    .clock_speed_separator()
                    .set_visible(true);
                self.infobar_content.clock_speed_max().set_visible(true);

                let max_label = crate::to_human_readable_nice(
                    max_clock_speed as f32 * 1_000_000.,
                    &DataType::Hertz,
                );
                self.infobar_content.clock_speed_max().set_text(&max_label);
            } else {
                self.infobar_content
                    .clock_speed_separator()
                    .set_visible(false);
                self.infobar_content.clock_speed_max().set_visible(false);
            }

            if let Some(clock_speed) = gpu.clock_speed_mhz {
                clock_speed_available = true;

                let clock_speed = clock_speed as f32 * 1_000_000.;
                self.middle_graph
                    .add_single_data_point(GRAPH_CLOCK_GPU_DATASET, vec![clock_speed]);
                self.bottom_graph
                    .add_single_data_point(GRAPH_CLOCK_GPU_DATASET, vec![clock_speed]);

                let clock_label = crate::to_human_readable_nice(clock_speed, &DataType::Hertz);

                self.infobar_content
                    .clock_speed_current()
                    .set_text(&clock_label);
            }

            self.infobar_content
                .set_clock_speed_available(clock_speed_available);
        }

        fn update_power_draw(&self, gpu: &Gpu) {
            let mut power_draw_available = false;

            if let Some(power_limit) = gpu.max_power_draw_watts {
                self.infobar_content
                    .power_draw_separator()
                    .set_visible(true);
                self.infobar_content.power_draw_max().set_visible(true);

                let power_limit = crate::to_human_readable_nice(power_limit, &DataType::Watts);
                self.infobar_content.power_draw_max().set_text(&power_limit);
            } else {
                self.infobar_content
                    .power_draw_separator()
                    .set_visible(false);
                self.infobar_content.power_draw_max().set_visible(false);
            }

            if let Some(power_draw) = gpu.power_draw_watts {
                power_draw_available = true;

                self.middle_graph
                    .add_single_data_point(GRAPH_POWER_DATASET, vec![power_draw]);
                self.bottom_graph
                    .add_single_data_point(GRAPH_POWER_DATASET, vec![power_draw]);

                let power_draw = crate::to_human_readable_nice(power_draw, &DataType::Watts);
                self.infobar_content
                    .power_draw_current()
                    .set_text(&power_draw);
            }

            self.infobar_content
                .set_power_draw_available(power_draw_available);
        }

        fn update_memory_info(&self, gpu: &Gpu) {
            fn update_dedicated_memory(this: &PerformancePageGpu, gpu: &Gpu) {
                if let Some(used_memory) = gpu.used_memory {
                    this.infobar_content.set_used_memory_valid(true);
                    this.infobar_content
                        .memory_usage_title()
                        .set_text(&i18n("Memory Usage"));

                    this.middle_graph
                        .add_single_data_point(GRAPH_VRAM_DATASET, vec![used_memory as f32]);
                    this.bottom_graph
                        .add_single_data_point(GRAPH_VRAM_DATASET, vec![used_memory as f32]);

                    let used_memory = crate::to_human_readable_nice(
                        gpu.used_memory.unwrap_or(0) as f32,
                        &DataType::MemoryBytes,
                    );
                    this.infobar_content
                        .memory_usage_current()
                        .set_text(&used_memory);
                } else {
                    this.infobar_content.set_used_memory_valid(false);

                    if this.infobar_content.total_memory_valid() {
                        this.infobar_content
                            .memory_usage_title()
                            .set_text(&i18n("Total Memory"));
                    }
                }
            }

            fn update_shared_memory(this: &PerformancePageGpu, gpu: &Gpu) {
                if let Some(total_shared_memory) = gpu.total_shared_memory {
                    let total_gtt = crate::to_human_readable_nice(
                        total_shared_memory as f32,
                        &DataType::MemoryBytes,
                    );

                    this.infobar_content.set_total_shared_memory_valid(true);
                    this.middle_graph
                        .set_dataset_max_scale(GRAPH_GTT_DATASET, total_shared_memory as f32);
                    this.bottom_graph
                        .set_dataset_max_scale(GRAPH_GTT_DATASET, total_shared_memory as f32);

                    this.infobar_content
                        .shared_mem_usage_max()
                        .set_text(&total_gtt);
                } else {
                    this.infobar_content.set_total_shared_memory_valid(false);
                }

                if let Some(used_shared_memory) = gpu.used_shared_memory {
                    this.middle_graph
                        .add_single_data_point(GRAPH_GTT_DATASET, vec![used_shared_memory as f32]);
                    this.bottom_graph
                        .add_single_data_point(GRAPH_GTT_DATASET, vec![used_shared_memory as f32]);

                    this.infobar_content.set_used_shared_memory_valid(true);
                    this.infobar_content
                        .shared_memory_usage_title()
                        .set_text(&i18n("Shared Memory Usage"));

                    let used_shared_mem_str = crate::to_human_readable_nice(
                        used_shared_memory as f32,
                        &DataType::MemoryBytes,
                    );

                    this.infobar_content
                        .shared_mem_usage_current()
                        .set_text(&used_shared_mem_str);
                } else {
                    this.infobar_content.set_used_shared_memory_valid(false);

                    if this.infobar_content.total_shared_memory_valid() {
                        this.infobar_content
                            .shared_memory_usage_title()
                            .set_text(&i18n("Total Shared Memory"));
                    }
                }
            }

            update_dedicated_memory(self, gpu);
            update_shared_memory(self, gpu);
        }

        fn update_memory_speed(&self, gpu: &Gpu) {
            let mut memory_speed_available = false;

            if let Some(max_memory_speed) = gpu.max_memory_speed_mhz {
                self.infobar_content
                    .memory_speed_separator()
                    .set_visible(true);
                self.infobar_content.memory_speed_max().set_visible(true);

                let ms_max = crate::to_human_readable_nice(
                    max_memory_speed as f32 * 1_000_000.,
                    &DataType::Hertz,
                );
                self.infobar_content.memory_speed_max().set_text(&ms_max);
            } else {
                self.infobar_content
                    .memory_speed_separator()
                    .set_visible(false);
                self.infobar_content.memory_speed_max().set_visible(false);
            }

            if let Some(memory_speed) = gpu.memory_speed_mhz {
                memory_speed_available = true;

                let memory_speed = memory_speed as f32 * 1_000_000.;
                self.middle_graph
                    .add_single_data_point(GRAPH_CLOCK_MEM_DATASET, vec![memory_speed]);
                self.bottom_graph
                    .add_single_data_point(GRAPH_CLOCK_MEM_DATASET, vec![memory_speed]);

                let memory_speed = crate::to_human_readable_nice(memory_speed, &DataType::Hertz);
                self.infobar_content
                    .memory_speed_current()
                    .set_text(&memory_speed);
            }

            self.infobar_content
                .set_memory_speed_available(memory_speed_available);
        }

        fn update_video_encode_decode(&self, gpu: &Gpu) {
            if let Some(encoder_percent) = gpu.encoder_percent {
                self.middle_graph
                    .add_single_data_point(GRAPH_ENCODE_DATASET, vec![encoder_percent]);
                self.bottom_graph
                    .add_single_data_point(GRAPH_ENCODE_DATASET, vec![encoder_percent]);

                self.infobar_content
                    .encode_percent()
                    .set_text(&format!("{}%", encoder_percent));
            }

            if !gpu.encode_decode_shared {
                if let Some(decoder_percent) = gpu.decoder_percent {
                    self.middle_graph
                        .add_single_data_point(GRAPH_DECODE_DATASET, vec![decoder_percent]);
                    self.bottom_graph
                        .add_single_data_point(GRAPH_DECODE_DATASET, vec![decoder_percent]);

                    self.infobar_content
                        .decode_percent()
                        .set_text(&format!("{}%", decoder_percent));
                }
            }
        }

        fn update_temperature(&self, gpu: &Gpu) {
            if let Some(temp) = gpu.temperature_c {
                self.infobar_content.box_temp().set_visible(true);

                self.middle_graph
                    .add_single_data_point(GRAPH_TEMPERATURE_DATASET, vec![temp]);
                self.bottom_graph
                    .add_single_data_point(GRAPH_TEMPERATURE_DATASET, vec![temp]);

                self.infobar_content
                    .temperature()
                    .set_text(&format!("{} °C", temp.round() as i32));
            } else {
                self.infobar_content.box_temp().set_visible(false);
            }
        }

        fn update_pcie(&self, gpu: &Gpu) {
            if let (Some(pcie_gen), Some(pcie_lanes)) = (gpu.pcie_gen, gpu.pcie_lanes) {
                self.infobar_content.set_pcie_info_visible(true);
                self.infobar_content
                    .pcie_speed()
                    .set_text(&format!("PCIe Gen {} x{} ", pcie_gen, pcie_lanes));
                if let (Some(max_pcie_gen), Some(max_pcie_lanes)) =
                    (gpu.max_pcie_gen, gpu.max_pcie_lanes)
                {
                    self.infobar_content.set_max_pcie_info_visible(
                        !(max_pcie_gen == pcie_gen && max_pcie_lanes == pcie_lanes),
                    )
                } else {
                    self.infobar_content.set_max_pcie_info_visible(false);
                }
            } else {
                self.infobar_content.set_pcie_info_visible(false);
                self.infobar_content.set_max_pcie_info_visible(false);
            }
        }
    }

    impl PerformancePageGpu {
        fn set_middle_graph_states(&self, gpu_graph: i32) {
            self.graph_middle_none
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_NONE));
            self.graph_middle_enc_dec
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_ENCODE_DECODE));
            self.graph_middle_memory
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_MEMORY));
            self.graph_middle_power
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_POWER));
            self.graph_middle_clocks
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_CLOCKS));
            self.graph_middle_temperature
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_TEMPERATURE));

            self.graph_bottom_enc_dec
                .set_enabled(gpu_graph != GRAPH_ENCODE_DECODE);
            self.graph_bottom_memory
                .set_enabled(gpu_graph != GRAPH_MEMORY);
            self.graph_bottom_power
                .set_enabled(gpu_graph != GRAPH_POWER);
            self.graph_bottom_clocks
                .set_enabled(gpu_graph != GRAPH_CLOCKS);
            self.graph_bottom_temperature
                .set_enabled(gpu_graph != GRAPH_TEMPERATURE);
        }
        fn set_bottom_graph_states(&self, gpu_graph: i32) {
            self.graph_bottom_none
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_NONE));
            self.graph_bottom_enc_dec
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_ENCODE_DECODE));
            self.graph_bottom_memory
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_MEMORY));
            self.graph_bottom_power
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_POWER));
            self.graph_bottom_clocks
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_CLOCKS));
            self.graph_bottom_temperature
                .set_state(&glib::Variant::from(gpu_graph == GRAPH_TEMPERATURE));

            self.graph_middle_enc_dec
                .set_enabled(gpu_graph != GRAPH_ENCODE_DECODE);
            self.graph_middle_memory
                .set_enabled(gpu_graph != GRAPH_MEMORY);
            self.graph_middle_power
                .set_enabled(gpu_graph != GRAPH_POWER);
            self.graph_middle_clocks
                .set_enabled(gpu_graph != GRAPH_CLOCKS);
            self.graph_middle_temperature
                .set_enabled(gpu_graph != GRAPH_TEMPERATURE);
        }

        fn set_middle_graph_settings(num: i32) {
            settings!()
                .set_int("performance-page-gpu-graph-middle", num)
                .unwrap_or_else(|_| {
                    g_critical!(
                        "MissionCenter::PerformancePage",
                        "Failed to save middle gpu graph state"
                    );
                });
        }

        fn set_bottom_graph_settings(num: i32) {
            settings!()
                .set_int("performance-page-gpu-graph-bottom", num)
                .unwrap_or_else(|_| {
                    g_critical!(
                        "MissionCenter::PerformancePage",
                        "Failed to save bottom gpu graph state"
                    );
                });
        }

        fn set_graph(
            &self,
            graph: &GraphWidget,
            graph_box: &gtk::Box,
            graph_label: &gtk::Label,
            graph_total: &gtk::Label,
            num: i32,
        ) {
            graph_box.set_visible(num != GRAPH_NONE);
            let graph_middle = self.graph_middle_idx.get();
            let graph_bottom = self.graph_bottom_idx.get();

            let single_graph = graph_middle == GRAPH_NONE && graph_bottom == GRAPH_NONE;

            self.big_box.set_homogeneous(!single_graph);
            self.big_box.set_spacing(!single_graph as i32 * 10);

            let enc_dec =
                graph_middle == GRAPH_ENCODE_DECODE || graph_bottom == GRAPH_ENCODE_DECODE;
            let memory = graph_middle == GRAPH_MEMORY || graph_bottom == GRAPH_MEMORY;
            let clocks = graph_middle == GRAPH_CLOCKS || graph_bottom == GRAPH_CLOCKS;
            self.infobar_content
                .set_legend_enc_dec_visible(enc_dec && !self.encode_decode_shared.get());
            self.infobar_content
                .set_legend_memory_visible(memory && self.gtt_available.get());
            self.infobar_content
                .set_legend_clock_visible(clocks && self.infobar_content.is_both_clocks_visible());

            if num != GRAPH_NONE {
                graph.set_data_visible(GRAPH_ENCODE_DATASET, num == GRAPH_ENCODE_DECODE);
                graph.set_data_visible(GRAPH_DECODE_DATASET, num == GRAPH_ENCODE_DECODE);

                graph.set_data_visible(GRAPH_VRAM_DATASET, num == GRAPH_MEMORY);
                graph.set_data_visible(
                    GRAPH_GTT_DATASET,
                    self.gtt_available.get() && num == GRAPH_MEMORY,
                );

                graph.set_data_visible(GRAPH_POWER_DATASET, num == GRAPH_POWER);

                graph.set_data_visible(GRAPH_CLOCK_GPU_DATASET, num == GRAPH_CLOCKS);
                graph.set_data_visible(GRAPH_CLOCK_MEM_DATASET, num == GRAPH_CLOCKS);

                graph.set_data_visible(GRAPH_TEMPERATURE_DATASET, num == GRAPH_TEMPERATURE);

                match num {
                    GRAPH_ENCODE_DECODE => {
                        graph_label.set_text(&i18n("Video encode/decode utilization over "));
                        graph_total.set_text(&i18n("100%"));
                    }
                    GRAPH_MEMORY => {
                        if self.gtt_available.get() {
                            graph_label.set_text(&i18n("Dedicated and shared memory usage over "));
                        } else {
                            graph_label.set_text(&i18n("Memory usage over "));
                        }
                    }
                    GRAPH_POWER => {
                        graph_label.set_text(&i18n("Power draw over "));
                    }
                    GRAPH_CLOCKS => {
                        graph_label.set_text(&i18n("Clock speed over "));
                    }
                    GRAPH_TEMPERATURE => {
                        graph_label.set_text(&i18n("Temperature over "));
                    }
                    _ => {}
                };

                self.update_graph_total(graph, graph_total, num);
                graph.force_redraw();
            }
        }

        fn update_graph_total(&self, graph: &GraphWidget, graph_total: &gtk::Label, num: i32) {
            let text = match num {
                GRAPH_NONE | GRAPH_ENCODE_DECODE => return,
                GRAPH_MEMORY => {
                    if self.gtt_available.get() {
                        format!(
                            "{} / {}",
                            crate::to_human_readable_nice(
                                graph.get_dataset_max_scale(GRAPH_VRAM_DATASET),
                                &DataType::MemoryBytes,
                            ),
                            crate::to_human_readable_nice(
                                graph.get_dataset_max_scale(GRAPH_GTT_DATASET),
                                &DataType::MemoryBytes,
                            ),
                        )
                    } else {
                        crate::to_human_readable_nice(
                            graph.get_dataset_max_scale(GRAPH_VRAM_DATASET),
                            &DataType::MemoryBytes,
                        )
                    }
                }
                GRAPH_POWER => crate::to_human_readable_nice(
                    graph.get_dataset_max_scale(GRAPH_POWER_DATASET),
                    &DataType::Watts,
                ),
                GRAPH_CLOCKS => crate::to_human_readable_nice(
                    graph.get_dataset_max_scale(GRAPH_CLOCK_GPU_DATASET),
                    &DataType::Hertz,
                ),
                GRAPH_TEMPERATURE => {
                    format!(
                        "{:.0} °C",
                        graph.get_dataset_max_scale(GRAPH_TEMPERATURE_DATASET)
                    )
                }
                _ => String::new(),
            };
            graph_total.set_text(&text)
        }

        fn set_middle_graph(&self, num: i32) {
            self.set_graph(
                &self.middle_graph,
                &self.middle_graph_box,
                &self.middle_graph_label,
                &self.middle_graph_total,
                num,
            )
        }

        fn set_bottom_graph(&self, num: i32) {
            self.set_graph(
                &self.bottom_graph,
                &self.bottom_graph_box,
                &self.bottom_graph_label,
                &self.bottom_graph_total,
                num,
            )
        }

        fn update_middle_graph_total(&self, num: i32) {
            self.update_graph_total(&self.middle_graph, &self.middle_graph_total, num)
        }

        fn update_bottom_graph_total(&self, num: i32) {
            self.update_graph_total(&self.bottom_graph, &self.bottom_graph_total, num)
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePageGpu {
        const NAME: &'static str = "PerformancePageGpu";
        type Type = super::PerformancePageGpu;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePageGpu {
        fn properties() -> &'static [ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }

        fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
            self.derived_property(id, pspec)
        }

        fn constructed(&self) {
            self.parent_constructed();

            let settings = settings!();

            let mut encode = DatasetGroup::new();
            encode.dataset_settings.fill = FillingSettings::None;
            encode.dataset_settings.dashed = true;
            let decode = DatasetGroup::new();

            let vram = DatasetGroup::new();
            let mut gtt = DatasetGroup::new();
            gtt.dataset_settings.dashed = true;
            gtt.dataset_settings.fill = FillingSettings::None;

            let mut power = DatasetGroup::new();
            power.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
            power.dataset_settings.rounding_settings = RoundingSettings::Pow10;
            power.dataset_settings.high_watermark = 0.;

            let mut clock_gpu = DatasetGroup::new();
            clock_gpu.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
            clock_gpu.dataset_settings.high_watermark = 0.;
            let mut clock_mem = DatasetGroup::new();
            clock_mem.dataset_settings.fill = FillingSettings::None;
            clock_mem.dataset_settings.dashed = true;
            clock_mem.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
            clock_mem.dataset_settings.high_watermark = 0.;

            let mut temp = DatasetGroup::new();
            temp.dataset_settings.scaling_settings = ScalingSettings::StickyUpDown;
            power.dataset_settings.rounding_settings = RoundingSettings::Integer;
            temp.dataset_settings.high_watermark = TEMPERATURE_HIGH_WATERMARK;
            temp.dataset_settings.low_watermark = TEMPERATURE_LOW_WATERMARK;

            self.middle_graph.add_dataset(encode.clone());
            self.middle_graph.add_dataset(decode.clone());
            self.middle_graph.add_dataset(vram.clone());
            self.middle_graph.add_dataset(gtt.clone());
            self.middle_graph.add_dataset(power.clone());
            self.middle_graph.add_dataset(clock_gpu.clone());
            self.middle_graph.add_dataset(clock_mem.clone());
            self.middle_graph.add_dataset(temp.clone());

            self.middle_graph
                .connect_datasets(GRAPH_CLOCK_GPU_DATASET, GRAPH_CLOCK_MEM_DATASET);
            self.middle_graph
                .connect_datasets(GRAPH_CLOCK_MEM_DATASET, GRAPH_CLOCK_GPU_DATASET);

            self.middle_graph.connect_to_settings(&settings);

            self.bottom_graph.add_dataset(encode);
            self.bottom_graph.add_dataset(decode);
            self.bottom_graph.add_dataset(vram);
            self.bottom_graph.add_dataset(gtt);
            self.bottom_graph.add_dataset(power);
            self.bottom_graph.add_dataset(clock_gpu);
            self.bottom_graph.add_dataset(clock_mem);
            self.bottom_graph.add_dataset(temp);

            self.bottom_graph
                .connect_datasets(GRAPH_CLOCK_GPU_DATASET, GRAPH_CLOCK_MEM_DATASET);
            self.bottom_graph
                .connect_datasets(GRAPH_CLOCK_MEM_DATASET, GRAPH_CLOCK_GPU_DATASET);

            self.bottom_graph.connect_to_settings(&settings);

            let util = DatasetGroup::new();
            self.graph_utilization.add_dataset(util);
            self.graph_utilization.connect_to_settings(&settings);

            let this = self.obj();

            this.as_ref()
                .bind_property(
                    "encode-decode-available",
                    &self.infobar_content,
                    "encode-decode-available",
                )
                .flags(glib::BindingFlags::SYNC_CREATE)
                .build();

            Self::configure_actions(&this);
            Self::configure_context_menu(&this);
        }
    }

    impl WidgetImpl for PerformancePageGpu {}

    impl BoxImpl for PerformancePageGpu {}
}

glib::wrapper! {
    pub struct PerformancePageGpu(ObjectSubclass<imp::PerformancePageGpu>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PageExt for PerformancePageGpu {
    fn infobar_collapsed(&self) {
        self.imp().infobar_content.set_collapsed(true);
    }

    fn infobar_uncollapsed(&self) {
        self.imp().infobar_content.set_collapsed(false);
    }
}

impl PerformancePageGpu {
    pub fn new(name: &str) -> Self {
        fn update_refresh_rate_sensitive_labels(
            this: &PerformancePageGpu,
            settings: &gio::Settings,
        ) {
            let this = this.imp();

            let data_points = settings.int("performance-page-data-points") as u32;
            let delay = settings.uint64("app-update-interval-u64");

            let graph_max_duration =
                (((delay as f64) * INTERVAL_STEP) * (data_points as f64)).round() as u32;

            this.graph_max_duration
                .set_text(&to_short_human_readable_time(graph_max_duration));
        }

        let this: Self = glib::Object::builder().property("name", name).build();
        let settings = settings!();
        update_refresh_rate_sensitive_labels(&this, &settings);

        settings.connect_changed(Some("performance-page-data-points"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        settings.connect_changed(Some("app-update-interval"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        this
    }

    pub fn set_static_information(&self, index: Option<usize>, gpu: &Gpu) -> bool {
        imp::PerformancePageGpu::set_static_information(self, index, gpu)
    }

    pub fn update_readings(&self, gpu: &Gpu, index: Option<usize>) -> bool {
        imp::PerformancePageGpu::update_readings(self, gpu, index)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePageGpu::update_animations(self, ticks)
    }
}
