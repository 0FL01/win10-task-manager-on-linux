/* performance_page/memory.rs
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

use std::cell::{Cell, OnceCell};

use adw::{self, subclass::prelude::*};
use glib::{ParamSpec, Properties, Value};
use gtk::{gio, glib, prelude::*};

use crate::performance_page::widgets::{
    AnimationFrame, DatasetGroup, FillingSettings, GraphWidget, GraphWidgetSettingsExt,
    MemoryCompositionWidget,
};
use crate::DataType;
use crate::{application::INTERVAL_STEP, i18n::*, settings, to_short_human_readable_time};

use super::PageExt;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePageMemory)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/memory.ui")]
    pub struct PerformancePageMemory {
        #[template_child]
        pub total_ram: TemplateChild<gtk::Label>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub grid_graphs: TemplateChild<gtk::Grid>,
        #[template_child]
        pub max_graph_ram: TemplateChild<gtk::Label>,
        #[template_child]
        pub usage_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub graph_max_duration: TemplateChild<gtk::Label>,
        #[template_child]
        pub mem_composition: TemplateChild<MemoryCompositionWidget>,
        #[template_child]
        pub box_mem_composition: TemplateChild<gtk::Box>,
        #[template_child]
        pub context_menu: TemplateChild<gtk::Popover>,
        #[template_child]
        pub box_swap_space: TemplateChild<gtk::Box>,
        #[template_child]
        pub box_system_memory: TemplateChild<gtk::Box>,
        #[template_child]
        pub swap_usage_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub total_swap: TemplateChild<gtk::Label>,

        #[property(get, set)]
        base_color: Cell<gtk::gdk::RGBA>,
        #[property(get, set)]
        memory_color: Cell<gtk::gdk::RGBA>,
        #[property(get, set)]
        summary_mode: Cell<bool>,

        pub action_swap_usage: gio::SimpleAction,

        #[property(get = Self::infobar_content, type = Option < gtk::Widget >)]
        pub infobar_content: OnceCell<gtk::Grid>,

        pub tooltip_widget: OnceCell<gtk::Box>,
        pub tt_label_in_use: OnceCell<gtk::Label>,
        pub tt_label_modified: OnceCell<gtk::Label>,
        pub tt_label_standby: OnceCell<gtk::Label>,
        pub tt_label_free: OnceCell<gtk::Label>,

        pub in_use: OnceCell<gtk::Label>,
        pub available: OnceCell<gtk::Label>,
        pub committed: OnceCell<gtk::Label>,
        pub cached: OnceCell<gtk::Label>,
        pub swap_available: OnceCell<gtk::Label>,
        pub swap_used: OnceCell<gtk::Label>,
        pub compressed: OnceCell<gtk::Label>,
        pub compression_savings: OnceCell<gtk::Label>,
        pub speed: OnceCell<gtk::Label>,
        pub slots_used: OnceCell<gtk::Label>,
        pub form_factor: OnceCell<gtk::Label>,
        pub ram_type: OnceCell<gtk::Label>,

        pub legend_used: OnceCell<gtk::Picture>,
        pub legend_commited: OnceCell<gtk::Picture>,
    }

    impl Default for PerformancePageMemory {
        fn default() -> Self {
            Self {
                total_ram: Default::default(),
                toast_overlay: Default::default(),
                grid_graphs: Default::default(),
                max_graph_ram: Default::default(),
                usage_graph: Default::default(),
                graph_max_duration: Default::default(),
                mem_composition: Default::default(),
                box_mem_composition: Default::default(),
                context_menu: Default::default(),
                box_swap_space: Default::default(),
                box_system_memory: Default::default(),
                swap_usage_graph: Default::default(),
                total_swap: Default::default(),

                base_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                memory_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                summary_mode: Cell::new(false),

                action_swap_usage: gio::SimpleAction::new_stateful(
                    "swap_usage",
                    None,
                    &true.to_variant(),
                ),

                infobar_content: Default::default(),

                tooltip_widget: Default::default(),
                tt_label_in_use: Default::default(),
                tt_label_modified: Default::default(),
                tt_label_standby: Default::default(),
                tt_label_free: Default::default(),

                in_use: Default::default(),
                available: Default::default(),
                committed: Default::default(),
                cached: Default::default(),
                swap_available: Default::default(),
                swap_used: Default::default(),
                compressed: Default::default(),
                compression_savings: Default::default(),
                speed: Default::default(),
                slots_used: Default::default(),
                form_factor: Default::default(),
                ram_type: Default::default(),

                legend_used: Default::default(),
                legend_commited: Default::default(),
            }
        }
    }

    impl PerformancePageMemory {
        fn infobar_content(&self) -> Option<gtk::Widget> {
            self.infobar_content.get().map(|ic| ic.clone().into())
        }
    }

    impl PerformancePageMemory {
        fn set_swap_space_graph_visible(&self, visible: bool) {
            if let Some(grid_layout_manager) = self.grid_graphs.layout_manager() {
                if let Ok(layout_child) = grid_layout_manager
                    .layout_child(&*self.box_system_memory)
                    .downcast::<gtk::GridLayoutChild>()
                {
                    if visible {
                        layout_child.set_row_span(12);
                    } else {
                        layout_child.set_row_span(20);
                    }
                }
            }

            self.box_swap_space.set_visible(visible);
        }

        fn configure_actions(this: &super::PerformancePageMemory) {
            use gtk::glib::*;
            let actions = gio::SimpleActionGroup::new();
            this.insert_action_group("graph", Some(&actions));

            let settings = settings!();
            let show_memory_composition =
                settings.boolean("performance-page-memory-composition-visible");
            let show_swap = settings.boolean("performance-page-memory-swap-visible");

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
            actions.add_action(&action);

            let action = gio::SimpleAction::new_stateful(
                "memory_composition",
                None,
                &glib::Variant::from(show_memory_composition),
            );
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    let mem_composition = &this.box_mem_composition;

                    let visible = !action
                        .state()
                        .and_then(|v| v.get::<bool>())
                        .unwrap_or(false);

                    mem_composition.set_visible(visible);

                    action.set_state(&glib::Variant::from(visible));

                    settings!()
                        .set_boolean("performance-page-memory-composition-visible", visible)
                        .unwrap_or_else(|_| {
                            g_critical!(
                                "MissionCenter::PerformancePage",
                                "Failed to save show composition graph"
                            );
                        });
                }
            });
            actions.add_action(&action);

            let action = &this.imp().action_swap_usage;
            action.set_state(&show_swap.to_variant());
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    let visible = !action
                        .state()
                        .and_then(|v| v.get::<bool>())
                        .unwrap_or(false);

                    action.set_state(&glib::Variant::from(visible));
                    this.set_swap_space_graph_visible(visible);

                    let settings = settings!();

                    // The action might be triggered by setting the state from DConf (or similar)
                    // Short-circuit if the state is already set
                    let setting = settings.boolean("performance-page-memory-swap-visible");
                    if setting != visible {
                        let _ =
                            settings.set_boolean("performance-page-memory-swap-visible", visible);
                    }
                }
            });
            actions.add_action(action);
        }

        fn configure_context_menu(this: &super::PerformancePageMemory) {
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

    impl PerformancePageMemory {
        pub fn set_static_information(
            this: &super::PerformancePageMemory,
            readings: &crate::magpie_client::Readings,
        ) -> bool {
            let this = this.imp();

            let settings = &settings!();
            let show_memory_composition =
                settings.boolean("performance-page-memory-composition-visible");

            this.usage_graph
                .set_all_datasets_max_scale(readings.mem_info.mem_total as f32);
            this.swap_usage_graph
                .set_all_datasets_max_scale(readings.mem_info.swap_total as f32);

            if !show_memory_composition {
                this.box_mem_composition.set_visible(false);
            }

            if let Some(legend_commited) = this.legend_commited.get() {
                legend_commited
                    .set_resource(Some("/io/missioncenter/MissionCenter/line-dashed-mem.svg"));
            }
            if let Some(legend_used) = this.legend_used.get() {
                legend_used
                    .set_resource(Some("/io/missioncenter/MissionCenter/line-solid-mem.svg"));
            }

            let total_mem = crate::to_human_readable_nice(
                readings.mem_info.mem_total as _,
                &DataType::MemoryBytes,
            );
            this.total_ram.set_text(&total_mem);
            this.max_graph_ram.set_text(&total_mem);

            let total_swap = crate::to_human_readable_nice(
                readings.mem_info.swap_total as _,
                &DataType::MemoryBytes,
            );
            this.total_swap.set_text(&total_swap);

            let show_swap = settings.boolean("performance-page-memory-swap-visible");
            if !show_swap {
                this.set_swap_space_graph_visible(false);
            }

            let mem_module_count = readings.mem_devices.len();
            if mem_module_count > 0 {
                if let Some(sp) = this.speed.get() {
                    if readings.mem_devices[0].speed != 0 {
                        sp.set_text(&format!("{} MT/s", readings.mem_devices[0].speed));
                    }
                }
                if let Some(su) = this.slots_used.get() {
                    if readings.mem_info.max_devices > 0 {
                        su.set_text(&i18n_f(
                            "{} of {}",
                            &[
                                &format!("{}", mem_module_count),
                                &format!("{}", readings.mem_info.max_devices),
                            ],
                        ));
                    } else {
                        su.set_text(&format!("{}", mem_module_count));
                    }
                }
                if let Some(ff) = this.form_factor.get() {
                    if readings.mem_devices[0].form_factor != "" {
                        ff.set_text(&format!("{}", readings.mem_devices[0].form_factor));
                    }
                }
                if let Some(rt) = this.ram_type.get() {
                    if readings.mem_devices[0].ram_type != "" {
                        rt.set_text(&format!("{}", readings.mem_devices[0].ram_type));
                    }
                }
            } else {
                this.toast_overlay.add_toast(adw::Toast::new(&i18n(
                    "Getting additional memory information failed",
                )))
            }

            true
        }

        pub fn update_readings(
            this: &super::PerformancePageMemory,
            readings: &crate::magpie_client::Readings,
        ) -> bool {
            let this = this.imp();
            let mem_info = &readings.mem_info;

            // https://gitlab.com/procps-ng/procps/-/blob/master/library/meminfo.c?ref_type=heads#L736
            let mem_avail = if mem_info.mem_available > mem_info.mem_total {
                mem_info.mem_free
            } else {
                mem_info.mem_available
            };
            let used = mem_info.mem_total.saturating_sub(mem_avail);
            let standby = mem_info.mem_total.saturating_sub(used + mem_info.mem_free);
            this.usage_graph.add_data_point(vec![
                vec![mem_info.committed as _],
                vec![mem_info.dirty as _],
                vec![used as _],
            ]);

            let total_mem = crate::to_human_readable_nice(
                readings.mem_info.mem_total as _,
                &DataType::MemoryBytes,
            );
            this.total_ram.set_text(&total_mem);

            let max_ram = crate::to_human_readable_nice(
                this.usage_graph.get_dataset_max_scale(0),
                &DataType::MemoryBytes,
            );
            this.max_graph_ram.set_text(&max_ram);

            let swap_used = mem_info.swap_total.saturating_sub(mem_info.swap_free);
            this.swap_usage_graph
                .add_data_point(vec![vec![swap_used as _]]);

            this.mem_composition.update_memory_information(mem_info);

            let used = crate::to_human_readable_nice(used as _, &DataType::MemoryBytes);
            if let Some(iu) = this.in_use.get() {
                iu.set_text(&used);
            }

            let available =
                crate::to_human_readable_nice(mem_info.mem_available as _, &DataType::MemoryBytes);
            if let Some(av) = this.available.get() {
                av.set_text(&available);
            }

            let committed =
                crate::to_human_readable_nice(mem_info.committed as _, &DataType::MemoryBytes);
            if let Some(cm) = this.committed.get() {
                cm.set_text(&committed);
            }

            let cached =
                crate::to_human_readable_nice(mem_info.cached as _, &DataType::MemoryBytes);
            if let Some(ch) = this.cached.get() {
                ch.set_text(&cached);
            }

            if mem_info.swap_total == 0 {
                this.action_swap_usage.set_enabled(false);
                this.set_swap_space_graph_visible(false);

                if let Some(sa) = this.swap_available.get() {
                    sa.set_visible(false);
                }

                if let Some(su) = this.swap_used.get() {
                    su.set_visible(false);
                }
            } else {
                let swap_available =
                    crate::to_human_readable_nice(mem_info.swap_total as _, &DataType::MemoryBytes);
                if let Some(sa) = this.swap_available.get() {
                    sa.set_visible(true);
                    sa.set_text(&swap_available);
                }

                let swap_used = crate::to_human_readable_nice(
                    mem_info.swap_total.saturating_sub(mem_info.swap_free) as _,
                    &DataType::MemoryBytes,
                );
                if let Some(su) = this.swap_used.get() {
                    su.set_visible(true);
                    su.set_text(&swap_used);
                }
            }

            // Memory compression statistics (zswap + zram combined)
            let zswap_original = mem_info.zswapped;
            let zswap_compressed = mem_info.zswap;
            let zram_original = mem_info.zram_orig_size;
            let zram_compressed = mem_info.zram_compr_size;

            let total_original = zswap_original + zram_original;
            let total_compressed = zswap_compressed + zram_compressed;

            // Only show compression stats if there's meaningful compression (>= 1 MiB)
            const MIN_COMPRESSION_THRESHOLD: u64 = 1024 * 1024;
            if total_original >= MIN_COMPRESSION_THRESHOLD {
                let compressed_str =
                    crate::to_human_readable_nice(total_compressed as _, &DataType::MemoryBytes);
                if let Some(c) = this.compressed.get() {
                    c.set_visible(true);
                    c.set_text(&compressed_str);
                }

                let savings = total_original.saturating_sub(total_compressed);
                let savings_str =
                    crate::to_human_readable_nice(savings as _, &DataType::MemoryBytes);
                if let Some(cs) = this.compression_savings.get() {
                    cs.set_visible(true);
                    cs.set_text(&savings_str);
                }
            } else {
                // Hide compression stats if no compression is active
                if let Some(c) = this.compressed.get() {
                    c.set_visible(false);
                }
                if let Some(cs) = this.compression_savings.get() {
                    cs.set_visible(false);
                }
            }

            let free =
                crate::to_human_readable_nice(mem_info.mem_free as _, &DataType::MemoryBytes);
            let dirty = crate::to_human_readable_nice(mem_info.dirty as _, &DataType::MemoryBytes);
            let standby = crate::to_human_readable_nice(standby as _, &DataType::MemoryBytes);

            if let Some(l) = this.tt_label_in_use.get() {
                l.set_text(&used)
            }

            if let Some(l) = this.tt_label_modified.get() {
                l.set_text(&dirty)
            }

            if let Some(l) = this.tt_label_standby.get() {
                l.set_text(&standby)
            }

            if let Some(l) = this.tt_label_free.get() {
                l.set_text(&free)
            }

            let total_swap = crate::to_human_readable_nice(
                readings.mem_info.swap_total as _,
                &DataType::MemoryBytes,
            );
            this.total_swap.set_text(&total_swap);

            true
        }

        pub fn update_animations(
            this: &super::PerformancePageMemory,
            ticks: AnimationFrame,
        ) -> bool {
            let this = this.imp();

            this.usage_graph.update_animation(ticks);
            this.swap_usage_graph.update_animation(ticks);

            true
        }

        fn data_summary(&self) -> String {
            let unknown = i18n("Unknown");
            let unknown = unknown.as_str();
            let na = i18n("N/A");
            let na = na.as_str();

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
                self.total_ram.label(),
                self.in_use
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.available
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.committed
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.cached
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.swap_available
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.swap_used
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.compressed
                    .get()
                    .map(|l| if l.is_visible() { l.label() } else { na.into() })
                    .unwrap_or(na.into()),
                self.compression_savings
                    .get()
                    .map(|l| if l.is_visible() { l.label() } else { na.into() })
                    .unwrap_or(na.into()),
                self.speed
                    .get()
                    .map(|l| {
                        if !l.uses_markup() {
                            l.label()
                        } else {
                            unknown.into()
                        }
                    })
                    .unwrap_or(unknown.into()),
                self.slots_used
                    .get()
                    .map(|l| {
                        if !l.uses_markup() {
                            l.label()
                        } else {
                            unknown.into()
                        }
                    })
                    .unwrap_or(unknown.into()),
                self.form_factor
                    .get()
                    .map(|l| {
                        if !l.uses_markup() {
                            l.label()
                        } else {
                            unknown.into()
                        }
                    })
                    .unwrap_or(unknown.into()),
                self.ram_type
                    .get()
                    .map(|l| {
                        if !l.uses_markup() {
                            l.label()
                        } else {
                            unknown.into()
                        }
                    })
                    .unwrap_or(unknown.into()),
            )
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePageMemory {
        const NAME: &'static str = "PerformancePageMemory";
        type Type = super::PerformancePageMemory;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            MemoryCompositionWidget::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePageMemory {
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

            let this = self.obj().clone();

            let mut dataset_a = DatasetGroup::new();
            dataset_a.dataset_settings.fill = FillingSettings::None;
            dataset_a.dataset_settings.dashed = true;

            let mut dataset_b = DatasetGroup::new();
            dataset_b.dataset_settings.fill = FillingSettings::None;

            let dataset_c = DatasetGroup::new();

            self.usage_graph.add_dataset(dataset_a);
            self.usage_graph.add_dataset(dataset_b);
            self.usage_graph.add_dataset(dataset_c);

            self.usage_graph.connect_datasets(0, 1);
            self.usage_graph.connect_datasets(1, 2);
            self.usage_graph.connect_datasets(2, 0);

            self.usage_graph.connect_to_settings(&settings!());

            let swap_dataset = DatasetGroup::new();

            self.swap_usage_graph.add_dataset(swap_dataset);
            self.swap_usage_graph.connect_to_settings(&settings!());

            Self::configure_actions(&this);
            Self::configure_context_menu(&this);

            self.box_system_memory.connect_query_tooltip({
                let this = self.obj().downgrade();
                move |_, _, _, _, tooltip| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return false,
                    };
                    let this = this.imp();

                    if let Some(tooltip_widget) = this.tooltip_widget.get() {
                        tooltip.set_custom(Some(tooltip_widget));
                        return true;
                    }

                    false
                }
            });

            let tooltip_content_builder = gtk::Builder::from_resource(
                "/io/missioncenter/MissionCenter/ui/performance_page/memory_info_tooltip.ui",
            );

            let _ = self.tooltip_widget.set(
                tooltip_content_builder
                    .object::<gtk::Box>("root")
                    .expect("Could not find `root` object in tooltip widget"),
            );

            let _ = self.tt_label_in_use.set(
                tooltip_content_builder
                    .object::<gtk::Label>("label_in_use")
                    .expect("Could not find `label_in_use` object in tooltip widget"),
            );

            let _ = self.tt_label_modified.set(
                tooltip_content_builder
                    .object::<gtk::Label>("label_modified")
                    .expect("Could not find `label_modified` object in tooltip widget"),
            );

            let _ = self.tt_label_standby.set(
                tooltip_content_builder
                    .object::<gtk::Label>("label_standby")
                    .expect("Could not find `label_standby` object in tooltip widget"),
            );

            let _ = self.tt_label_free.set(
                tooltip_content_builder
                    .object::<gtk::Label>("label_free")
                    .expect("Could not find `label_free` object in tooltip widget"),
            );

            let sidebar_content_builder = gtk::Builder::from_resource(
                "/io/missioncenter/MissionCenter/ui/performance_page/memory_details.ui",
            );
            let _ = self.infobar_content.set(
                sidebar_content_builder
                    .object::<gtk::Grid>("root")
                    .expect("Could not find `root` object in details pane"),
            );

            let _ = self.in_use.set(
                sidebar_content_builder
                    .object::<gtk::Label>("in_use")
                    .expect("Could not find `in_use` object in details pane"),
            );
            let _ = self.available.set(
                sidebar_content_builder
                    .object::<gtk::Label>("available")
                    .expect("Could not find `available` object in details pane"),
            );
            let _ = self.committed.set(
                sidebar_content_builder
                    .object::<gtk::Label>("committed")
                    .expect("Could not find `committed` object in details pane"),
            );
            let _ = self.cached.set(
                sidebar_content_builder
                    .object::<gtk::Label>("cached")
                    .expect("Could not find `cached` object in details pane"),
            );
            let _ = self.swap_available.set(
                sidebar_content_builder
                    .object::<gtk::Label>("swap_available")
                    .expect("Could not find `swap_available` object in details pane"),
            );
            let _ = self.swap_used.set(
                sidebar_content_builder
                    .object::<gtk::Label>("swap_used")
                    .expect("Could not find `swap_used` object in details pane"),
            );
            let _ = self.compressed.set(
                sidebar_content_builder
                    .object::<gtk::Label>("compressed")
                    .expect("Could not find `compressed` object in details pane"),
            );
            let _ = self.compression_savings.set(
                sidebar_content_builder
                    .object::<gtk::Label>("compression_savings")
                    .expect("Could not find `compression_savings` object in details pane"),
            );
            let _ = self.legend_used.set(
                sidebar_content_builder
                    .object::<gtk::Picture>("legend_used")
                    .expect("Could not find `legend_used` object in details pane"),
            );
            let _ = self.legend_commited.set(
                sidebar_content_builder
                    .object::<gtk::Picture>("legend_commited")
                    .expect("Could not find `legend_commited` object in details pane"),
            );

            let default_label = format!("{}", i18n("Unknown"));
            let default_label = default_label.as_str();

            let speed: gtk::Label = sidebar_content_builder
                .object("speed")
                .expect("Could not find `speed` object in details pane");
            speed.set_label(default_label);
            let _ = self.speed.set(speed);

            let slots_used: gtk::Label = sidebar_content_builder
                .object("slots_used")
                .expect("Could not find `slots_used` object in details pane");
            slots_used.set_label(default_label);
            let _ = self.slots_used.set(slots_used);

            let form_factor: gtk::Label = sidebar_content_builder
                .object::<gtk::Label>("form_factor")
                .expect("Could not find `form_factor` object in details pane");
            form_factor.set_label(default_label);
            let _ = self.form_factor.set(form_factor);

            let ram_type: gtk::Label = sidebar_content_builder
                .object("ram_type")
                .expect("Could not find `ram_type` object in details pane");
            ram_type.set_label(default_label);
            let _ = self.ram_type.set(ram_type);
        }
    }

    impl WidgetImpl for PerformancePageMemory {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl BoxImpl for PerformancePageMemory {}
}

glib::wrapper! {
    pub struct PerformancePageMemory(ObjectSubclass<imp::PerformancePageMemory>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PageExt for PerformancePageMemory {
    fn infobar_collapsed(&self) {
        self.imp()
            .infobar_content
            .get()
            .and_then(|ic| Some(ic.set_margin_top(10)));
    }

    fn infobar_uncollapsed(&self) {
        self.imp()
            .infobar_content
            .get()
            .and_then(|ic| Some(ic.set_margin_top(65)));
    }
}

impl PerformancePageMemory {
    pub fn new(settings: &gio::Settings) -> Self {
        let this: Self = glib::Object::builder().build();

        fn update_refresh_rate_sensitive_labels(
            this: &PerformancePageMemory,
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
        update_refresh_rate_sensitive_labels(&this, settings);

        settings.connect_changed(Some("performance-page-data-points"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        settings.connect_changed(Some("app-update-interval-u64"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        settings.connect_changed(Some("performance-page-memory-swap-visible"), {
            let this = this.downgrade();
            move |settings, _| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                let setting = settings.boolean("performance-page-memory-swap-visible");
                let action_state = this
                    .imp()
                    .action_swap_usage
                    .state()
                    .and_then(|v| v.get::<bool>())
                    .unwrap_or(false);

                // Short-circuit if the state is already set
                if setting != action_state {
                    let _ = WidgetExt::activate_action(&this, "graph.swap_usage", None);
                }
            }
        });

        this
    }

    pub fn set_static_information(&self, readings: &crate::magpie_client::Readings) -> bool {
        imp::PerformancePageMemory::set_static_information(self, readings)
    }

    pub fn update_readings(&self, readings: &crate::magpie_client::Readings) -> bool {
        imp::PerformancePageMemory::update_readings(self, readings)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePageMemory::update_animations(self, ticks)
    }
}
