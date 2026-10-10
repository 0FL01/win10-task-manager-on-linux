/* performance_page/cpu.rs
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

use adw::subclass::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::glib::g_critical;
use gtk::{gio, glib, prelude::*};

use crate::performance_page::widgets::{
    AnimationFrame, DatasetGroup, FillingSettings, GraphWidget, GraphWidgetSettingsExt,
    RoundingSettings, ScalingSettings,
};
use crate::DataType;
use crate::{application::INTERVAL_STEP, i18n::*, settings, to_short_human_readable_time};

use super::PageExt;
use crate::performance_page::fan::{TEMPERATURE_HIGH_WATERMARK, TEMPERATURE_LOW_WATERMARK};

const GRAPH_NONE: i32 = 0;
const GRAPH_POWER: i32 = 1;
const GRAPH_POWER_DATASET: usize = 0;
const GRAPH_CLOCK: i32 = 2;
const GRAPH_CLOCK_DATASET: usize = 1;
const GRAPH_TEMPERATURE: i32 = 3;
const GRAPH_TEMPERATURE_DATASET: usize = 2;

mod imp {
    use super::*;

    const GRAPH_SELECTION_OVERALL: i32 = 1;
    const GRAPH_SELECTION_ALL: i32 = 2;
    const GRAPH_SELECTION_ALL_THREADS: i32 = 3;
    const GRAPH_SELECTION_ALL_THREADS_STACKED: i32 = 4;

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePageCpu)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/cpu.ui")]
    pub struct PerformancePageCpu {
        #[template_child]
        pub cpu_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub usage_graphs: TemplateChild<gtk::Grid>,
        #[template_child]
        pub graph_max_duration: TemplateChild<gtk::Label>,
        #[template_child]
        pub context_menu: TemplateChild<gtk::Popover>,

        #[template_child]
        pub bottom_graph_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub bottom_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub bottom_graph_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub bottom_graph_total: TemplateChild<gtk::Label>,

        #[property(get, set = Self::set_base_color)]
        base_color: Cell<gtk::gdk::RGBA>,
        #[property(get, set)]
        summary_mode: Cell<bool>,

        pub graph_widgets: Cell<Vec<GraphWidget>>,

        #[property(get = Self::infobar_content, type = Option < gtk::Widget >)]
        pub infobar_content: OnceCell<gtk::Box>,
        pub power_row: OnceCell<gtk::Box>,

        pub utilization: OnceCell<gtk::Label>,
        pub speed: OnceCell<gtk::Label>,
        pub speed_label: OnceCell<gtk::Label>,
        pub speed_indicator: OnceCell<gtk::Label>,
        pub power_draw: OnceCell<gtk::Label>,
        pub processes: OnceCell<gtk::Label>,
        pub threads: OnceCell<gtk::Label>,
        pub handles: OnceCell<gtk::Label>,
        pub uptime: OnceCell<gtk::Label>,
        pub temperature: OnceCell<gtk::Label>,
        pub base_speed: OnceCell<gtk::Label>,
        pub sockets: OnceCell<gtk::Label>,
        pub virt_proc: OnceCell<gtk::Label>,
        pub virtualization: OnceCell<gtk::Label>,
        pub virt_machine: OnceCell<gtk::Label>,
        pub l1_cache: OnceCell<gtk::Label>,
        pub l2_cache: OnceCell<gtk::Label>,
        pub l3_cache: OnceCell<gtk::Label>,
        pub cpufreq_driver: OnceCell<gtk::Label>,
        pub cpufreq_driver_label: OnceCell<gtk::Label>,
        pub cpufreq_governor: OnceCell<gtk::Label>,
        pub cpufreq_governor_label: OnceCell<gtk::Label>,
        pub energy_performance_preference: OnceCell<gtk::Label>,
        pub energy_performance_preference_label: OnceCell<gtk::Label>,

        is_bogomips: Cell<bool>,
        bottom_graph_selection: Cell<i32>,

        graph_power: gio::SimpleAction,
        graph_clocks: gio::SimpleAction,
        graph_temperature: gio::SimpleAction,
        graph_none: gio::SimpleAction,

        actions: gio::SimpleActionGroup,
    }

    macro_rules! update_selection_callback {
        ($toggled_action: ident, $this: ident, $new_idx: ident, $($disabled_actions: ident),*) => {
            let this = match $this.upgrade() {
                Some(this) => this,
                None => return,
            };

            let graph_widgets: Vec<GraphWidget> = this.imp().graph_widgets.take();

            graph_widgets[0].set_visible($new_idx == GRAPH_SELECTION_OVERALL);

            if $new_idx == GRAPH_SELECTION_ALL_THREADS {
                graph_widgets[1].set_dataset_max_scale(0, 100.);
                graph_widgets[1].set_dataset_scaling(0, ScalingSettings::Fixed);
                graph_widgets[1].set_dataset_opacity(0, 100. / 255. / (graph_widgets.len() - 2) as f32);
                graph_widgets[1].set_visible(true);
            } else if $new_idx == GRAPH_SELECTION_ALL_THREADS_STACKED {
                graph_widgets[1].set_dataset_max_scale(0, 100. * (graph_widgets.len() - 2) as f32);
                graph_widgets[1].set_dataset_scaling(0, ScalingSettings::Stacking);
                graph_widgets[1].set_dataset_opacity(0, 100. / 255.);
                graph_widgets[1].set_visible(true);
                graph_widgets[1].set_filled(0, FillingSettings::FillToBottom);
            } else {
                graph_widgets[1].set_visible(false);
            }

            for graph_widget in graph_widgets.iter().skip(2) {
                graph_widget.set_visible($new_idx == GRAPH_SELECTION_ALL);
            }

            $($disabled_actions.set_state(&glib::Variant::from(false)));*;

            $toggled_action.set_state(&glib::Variant::from(true));

            settings!()
                .set_int("performance-page-cpu-graph", $new_idx)
                .unwrap_or_else(|_| {
                    g_critical!(
                                "MissionCenter::PerformancePage",
                                "Failed to save selected CPU graph"
                            );
                });

            this.imp().graph_widgets.set(graph_widgets);
        }
    }

    impl Default for PerformancePageCpu {
        fn default() -> Self {
            Self {
                cpu_name: Default::default(),
                usage_graphs: Default::default(),
                graph_max_duration: Default::default(),
                context_menu: Default::default(),

                bottom_graph_box: Default::default(),
                bottom_graph: Default::default(),
                bottom_graph_label: Default::default(),
                bottom_graph_total: Default::default(),

                base_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                summary_mode: Cell::new(false),

                graph_widgets: Cell::new(Vec::new()),
                bottom_graph_selection: Cell::new(0),

                is_bogomips: Cell::new(false),

                infobar_content: Default::default(),
                power_row: Default::default(),

                utilization: Default::default(),
                speed: Default::default(),
                speed_label: Default::default(),
                speed_indicator: Default::default(),
                power_draw: Default::default(),
                processes: Default::default(),
                threads: Default::default(),
                handles: Default::default(),
                uptime: Default::default(),
                temperature: Default::default(),
                base_speed: Default::default(),
                sockets: Default::default(),
                virt_proc: Default::default(),
                virtualization: Default::default(),
                virt_machine: Default::default(),
                l1_cache: Default::default(),
                l2_cache: Default::default(),
                l3_cache: Default::default(),
                cpufreq_driver: Default::default(),
                cpufreq_driver_label: Default::default(),
                cpufreq_governor: Default::default(),
                cpufreq_governor_label: Default::default(),
                energy_performance_preference: Default::default(),
                energy_performance_preference_label: Default::default(),

                graph_power: gio::SimpleAction::new_stateful(
                    "cpu_power",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_clocks: gio::SimpleAction::new_stateful(
                    "cpu_clocks",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_temperature: gio::SimpleAction::new_stateful(
                    "cpu_temperature",
                    None,
                    &glib::Variant::from(true),
                ),

                graph_none: gio::SimpleAction::new_stateful(
                    "cpu_none",
                    None,
                    &glib::Variant::from(true),
                ),

                actions: gio::SimpleActionGroup::new(),
            }
        }
    }

    impl PerformancePageCpu {
        fn set_base_color(&self, base_color: gtk::gdk::RGBA) {
            let graph_widgets = self.graph_widgets.take();
            for graph_widget in &graph_widgets {
                graph_widget.set_base_color(base_color.clone());
            }
            self.graph_widgets.set(graph_widgets);

            self.base_color.set(base_color);
        }

        fn infobar_content(&self) -> Option<gtk::Widget> {
            self.infobar_content.get().map(|ic| ic.clone().into())
        }
    }

    impl PerformancePageCpu {
        #[allow(unused)]
        fn configure_actions(this: &super::PerformancePageCpu) {
            let settings = settings!();
            let graph_selection = settings.int("performance-page-cpu-graph");
            let bottom_graph_selection = settings.int("performance-page-cpu-graph-bottom");
            let show_kernel_times = settings.boolean("performance-page-kernel-times");

            this.insert_action_group("graph", Some(&this.imp().actions));

            let overall_action = gio::SimpleAction::new_stateful(
                "overall",
                None,
                &glib::Variant::from(graph_selection == GRAPH_SELECTION_OVERALL),
            );
            let all_processors_action = gio::SimpleAction::new_stateful(
                "all-processors",
                None,
                &glib::Variant::from(graph_selection == GRAPH_SELECTION_ALL),
            );
            let all_threads_action = gio::SimpleAction::new_stateful(
                "all-threads",
                None,
                &glib::Variant::from(graph_selection == GRAPH_SELECTION_ALL_THREADS),
            );
            let stacked_threads_action = gio::SimpleAction::new_stateful(
                "all-threads-stacked",
                None,
                &glib::Variant::from(graph_selection == GRAPH_SELECTION_ALL_THREADS_STACKED),
            );

            let ova = overall_action.clone();
            let ata = all_threads_action.clone();
            let sta = stacked_threads_action.clone();

            all_processors_action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    update_selection_callback!(action, this, GRAPH_SELECTION_ALL, ova, ata, sta);
                }
            });
            this.imp().actions.add_action(&all_processors_action);

            let apa = all_processors_action.clone();
            let ata = all_threads_action.clone();
            let sta = stacked_threads_action.clone();

            overall_action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    update_selection_callback!(
                        action,
                        this,
                        GRAPH_SELECTION_OVERALL,
                        apa,
                        ata,
                        sta
                    );
                }
            });
            this.imp().actions.add_action(&overall_action);

            let apa = all_processors_action.clone();
            let ova = overall_action.clone();
            let sta = stacked_threads_action.clone();

            all_threads_action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    update_selection_callback!(
                        action,
                        this,
                        GRAPH_SELECTION_ALL_THREADS,
                        apa,
                        ova,
                        sta
                    );
                }
            });
            this.imp().actions.add_action(&all_threads_action);

            let apa = all_processors_action.clone();
            let ova = overall_action.clone();
            let ata = all_threads_action.clone();

            stacked_threads_action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    update_selection_callback!(
                        action,
                        this,
                        GRAPH_SELECTION_ALL_THREADS_STACKED,
                        apa,
                        ova,
                        ata
                    );
                }
            });
            this.imp().actions.add_action(&stacked_threads_action);

            let action = gio::SimpleAction::new_stateful(
                "kernel_times",
                None,
                &glib::Variant::from(show_kernel_times),
            );
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    let graph_widgets = this.imp().graph_widgets.take();

                    let visible = !action
                        .state()
                        .and_then(|v| v.get::<bool>())
                        .unwrap_or(false);

                    graph_widgets[0].set_data_visible(1, visible);
                    for graph_widget in graph_widgets.iter().skip(2) {
                        graph_widget.set_data_visible(1, visible);
                    }

                    action.set_state(&glib::Variant::from(visible));

                    settings!()
                        .set_boolean("performance-page-kernel-times", visible)
                        .unwrap_or_else(|_| {
                            g_critical!(
                                "MissionCenter::PerformancePage",
                                "Failed to save kernel times setting"
                            );
                        });

                    this.imp().graph_widgets.set(graph_widgets);
                }
            });
            this.imp().actions.add_action(&action);

            let action = gio::SimpleAction::new("copy", None);
            action.connect_activate({
                let this = this.downgrade();
                move |_, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    let clipboard = this.clipboard();
                    clipboard.set_text(this.imp().data_summary().as_str());
                }
            });
            this.imp().actions.add_action(&action);

            let action = &this.imp().graph_power;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_graph_settings(GRAPH_POWER));
            this.imp().actions.add_action(action);

            let action = &this.imp().graph_clocks;
            action.set_enabled(true);
            action.connect_activate(move |_, _| Self::set_graph_settings(GRAPH_CLOCK));
            this.imp().actions.add_action(action);

            let action = &this.imp().graph_temperature;
            action.set_enabled(false);
            action.connect_activate(move |_, _| Self::set_graph_settings(GRAPH_TEMPERATURE));
            this.imp().actions.add_action(action);

            let action = &this.imp().graph_none;
            action.set_enabled(true);
            action.connect_activate(move |_, _| Self::set_graph_settings(GRAPH_NONE));
            this.imp().actions.add_action(action);

            settings.connect_changed(Some("performance-page-cpu-graph-bottom"), {
                let this = this.imp().obj().downgrade();
                move |settings, _| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();

                        let cpu_graph_bottom = settings.int("performance-page-cpu-graph-bottom");
                        this.bottom_graph_selection.set(cpu_graph_bottom);

                        this.set_graph_states(cpu_graph_bottom);
                        this.set_graph(cpu_graph_bottom);
                    }
                }
            });

            this.imp()
                .bottom_graph_selection
                .set(bottom_graph_selection);
        }

        fn configure_context_menu(this: &super::PerformancePageCpu) {
            let right_click_controller = gtk::GestureClick::new();
            right_click_controller.set_button(3); // Secondary click (AKA right click)
            right_click_controller.connect_released({
                let this = this.downgrade();
                move |_click, _n_press, x, y| {
                    if let Some(this) = this.upgrade() {
                        this.imp()
                            .context_menu
                            .set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                                x.round() as i32,
                                y.round() as i32,
                                1,
                                1,
                            )));
                        this.imp().context_menu.popup();
                    }
                }
            });
            this.add_controller(right_click_controller);
        }
    }

    impl PerformancePageCpu {
        pub fn set_static_information(
            this: &super::PerformancePageCpu,
            readings: &crate::magpie_client::Readings,
        ) -> bool {
            let this = this.imp();

            let static_cpu_info = &readings.cpu;

            let mut cpu_bottom_graph = settings!().int("performance-page-cpu-graph-bottom");

            this.cpu_name
                .set_text(static_cpu_info.name.as_ref().unwrap_or(&i18n("Unknown")));

            this.populate_usage_graphs(static_cpu_info.core_usage_percent.len());

            // Check if we're using BogoMIPS instead of real frequency
            this.is_bogomips.set(
                static_cpu_info
                    .frequency_driver
                    .as_ref()
                    .map(|d| d.as_str() == "bogomips")
                    .unwrap_or(false),
            );

            if let Some(base_speed) = this.base_speed.get() {
                if let Some(base_frequency) = static_cpu_info.base_freq_khz {
                    if this.is_bogomips.get() {
                        let freq = base_frequency as f32 / 1000.;
                        base_speed.set_text(&format!("{:.2} BogoMIPS", freq));

                        this.bottom_graph
                            .set_dataset_max_scale(GRAPH_CLOCK_DATASET, freq);
                    } else {
                        let freq = base_frequency as f32 * 1000.;
                        base_speed.set_text(&crate::to_human_readable_nice(freq, &DataType::Hertz));

                        this.bottom_graph
                            .set_dataset_max_scale(GRAPH_CLOCK_DATASET, freq);
                    }
                } else {
                    base_speed.set_text(&i18n("Unknown"));
                }
            }

            if this.is_bogomips.get() {
                if let Some(speed) = this.speed.get() {
                    // Display raw BogoMIPS value with tooltip explanation
                    let tooltip_text = i18n_f(
                        "Real CPU frequency data is unavailable (common in VMs, containers, or cloud instances). Displaying BogoMIPS value as a fallback. BogoMIPS is a delay loop calibration metric, not a true frequency measurement.",
                        &[]
                    );
                    speed.set_tooltip_text(Some(&tooltip_text));
                    // Show the "(?)" indicator and set tooltips on label and indicator
                    if let Some(indicator) = this.speed_indicator.get() {
                        indicator.set_visible(true);
                        indicator.set_tooltip_text(Some(&tooltip_text));
                    }

                    if let Some(label) = this.speed_label.get() {
                        label.set_tooltip_text(Some(&tooltip_text));
                    }
                }
            }

            if let Some(virtualization) = this.virtualization.get() {
                if let Some(vt) = static_cpu_info.virtualization_technology.as_ref() {
                    virtualization.set_text(vt.as_ref());
                } else {
                    virtualization.set_text(&i18n("Unsupported"));
                }
            }

            if let Some(virt_machine) = this.virt_machine.get() {
                if let Some(is_vm) = static_cpu_info.is_virtual_machine {
                    if is_vm {
                        virt_machine.set_text(&i18n("Yes"));
                    } else {
                        virt_machine.set_text(&i18n("No"));
                    }
                } else {
                    virt_machine.set_text(&i18n("Unknown"));
                }
            }

            if let Some(sockets) = this.sockets.get() {
                if let Some(socket_count) = static_cpu_info.socket_count {
                    sockets.set_text(&format!("{}", socket_count));
                } else {
                    sockets.set_text(&i18n("Unknown"));
                }
            }

            let l1_cache_size = if let Some(size) = static_cpu_info.l1_combined_cache_bytes {
                crate::to_human_readable_nice(size as f32, &DataType::MemoryBytes)
            } else {
                i18n("N/A")
            };
            if let Some(l1_cache) = this.l1_cache.get() {
                l1_cache.set_text(&l1_cache_size);
            }

            let l2_cache_size = if let Some(size) = static_cpu_info.l2_cache_bytes {
                crate::to_human_readable_nice(size as f32, &DataType::MemoryBytes)
            } else {
                i18n("N/A")
            };
            if let Some(l2_cache) = this.l2_cache.get() {
                l2_cache.set_text(&l2_cache_size);
            }

            let l3_cache_size = if let Some(size) = static_cpu_info.l3_cache_bytes {
                crate::to_human_readable_nice(size as f32, &DataType::MemoryBytes)
            } else {
                i18n("N/A")
            };
            if let Some(l3_cache) = this.l3_cache.get() {
                l3_cache.set_text(&l3_cache_size);
            }

            let _ = if let Some(size) = static_cpu_info.l4_cache_bytes {
                crate::to_human_readable_nice(size as f32, &DataType::MemoryBytes)
            } else {
                i18n("N/A")
            };

            if static_cpu_info.power_draw_w.is_some() {
                this.graph_power.set_enabled(true);
            } else if cpu_bottom_graph == GRAPH_POWER {
                cpu_bottom_graph = GRAPH_TEMPERATURE;
                Self::set_graph_settings(cpu_bottom_graph);
            }

            if static_cpu_info.temperature_celsius.is_some() {
                this.graph_temperature.set_enabled(true);
            } else if let Some(temp) = this.temperature.get() {
                temp.set_visible(false);

                if cpu_bottom_graph == GRAPH_TEMPERATURE {
                    cpu_bottom_graph = GRAPH_CLOCK;
                    Self::set_graph_settings(cpu_bottom_graph);
                }
            }

            this.set_graph_states(cpu_bottom_graph);
            this.set_graph(cpu_bottom_graph);

            true
        }

        pub fn update_readings(
            this: &super::PerformancePageCpu,
            readings: &crate::magpie_client::Readings,
        ) -> bool {
            let this = this.imp();

            let dynamic_cpu_info = &readings.cpu;
            let core_count = dynamic_cpu_info.core_usage_percent.len();

            // Turns out cores can come and go at runtime
            let mut graph_widgets = this.graph_widgets.take();
            if graph_widgets.len() != core_count + 2 {
                this.graph_widgets.set(graph_widgets);
                this.populate_usage_graphs(core_count);
                graph_widgets = this.graph_widgets.take();
            }

            // Update global CPU graph
            graph_widgets[0].add_data_point(vec![
                vec![dynamic_cpu_info.total_usage_percent],
                vec![dynamic_cpu_info.kernel_usage_percent],
            ]);
            graph_widgets[1].add_data_point(vec![dynamic_cpu_info.core_usage_percent.clone()]);

            // Update per-core graphs
            let per_core = dynamic_cpu_info
                .core_usage_percent
                .iter()
                .zip(dynamic_cpu_info.core_kernel_usage_percent.iter());
            for (graph_widget, (usage, kernel)) in graph_widgets.iter_mut().skip(2).zip(per_core) {
                graph_widget.add_data_point(vec![vec![*usage], vec![*kernel]]);
            }

            this.graph_widgets.set(graph_widgets);

            if let Some(utilization) = this.utilization.get() {
                utilization.set_text(&format!(
                    "{}%",
                    dynamic_cpu_info.total_usage_percent.round()
                ));
            }

            if let Some(speed) = this.speed.get() {
                if this.is_bogomips.get() {
                    let freq = dynamic_cpu_info.current_frequency_mhz as f32;

                    this.bottom_graph
                        .add_single_data_point(GRAPH_CLOCK_DATASET, vec![freq]);

                    speed.set_text(&format!("{:.2} BogoMIPS", freq));
                } else {
                    let freq = dynamic_cpu_info.current_frequency_mhz as f32 * 1000. * 1000.;

                    this.bottom_graph
                        .add_single_data_point(GRAPH_CLOCK_DATASET, vec![freq]);

                    speed.set_text(&crate::to_human_readable_nice(freq, &DataType::Hertz));
                }
            }

            if let Some(power_draw) = this.power_draw.get() {
                if let Some(power_draw_num) = dynamic_cpu_info.power_draw_w {
                    this.bottom_graph
                        .add_single_data_point(GRAPH_POWER_DATASET, vec![power_draw_num]);

                    power_draw.set_text(&crate::to_human_readable_nice(
                        power_draw_num,
                        &DataType::Watts,
                    ))
                } else {
                    if let Some(power_row) = this.power_row.get() {
                        power_row.set_visible(false)
                    }
                }
            }
            if let Some(processes) = this.processes.get() {
                processes.set_text(&format!("{}", dynamic_cpu_info.total_process_count));
            }

            if let Some(threads) = this.threads.get() {
                threads.set_text(&format!("{}", dynamic_cpu_info.total_thread_count));
            }

            if let Some(handles) = this.handles.get() {
                handles.set_text(&format!("{}", dynamic_cpu_info.total_handle_count));
            }

            let uptime = dynamic_cpu_info.uptime_seconds;
            let days = uptime / 86400;
            let hours = (uptime % 86400) / 3600;
            let minutes = (uptime % 3600) / 60;
            let seconds = uptime % 60;

            if let Some(uptime) = this.uptime.get() {
                uptime.set_text(&format!(
                    "{:02}:{:02}:{:02}:{:02}",
                    days, hours, minutes, seconds
                ));
            }

            if let (Some(cpufreq_driver), Some(cpufreq_driver_label)) =
                (this.cpufreq_driver.get(), this.cpufreq_driver_label.get())
            {
                if let Some(governor) = dynamic_cpu_info.frequency_driver.as_ref() {
                    cpufreq_driver.set_text(governor.as_ref());
                } else {
                    cpufreq_driver.set_visible(false);
                    cpufreq_driver_label.set_visible(false);
                }
            }

            if let (Some(cpufreq_governor), Some(cpufreq_governor_label)) = (
                this.cpufreq_governor.get(),
                this.cpufreq_governor_label.get(),
            ) {
                if let Some(governor) = dynamic_cpu_info.frequency_governor.as_ref() {
                    cpufreq_governor.set_text(governor.as_ref());
                } else {
                    cpufreq_governor.set_visible(false);
                    cpufreq_governor_label.set_visible(false);
                }
            }

            if let (
                Some(energy_performance_preference),
                Some(energy_performance_preference_label),
            ) = (
                this.energy_performance_preference.get(),
                this.energy_performance_preference_label.get(),
            ) {
                if let Some(governor) = dynamic_cpu_info.power_preference.as_ref() {
                    energy_performance_preference.set_text(governor.as_ref());
                } else {
                    energy_performance_preference.set_visible(false);
                    energy_performance_preference_label.set_visible(false);
                }
            }

            if let Some(temp) = dynamic_cpu_info.temperature_celsius {
                this.bottom_graph
                    .add_single_data_point(GRAPH_TEMPERATURE_DATASET, vec![temp]);
                if let Some(temperature) = this.temperature.get() {
                    temperature.set_text(&format!("{:.0} °C", temp));
                }
            }

            true
        }

        pub fn update_animations(this: &super::PerformancePageCpu, ticks: AnimationFrame) -> bool {
            let this = this.imp();

            let widgets = this.graph_widgets.take();

            for widget in &widgets {
                widget.update_animation(ticks);
            }
            this.bottom_graph.update_animation(ticks);
            this.update_graph_total(this.bottom_graph_selection.get());

            this.graph_widgets.set(widgets);

            true
        }

        fn data_summary(&self) -> String {
            let base_speed = self
                .base_speed
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());
            let sockets = self.sockets.get().map(|v| v.label()).unwrap_or("".into());
            let virt_proc = self.virt_proc.get().map(|v| v.label()).unwrap_or("".into());
            let virtualization = self
                .virtualization
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());
            let virt_machine = self
                .virt_machine
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());
            let l1_cache = self.l1_cache.get().map(|v| v.label()).unwrap_or("".into());
            let l2_cache = self.l2_cache.get().map(|v| v.label()).unwrap_or("".into());
            let l3_cache = self.l3_cache.get().map(|v| v.label()).unwrap_or("".into());
            let cpufreq_driver = self
                .cpufreq_driver
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());
            let energy_performance_preference = self
                .energy_performance_preference
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());
            let cpufreq_governor = self
                .cpufreq_governor
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());
            let utilization = self
                .utilization
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());
            let speed = self.speed.get().map(|v| v.label()).unwrap_or("".into());
            let processes = self.processes.get().map(|v| v.label()).unwrap_or("".into());
            let threads = self.threads.get().map(|v| v.label()).unwrap_or("".into());
            let handles = self.handles.get().map(|v| v.label()).unwrap_or("".into());
            let uptime = self.uptime.get().map(|v| v.label()).unwrap_or("".into());
            let temperature = self
                .temperature
                .get()
                .map(|v| v.label())
                .unwrap_or("".into());

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
                self.cpu_name.label(),
                base_speed,
                sockets,
                virt_proc,
                virtualization,
                virt_machine,
                l1_cache,
                l2_cache,
                l3_cache,
                cpufreq_driver,
                cpufreq_governor,
                energy_performance_preference,
                utilization,
                speed,
                processes,
                threads,
                handles,
                uptime,
                temperature
            )
        }

        fn populate_usage_graphs(&self, cpu_count: usize) {
            let base_color = self.obj().base_color();

            while let Some(child) = self.usage_graphs.first_child() {
                self.usage_graphs.remove(&child);
            }

            if let Some(virt_proc) = self.virt_proc.get() {
                virt_proc.set_text(&format!("{}", cpu_count));
            }

            let col_count = Self::compute_column_count(cpu_count);

            let settings = settings!();
            let graph_selection = settings.int("performance-page-cpu-graph");
            let show_kernel_times = settings.boolean("performance-page-kernel-times");

            // Add one for overall CPU utilization
            let mut graph_widgets = vec![];

            let overall = GraphWidget::new();
            overall.connect_to_settings(&settings);
            overall.set_base_color(&base_color);
            overall.set_visible(graph_selection == GRAPH_SELECTION_OVERALL);

            let mut usage_group = DatasetGroup::new();
            usage_group.dataset_settings.scaling_settings = ScalingSettings::Fixed;
            usage_group.dataset_settings.high_watermark = 100.;
            usage_group.set_datasets(cpu_count);
            let mut kernel_group = DatasetGroup::new();
            kernel_group.dataset_settings.fill = FillingSettings::None;
            kernel_group.dataset_settings.dashed = true;
            kernel_group.dataset_settings.visible = show_kernel_times;
            kernel_group.dataset_settings.high_watermark = 100.;

            overall.add_dataset(usage_group);
            overall.add_dataset(kernel_group);

            graph_widgets.push(overall);

            let thread_wise = GraphWidget::new();
            thread_wise.connect_to_settings(&settings);
            thread_wise.set_base_color(&base_color);
            thread_wise.set_visible(
                graph_selection == GRAPH_SELECTION_ALL_THREADS
                    || graph_selection == GRAPH_SELECTION_ALL_THREADS_STACKED,
            );

            let mut usage_group = DatasetGroup::new();
            if graph_selection == GRAPH_SELECTION_ALL_THREADS_STACKED {
                usage_group.dataset_settings.scaling_settings = ScalingSettings::Stacking;
                usage_group.dataset_settings.high_watermark = 100. * cpu_count as f32;
            } else {
                usage_group.dataset_settings.scaling_settings = ScalingSettings::Fixed;
                usage_group.dataset_settings.high_watermark = 100.;
                usage_group.dataset_settings.opacity = 100. / 255. / cpu_count as f32;
            }
            usage_group.set_datasets(cpu_count);

            thread_wise.add_dataset(usage_group);

            graph_widgets.push(thread_wise);

            self.usage_graphs.attach(&graph_widgets[0], 0, 0, 1, 1);
            self.usage_graphs.attach(&graph_widgets[1], 0, 0, 1, 1);

            for i in 0..cpu_count {
                let row_idx = i / col_count;
                let col_idx = i % col_count;

                let new_graph = GraphWidget::new();
                new_graph.connect_to_settings(&settings);
                new_graph.set_base_color(&base_color);
                new_graph.set_visible(graph_selection == GRAPH_SELECTION_ALL);

                let mut usage_group = DatasetGroup::new();
                usage_group.dataset_settings.high_watermark = 100.;
                let mut kernel_group = DatasetGroup::new();
                kernel_group.dataset_settings.fill = FillingSettings::None;
                kernel_group.dataset_settings.dashed = true;
                kernel_group.dataset_settings.visible = show_kernel_times;
                kernel_group.dataset_settings.high_watermark = 100.;

                new_graph.add_dataset(usage_group);
                new_graph.add_dataset(kernel_group);

                self.usage_graphs
                    .attach(&new_graph, col_idx as i32, row_idx as i32, 1, 1);

                graph_widgets.push(new_graph);
            }

            self.graph_widgets.set(graph_widgets);
        }

        fn compute_column_count(item_count: usize) -> usize {
            if item_count <= 3 {
                return item_count;
            }

            let sqrt_item_count = (item_count as f64).sqrt().round() as usize;
            for i in sqrt_item_count..item_count.min(sqrt_item_count * 2) {
                if item_count % i == 0 {
                    return i;
                }
            }

            sqrt_item_count
        }
    }

    impl PerformancePageCpu {
        fn set_graph_states(&self, cpu_graph_bottom: i32) {
            self.graph_none
                .set_state(&glib::Variant::from(cpu_graph_bottom == GRAPH_NONE));
            self.graph_power
                .set_state(&glib::Variant::from(cpu_graph_bottom == GRAPH_POWER));
            self.graph_clocks
                .set_state(&glib::Variant::from(cpu_graph_bottom == GRAPH_CLOCK));
            self.graph_temperature
                .set_state(&glib::Variant::from(cpu_graph_bottom == GRAPH_TEMPERATURE));
        }

        fn set_graph_settings(num: i32) {
            settings!()
                .set_int("performance-page-cpu-graph-bottom", num)
                .unwrap_or_else(|_| {
                    g_critical!(
                        "MissionCenter::PerformancePage",
                        "Failed to save bottom graph state"
                    );
                });
        }

        fn set_graph(&self, num: i32) {
            self.bottom_graph
                .set_data_visible(GRAPH_POWER_DATASET, num == GRAPH_POWER);
            self.bottom_graph
                .set_data_visible(GRAPH_CLOCK_DATASET, num == GRAPH_CLOCK);
            self.bottom_graph
                .set_data_visible(GRAPH_TEMPERATURE_DATASET, num == GRAPH_TEMPERATURE);

            if num == GRAPH_NONE {
                self.bottom_graph_box.set_visible(false);
            } else {
                self.bottom_graph_box.set_visible(true);

                match num {
                    GRAPH_POWER => {
                        self.bottom_graph_label.set_text(&i18n("Power draw over "));
                    }
                    GRAPH_CLOCK => {
                        self.bottom_graph_label.set_text(&i18n("Clock speed over "));
                    }
                    GRAPH_TEMPERATURE => {
                        self.bottom_graph_label.set_text(&i18n("Temperature over "));
                    }
                    _ => {}
                };

                self.bottom_graph.force_redraw();
                self.update_graph_total(num);
            }
        }

        fn update_graph_total(&self, num: i32) {
            let text = match num {
                GRAPH_CLOCK if self.is_bogomips.get() => format!(
                    "{:.2} BogoMIPS",
                    self.bottom_graph.get_dataset_max_scale(GRAPH_CLOCK_DATASET)
                ),
                GRAPH_POWER => crate::to_human_readable_nice(
                    self.bottom_graph.get_dataset_max_scale(GRAPH_POWER_DATASET),
                    &DataType::Watts,
                ),
                GRAPH_CLOCK => crate::to_human_readable_nice(
                    self.bottom_graph.get_dataset_max_scale(GRAPH_CLOCK_DATASET),
                    &DataType::Hertz,
                ),
                GRAPH_TEMPERATURE => {
                    format!(
                        "{:.0} °C",
                        self.bottom_graph
                            .get_dataset_max_scale(GRAPH_TEMPERATURE_DATASET)
                    )
                }
                _ => String::new(),
            };
            self.bottom_graph_total.set_text(&text)
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePageCpu {
        const NAME: &'static str = "PerformancePageCpu";
        type Type = super::PerformancePageCpu;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePageCpu {
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

            let obj = self.obj();
            let this = obj.upcast_ref::<super::PerformancePageCpu>().clone();

            Self::configure_actions(&this);
            Self::configure_context_menu(&this);

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

            self.bottom_graph.add_dataset(power);
            self.bottom_graph.add_dataset(clock);
            self.bottom_graph.add_dataset(temp);

            self.bottom_graph.connect_to_settings(&settings!());

            let sidebar_content_builder = gtk::Builder::from_resource(
                "/io/missioncenter/MissionCenter/ui/performance_page/cpu_details.ui",
            );

            let _ = self.infobar_content.set(
                sidebar_content_builder
                    .object::<gtk::Box>("root")
                    .expect("Could not find `root` object in details pane"),
            );
            let _ = self.utilization.set(
                sidebar_content_builder
                    .object::<gtk::Label>("utilization")
                    .expect("Could not find `utilization` object in details pane"),
            );
            let _ = self.speed.set(
                sidebar_content_builder
                    .object::<gtk::Label>("speed")
                    .expect("Could not find `speed` object in details pane"),
            );
            let _ = self.speed_label.set(
                sidebar_content_builder
                    .object::<gtk::Label>("speed_label")
                    .expect("Could not find `speed_label` object in details pane"),
            );
            let _ = self.speed_indicator.set(
                sidebar_content_builder
                    .object::<gtk::Label>("speed_indicator")
                    .expect("Could not find `speed_indicator` object in details pane"),
            );
            let _ = self.power_draw.set(
                sidebar_content_builder
                    .object::<gtk::Label>("power_draw")
                    .expect("Could not find `power_draw` object in details pane"),
            );
            let _ = self.power_row.set(
                sidebar_content_builder
                    .object::<gtk::Box>("power_row")
                    .expect("Could not find `power_row` object in details pane"),
            );
            let _ = self.processes.set(
                sidebar_content_builder
                    .object::<gtk::Label>("processes")
                    .expect("Could not find `processes` object in details pane"),
            );
            let _ = self.threads.set(
                sidebar_content_builder
                    .object::<gtk::Label>("threads")
                    .expect("Could not find `threads` object in details pane"),
            );
            let _ = self.handles.set(
                sidebar_content_builder
                    .object::<gtk::Label>("handles")
                    .expect("Could not find `handles` object in details pane"),
            );
            let _ = self.uptime.set(
                sidebar_content_builder
                    .object::<gtk::Label>("uptime")
                    .expect("Could not find `uptime` object in details pane"),
            );
            let _ = self.temperature.set(
                sidebar_content_builder
                    .object::<gtk::Label>("temperature")
                    .expect("Could not find `temperature` object in details pane"),
            );
            let _ = self.base_speed.set(
                sidebar_content_builder
                    .object::<gtk::Label>("base_speed")
                    .expect("Could not find `base_speed` object in details pane"),
            );
            let _ = self.sockets.set(
                sidebar_content_builder
                    .object::<gtk::Label>("sockets")
                    .expect("Could not find `sockets` object in details pane"),
            );
            let _ = self.virt_proc.set(
                sidebar_content_builder
                    .object::<gtk::Label>("virt_proc")
                    .expect("Could not find `virt_proc` object in details pane"),
            );
            let _ = self.virtualization.set(
                sidebar_content_builder
                    .object::<gtk::Label>("virtualization")
                    .expect("Could not find `virtualization` object in details pane"),
            );
            let _ = self.virt_machine.set(
                sidebar_content_builder
                    .object::<gtk::Label>("virt_machine")
                    .expect("Could not find `virt_machine` object in details pane"),
            );
            let _ = self.l1_cache.set(
                sidebar_content_builder
                    .object::<gtk::Label>("l1_cache")
                    .expect("Could not find `l1_cache` object in details pane"),
            );
            let _ = self.l2_cache.set(
                sidebar_content_builder
                    .object::<gtk::Label>("l2_cache")
                    .expect("Could not find `l2_cache` object in details pane"),
            );
            let _ = self.l3_cache.set(
                sidebar_content_builder
                    .object::<gtk::Label>("l3_cache")
                    .expect("Could not find `l3_cache` object in details pane"),
            );
            let _ = self.cpufreq_driver.set(
                sidebar_content_builder
                    .object::<gtk::Label>("cpufreq_driver")
                    .expect("Could not find `cpufreq_driver` object in details pane"),
            );
            let _ = self.cpufreq_driver_label.set(
                sidebar_content_builder
                    .object::<gtk::Label>("cpufreq_driver_label")
                    .expect("Could not find `cpufreq_driver_label` object in details pane"),
            );
            let _ = self.cpufreq_governor.set(
                sidebar_content_builder
                    .object::<gtk::Label>("cpufreq_governor")
                    .expect("Could not find `cpufreq_governor` object in details pane"),
            );
            let _ = self.cpufreq_governor_label.set(
                sidebar_content_builder
                    .object::<gtk::Label>("cpufreq_governor_label")
                    .expect("Could not find `cpufreq_governor_label` object in details pane"),
            );
            let _ = self.energy_performance_preference.set(
                sidebar_content_builder
                    .object::<gtk::Label>("energy_performance_preference")
                    .expect(
                        "Could not find `energy_performance_preference` object in details pane",
                    ),
            );
            let _ = self.energy_performance_preference_label.set(
                sidebar_content_builder
                    .object::<gtk::Label>("energy_performance_preference_label")
                    .expect("Could not find `energy_performance_preference_label` object in details pane"),
            );
        }
    }

    impl WidgetImpl for PerformancePageCpu {}

    impl BoxImpl for PerformancePageCpu {}
}

glib::wrapper! {
    pub struct PerformancePageCpu(ObjectSubclass<imp::PerformancePageCpu>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PageExt for PerformancePageCpu {
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

impl PerformancePageCpu {
    pub fn new(settings: &gio::Settings) -> Self {
        let this: Self = glib::Object::builder().build();

        fn update_refresh_rate_sensitive_labels(
            this: &PerformancePageCpu,
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

        this
    }

    pub fn set_static_information(&self, readings: &crate::magpie_client::Readings) -> bool {
        imp::PerformancePageCpu::set_static_information(self, readings)
    }

    pub fn update_readings(&self, readings: &crate::magpie_client::Readings) -> bool {
        imp::PerformancePageCpu::update_readings(self, readings)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePageCpu::update_animations(self, ticks)
    }
}
