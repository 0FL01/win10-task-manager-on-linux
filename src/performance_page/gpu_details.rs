/* performance_page/gpu_details.rs
 *
 * Copyright 2025 Mission Center Developers
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

use std::cell::Cell;

use glib::{ParamSpec, Properties, Value};
use gtk::prelude::WidgetExt;
use gtk::{gdk::prelude::*, glib, subclass::prelude::*};

mod imp {
    use std::marker::PhantomData;

    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::GpuDetails)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/gpu_details.ui")]
    pub struct GpuDetails {
        #[template_child]
        pub utilization: TemplateChild<gtk::Label>,
        #[template_child]
        pub memory_usage_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub memory_usage_current: TemplateChild<gtk::Label>,
        #[template_child]
        pub memory_usage_max: TemplateChild<gtk::Label>,
        #[template_child]
        pub shared_memory_usage_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub gtt_usage_current: TemplateChild<gtk::Label>,
        #[template_child]
        pub gtt_usage_max: TemplateChild<gtk::Label>,
        #[template_child]
        pub clock_speed_current: TemplateChild<gtk::Label>,
        #[template_child]
        pub clock_speed_separator: TemplateChild<gtk::Label>,
        #[template_child]
        pub clock_speed_max: TemplateChild<gtk::Label>,
        #[template_child]
        pub memory_speed_current: TemplateChild<gtk::Label>,
        #[template_child]
        pub memory_speed_separator: TemplateChild<gtk::Label>,
        #[template_child]
        pub memory_speed_max: TemplateChild<gtk::Label>,
        #[template_child]
        pub power_draw_current: TemplateChild<gtk::Label>,
        #[template_child]
        pub power_draw_separator: TemplateChild<gtk::Label>,
        #[template_child]
        pub power_draw_max: TemplateChild<gtk::Label>,
        #[template_child]
        pub encode_percent: TemplateChild<gtk::Label>,
        #[template_child]
        pub decode_percent: TemplateChild<gtk::Label>,
        #[template_child]
        pub temperature: TemplateChild<gtk::Label>,
        #[template_child]
        pub opengl_version: TemplateChild<gtk::Label>,
        #[template_child]
        pub vulkan_version: TemplateChild<gtk::Label>,
        #[template_child]
        pub pcie_speed_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub pcie_speed: TemplateChild<gtk::Label>,
        #[template_child]
        pub max_pcie_speed_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub max_pcie_speed: TemplateChild<gtk::Label>,
        #[template_child]
        pub pci_addr: TemplateChild<gtk::Label>,

        #[template_child]
        pub box_temp: TemplateChild<gtk::Box>,
        #[template_child]
        pub box_mem_speed: TemplateChild<gtk::Box>,
        #[template_child]
        pub box_power_draw: TemplateChild<gtk::Box>,
        #[template_child]
        pub encode_label: TemplateChild<gtk::Label>,

        #[template_child]
        pub legend_encode: TemplateChild<gtk::Picture>,
        #[template_child]
        pub legend_decode: TemplateChild<gtk::Picture>,
        #[template_child]
        pub legend_vram: TemplateChild<gtk::Picture>,
        #[template_child]
        pub legend_gtt: TemplateChild<gtk::Picture>,
        #[template_child]
        pub legend_clock: TemplateChild<gtk::Picture>,
        #[template_child]
        pub legend_clock_mem: TemplateChild<gtk::Picture>,

        #[property(get, set)]
        clock_speed_available: Cell<bool>,
        #[property(get, set)]
        power_draw_available: Cell<bool>,
        #[property(get, set)]
        memory_speed_available: Cell<bool>,
        #[property(get, set)]
        encode_decode_available: Cell<bool>,
        #[property(get, set)]
        encode_decode_shared: Cell<bool>,
        #[property(get, set = Self::set_total_memory_valid)]
        total_memory_valid: Cell<bool>,
        #[property(get, set = Self::set_used_memory_valid)]
        used_memory_valid: Cell<bool>,
        #[allow(dead_code)]
        #[property(get = Self::dedicated_memory_available)]
        dedicated_memory_available: PhantomData<bool>,
        #[allow(dead_code)]
        #[property(get = Self::show_dedicated_separator)]
        show_dedicated_separator: PhantomData<bool>,
        #[property(get, set = Self::set_total_shared_memory_valid)]
        total_shared_memory_valid: Cell<bool>,
        #[property(get, set = Self::set_used_shared_memory_valid)]
        used_shared_memory_valid: Cell<bool>,
        #[allow(dead_code)]
        #[property(get = Self::shared_memory_available)]
        shared_memory_available: PhantomData<bool>,
        #[allow(dead_code)]
        #[property(get = Self::show_shared_separator)]
        show_shared_separator: PhantomData<bool>,
        #[property(get, set)]
        pcie_info_visible: Cell<bool>,
        #[property(get, set)]
        max_pcie_info_visible: Cell<bool>,
    }

    impl Default for GpuDetails {
        fn default() -> Self {
            Self {
                utilization: TemplateChild::default(),
                memory_usage_title: TemplateChild::default(),
                memory_usage_current: TemplateChild::default(),
                memory_usage_max: TemplateChild::default(),
                shared_memory_usage_title: TemplateChild::default(),
                gtt_usage_current: TemplateChild::default(),
                gtt_usage_max: TemplateChild::default(),
                clock_speed_current: TemplateChild::default(),
                clock_speed_separator: TemplateChild::default(),
                clock_speed_max: TemplateChild::default(),
                memory_speed_current: TemplateChild::default(),
                memory_speed_separator: TemplateChild::default(),
                memory_speed_max: TemplateChild::default(),
                power_draw_current: TemplateChild::default(),
                power_draw_separator: TemplateChild::default(),
                power_draw_max: TemplateChild::default(),
                encode_percent: TemplateChild::default(),
                decode_percent: TemplateChild::default(),
                temperature: TemplateChild::default(),
                opengl_version: TemplateChild::default(),
                vulkan_version: TemplateChild::default(),
                pcie_speed_label: TemplateChild::default(),
                pcie_speed: TemplateChild::default(),
                max_pcie_speed_label: TemplateChild::default(),
                max_pcie_speed: TemplateChild::default(),
                pci_addr: TemplateChild::default(),

                box_temp: TemplateChild::default(),
                box_mem_speed: TemplateChild::default(),
                box_power_draw: TemplateChild::default(),
                encode_label: TemplateChild::default(),

                legend_encode: TemplateChild::default(),
                legend_decode: TemplateChild::default(),
                legend_vram: TemplateChild::default(),
                legend_gtt: TemplateChild::default(),
                legend_clock: TemplateChild::default(),
                legend_clock_mem: TemplateChild::default(),

                clock_speed_available: Cell::new(true),
                power_draw_available: Cell::new(true),
                memory_speed_available: Cell::new(true),
                encode_decode_available: Cell::new(true),
                encode_decode_shared: Cell::new(false),
                total_memory_valid: Cell::new(true),
                used_memory_valid: Cell::new(true),
                dedicated_memory_available: PhantomData,
                show_dedicated_separator: PhantomData,
                total_shared_memory_valid: Cell::new(false),
                used_shared_memory_valid: Cell::new(false),
                shared_memory_available: PhantomData,
                show_shared_separator: PhantomData,
                pcie_info_visible: Cell::new(false),
                max_pcie_info_visible: Cell::new(false),
            }
        }
    }

    impl GpuDetails {
        fn set_total_memory_valid(&self, valid: bool) {
            self.total_memory_valid.set(valid);
            self.obj().notify_dedicated_memory_available();
            self.obj().notify_show_dedicated_separator();
        }

        fn set_used_memory_valid(&self, valid: bool) {
            self.used_memory_valid.set(valid);
            self.obj().notify_dedicated_memory_available();
            self.obj().notify_show_dedicated_separator();
        }

        fn dedicated_memory_available(&self) -> bool {
            self.total_memory_valid.get() || self.used_memory_valid.get()
        }

        fn show_dedicated_separator(&self) -> bool {
            self.total_memory_valid.get() && self.used_memory_valid.get()
        }

        fn set_total_shared_memory_valid(&self, valid: bool) {
            self.total_shared_memory_valid.set(valid);
            self.obj().notify_shared_memory_available();
            self.obj().notify_show_shared_separator();
        }

        fn set_used_shared_memory_valid(&self, valid: bool) {
            self.used_shared_memory_valid.set(valid);
            self.obj().notify_shared_memory_available();
            self.obj().notify_show_shared_separator();
        }

        fn shared_memory_available(&self) -> bool {
            self.total_shared_memory_valid.get() || self.used_shared_memory_valid.get()
        }

        fn show_shared_separator(&self) -> bool {
            self.total_shared_memory_valid.get() && self.used_shared_memory_valid.get()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GpuDetails {
        const NAME: &'static str = "GpuDetails";
        type Type = super::GpuDetails;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GpuDetails {
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

            self.legend_encode
                .set_resource(Some("/io/missioncenter/MissionCenter/line-dashed-gpu.svg"));
            self.legend_decode
                .set_resource(Some("/io/missioncenter/MissionCenter/line-solid-gpu.svg"));

            self.legend_gtt
                .set_resource(Some("/io/missioncenter/MissionCenter/line-dashed-gpu.svg"));
            self.legend_vram
                .set_resource(Some("/io/missioncenter/MissionCenter/line-solid-gpu.svg"));

            self.legend_clock_mem
                .set_resource(Some("/io/missioncenter/MissionCenter/line-dashed-gpu.svg"));
            self.legend_clock
                .set_resource(Some("/io/missioncenter/MissionCenter/line-solid-gpu.svg"));
        }
    }

    impl WidgetImpl for GpuDetails {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl BoxImpl for GpuDetails {}
}

glib::wrapper! {
    pub struct GpuDetails(ObjectSubclass<imp::GpuDetails>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl GpuDetails {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_collapsed(&self, collapsed: bool) {
        if collapsed {
            self.set_margin_top(10);
        } else {
            self.set_margin_top(65);
        }
    }

    pub fn utilization(&self) -> &gtk::Label {
        &self.imp().utilization
    }

    pub fn memory_usage_title(&self) -> &gtk::Label {
        &self.imp().memory_usage_title
    }

    pub fn memory_usage_current(&self) -> &gtk::Label {
        &self.imp().memory_usage_current
    }

    pub fn memory_usage_max(&self) -> &gtk::Label {
        &self.imp().memory_usage_max
    }

    pub fn shared_memory_usage_title(&self) -> &gtk::Label {
        &self.imp().shared_memory_usage_title
    }

    pub fn shared_mem_usage_current(&self) -> &gtk::Label {
        &self.imp().gtt_usage_current
    }

    pub fn shared_mem_usage_max(&self) -> &gtk::Label {
        &self.imp().gtt_usage_max
    }

    pub fn clock_speed_current(&self) -> &gtk::Label {
        &self.imp().clock_speed_current
    }

    pub fn clock_speed_separator(&self) -> &gtk::Label {
        &self.imp().clock_speed_separator
    }

    pub fn clock_speed_max(&self) -> &gtk::Label {
        &self.imp().clock_speed_max
    }

    pub fn memory_speed_current(&self) -> &gtk::Label {
        &self.imp().memory_speed_current
    }

    pub fn memory_speed_separator(&self) -> &gtk::Label {
        &self.imp().memory_speed_separator
    }

    pub fn memory_speed_max(&self) -> &gtk::Label {
        &self.imp().memory_speed_max
    }

    pub fn power_draw_current(&self) -> &gtk::Label {
        &self.imp().power_draw_current
    }

    pub fn power_draw_separator(&self) -> &gtk::Label {
        &self.imp().power_draw_separator
    }

    pub fn power_draw_max(&self) -> &gtk::Label {
        &self.imp().power_draw_max
    }

    pub fn encode_percent(&self) -> &gtk::Label {
        &self.imp().encode_percent
    }

    pub fn decode_percent(&self) -> &gtk::Label {
        &self.imp().decode_percent
    }

    pub fn temperature(&self) -> &gtk::Label {
        &self.imp().temperature
    }

    pub fn opengl_version(&self) -> &gtk::Label {
        &self.imp().opengl_version
    }

    pub fn vulkan_version(&self) -> &gtk::Label {
        &self.imp().vulkan_version
    }

    pub fn pcie_speed_label(&self) -> &gtk::Label {
        &self.imp().pcie_speed_label
    }

    pub fn pcie_speed(&self) -> &gtk::Label {
        &self.imp().pcie_speed
    }

    pub fn max_pcie_speed_label(&self) -> &gtk::Label {
        &self.imp().max_pcie_speed_label
    }

    pub fn max_pcie_speed(&self) -> &gtk::Label {
        &self.imp().max_pcie_speed
    }

    pub fn pci_addr(&self) -> &gtk::Label {
        &self.imp().pci_addr
    }

    pub fn box_temp(&self) -> &gtk::Box {
        &self.imp().box_temp
    }

    pub fn box_mem_speed(&self) -> &gtk::Box {
        &self.imp().box_mem_speed
    }

    pub fn box_power_draw(&self) -> &gtk::Box {
        &self.imp().box_power_draw
    }

    pub fn encode_label(&self) -> &gtk::Label {
        &self.imp().encode_label
    }

    pub fn legend_encode(&self) -> &gtk::Picture {
        &self.imp().legend_encode
    }

    pub fn legend_decode(&self) -> &gtk::Picture {
        &self.imp().legend_decode
    }

    pub fn legend_vram(&self) -> &gtk::Picture {
        &self.imp().legend_vram
    }

    pub fn legend_gtt(&self) -> &gtk::Picture {
        &self.imp().legend_gtt
    }

    pub fn legend_clock(&self) -> &gtk::Picture {
        &self.imp().legend_gtt
    }

    pub fn legend_clock_mem(&self) -> &gtk::Picture {
        &self.imp().legend_gtt
    }
}

impl GpuDetails {
    pub fn set_legend_clock_visible(&self, v: bool) {
        self.imp().legend_clock.set_visible(v)
    }

    pub fn set_legend_memory_visible(&self, v: bool) {
        self.imp().legend_vram.set_visible(v)
    }

    pub fn set_legend_enc_dec_visible(&self, v: bool) {
        self.imp().legend_encode.set_visible(v)
    }

    pub fn is_both_clocks_visible(&self) -> bool {
        self.clock_speed_available() || self.memory_speed_available()
    }
}
