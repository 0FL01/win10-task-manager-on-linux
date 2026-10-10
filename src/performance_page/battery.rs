/* src/performance_page/battery.rs
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

use std::cell::{Cell, OnceCell, RefCell};

use adw::subclass::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::{gio, glib, prelude::*};

use magpie_types::battery::Battery;

use super::widgets::{AnimationFrame, FillingSettings, GraphWidget};
use crate::application::INTERVAL_STEP;
use crate::i18n::*;
use crate::performance_page::widgets::{
    DatasetGroup, GraphWidgetSettingsExt, RoundingSettings, ScalingSettings,
};
use crate::performance_page::PageExt;
use crate::to_long_human_readable_time;
use crate::to_short_human_readable_time;
use crate::DataType;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePageBattery)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/battery.ui")]
    pub struct PerformancePageBattery {
        #[template_child]
        pub title_battery_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub title_battery_model: TemplateChild<gtk::Label>,

        #[template_child]
        pub energy_rate_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub energy_rate_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub energy_rate_max_y: TemplateChild<gtk::Label>,
        #[template_child]
        pub energy_rate_min_y: TemplateChild<gtk::Label>,
        #[template_child]
        pub energy_rate_max_duration: TemplateChild<gtk::Label>,
        #[template_child]
        pub energy_rate_box: TemplateChild<gtk::Box>,

        #[template_child]
        pub history_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub history_box: TemplateChild<gtk::Box>,

        #[template_child]
        pub context_menu: TemplateChild<gtk::Popover>,

        #[property(get = Self::name, set = Self::set_name, type = String)]
        name: RefCell<String>,
        #[property(get, set)]
        base_color: Cell<gtk::gdk::RGBA>,
        #[property(get, set)]
        summary_mode: Cell<bool>,

        #[property(get = Self::infobar_content, type = Option < gtk::Widget >)]
        pub infobar_content: OnceCell<gtk::Grid>,

        pub percentage: OnceCell<gtk::Label>,
        pub energy: OnceCell<gtk::Label>,
        pub power: OnceCell<gtk::Label>,
        pub voltage: OnceCell<gtk::Label>,
        pub voltage_box: OnceCell<gtk::Box>,
        pub time_to: OnceCell<gtk::Label>,
        pub time_to_box: OnceCell<gtk::Box>,
        pub time_to_direction: OnceCell<gtk::Label>,
        pub state: OnceCell<gtk::Label>,
        pub charge_cycles: OnceCell<gtk::Label>,

        pub serial: OnceCell<gtk::Label>,
        pub kind: OnceCell<gtk::Label>,
        pub power_supply: OnceCell<gtk::Label>,
        pub technology: OnceCell<gtk::Label>,
        pub capacity: OnceCell<gtk::Label>,
        pub energy_empty: OnceCell<gtk::Label>,
        pub energy_full: OnceCell<gtk::Label>,
        pub energy_full_design: OnceCell<gtk::Label>,
        pub voltage_min_design: OnceCell<gtk::Label>,
        pub voltage_max_design: OnceCell<gtk::Label>,
        pub charge_threshold_enabled: OnceCell<gtk::Label>,
        pub charge_start_threshold: OnceCell<gtk::Label>,
        pub charge_end_threshold: OnceCell<gtk::Label>,
    }

    impl Default for PerformancePageBattery {
        fn default() -> Self {
            Self {
                title_battery_name: Default::default(),
                title_battery_model: Default::default(),

                energy_rate_graph: Default::default(),
                energy_rate_label: Default::default(),
                energy_rate_max_y: Default::default(),
                energy_rate_min_y: Default::default(),
                energy_rate_max_duration: Default::default(),
                energy_rate_box: Default::default(),

                history_box: Default::default(),
                history_graph: Default::default(),

                context_menu: Default::default(),

                name: RefCell::new(String::new()),
                base_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                summary_mode: Cell::new(false),

                infobar_content: Default::default(),

                percentage: Default::default(),
                energy: Default::default(),
                power: Default::default(),
                voltage: Default::default(),
                voltage_box: Default::default(),
                time_to: Default::default(),
                time_to_box: Default::default(),
                time_to_direction: Default::default(),
                state: Default::default(),
                charge_cycles: Default::default(),

                serial: Default::default(),
                kind: Default::default(),
                power_supply: Default::default(),
                technology: Default::default(),
                capacity: Default::default(),
                energy_empty: Default::default(),
                energy_full: Default::default(),
                energy_full_design: Default::default(),
                voltage_min_design: Default::default(),
                voltage_max_design: Default::default(),
                charge_threshold_enabled: Default::default(),
                charge_start_threshold: Default::default(),
                charge_end_threshold: Default::default(),
            }
        }
    }

    impl PerformancePageBattery {
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
            self.infobar_content.get().map(|ic| ic.clone().into())
        }
    }

    impl PerformancePageBattery {
        fn configure_actions(this: &super::PerformancePageBattery) {
            let actions = gio::SimpleActionGroup::new();
            this.insert_action_group("graph", Some(&actions));

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
        }

        fn configure_context_menu(this: &super::PerformancePageBattery) {
            let right_click_controller = gtk::GestureClick::new();
            right_click_controller.set_button(3); // Secondary click (AKA right click)
            right_click_controller.connect_released({
                let this = this.downgrade();
                move |_click, _n_press, x, y| {
                    if let Some(this) = this.upgrade() {
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
                }
            });
            this.add_controller(right_click_controller);
        }
    }

    impl PerformancePageBattery {
        pub fn set_static_information(
            this: &super::PerformancePageBattery,
            battery: &Battery,
        ) -> bool {
            let this = this.imp();

            if let Some(kind) = this.kind.get() {
                if let Some(v) = &battery.kind {
                    kind.set_text(&batterykind_to_str(v));
                    this.title_battery_name.set_text(&batterykind_to_str(v));
                } else {
                    kind.set_visible(false)
                }
            }

            let vendor_model = match (battery.vendor.as_ref(), battery.model.as_ref()) {
                (Some(vendor), Some(model)) => &format!("{} {}", vendor, model),
                (Some(vendor), None) => vendor,
                (None, Some(model)) => model,
                (None, None) => &i18n("Unknown"),
            };
            this.title_battery_model.set_text(&vendor_model);

            if let Some(serial) = this.serial.get() {
                if let Some(v) = &battery.serial {
                    serial.set_text(&v)
                } else {
                    serial.set_visible(false)
                }
            }

            if let Some(power_supply) = this.power_supply.get() {
                if let Some(v) = &battery.power_supply {
                    if *v {
                        power_supply.set_text(&i18n("Yes"))
                    } else {
                        power_supply.set_text(&i18n("No"))
                    }
                } else {
                    power_supply.set_visible(false)
                }
            }

            if let Some(technology) = this.technology.get() {
                if let Some(tech) = &battery.technology {
                    technology.set_text(&batterytechnology_to_str(tech))
                } else {
                    technology.set_visible(false)
                }
            }

            if let Some(capacity) = this.capacity.get() {
                if let Some(v) = &battery.capacity {
                    capacity.set_text(&i18n_f("{}%", &[&format!("{:.0}", v * 100.)]))
                } else {
                    capacity.set_visible(false)
                }
            }

            if let Some(energy_empty) = this.energy_empty.get() {
                if let Some(v) = &battery.energy_empty {
                    energy_empty.set_text(&crate::to_human_readable_nice(
                        *v as f32 / 1000.,
                        &DataType::WattHours,
                    ));
                } else {
                    energy_empty.set_visible(false)
                }
            }

            if let Some(energy_full) = this.energy_full.get() {
                if let Some(v) = &battery.energy_full {
                    energy_full.set_text(&crate::to_human_readable_nice(
                        *v as f32 / 1000.,
                        &DataType::WattHours,
                    ));
                } else {
                    energy_full.set_visible(false)
                }
            }

            if let Some(energy_full_design) = this.energy_full_design.get() {
                if let Some(v) = &battery.energy_full_design {
                    energy_full_design.set_text(&crate::to_human_readable_nice(
                        *v as f32 / 1000.,
                        &DataType::WattHours,
                    ));
                } else {
                    energy_full_design.set_visible(false)
                }
            }

            if let Some(voltage_min_design) = this.voltage_min_design.get() {
                if let Some(v) = &battery.voltage_min_design {
                    voltage_min_design
                        .set_text(&crate::to_human_readable_nice(*v, &DataType::Volts));
                } else {
                    voltage_min_design.set_visible(false)
                }
            }

            if let Some(voltage_max_design) = this.voltage_max_design.get() {
                if let Some(v) = &battery.voltage_max_design {
                    voltage_max_design
                        .set_text(&crate::to_human_readable_nice(*v, &DataType::Volts));
                } else {
                    voltage_max_design.set_visible(false)
                }
            }

            if let Some(voltage) = this.voltage.get() {
                voltage.set_visible(battery.voltage.is_some())
            }

            if let Some(energy) = this.energy.get() {
                energy.set_visible(battery.energy.is_some());
            }

            if let Some(charge_cycles) = this.charge_cycles.get() {
                charge_cycles.set_visible(battery.charge_cycles.is_some())
            }

            if battery.charge_threshold_supported == 0 {
                if let Some(charge_threshold_enabled) = this.charge_threshold_enabled.get() {
                    charge_threshold_enabled.set_visible(false);
                    charge_threshold_enabled.set_text("");
                }

                if let Some(charge_start_threshold) = this.charge_start_threshold.get() {
                    charge_start_threshold.set_visible(false);
                    charge_start_threshold.set_text("");
                }

                if let Some(charge_end_threshold) = this.charge_end_threshold.get() {
                    charge_end_threshold.set_visible(false);
                    charge_end_threshold.set_text("");
                }
            }

            let mut energy_rate_graph;
            let power_supply = battery.power_supply.unwrap_or(false);
            if power_supply && battery.state.is_some() {
                energy_rate_graph = DatasetGroup::new_with_fill(0.0);
                energy_rate_graph.dataset_settings.scaling_settings =
                    ScalingSettings::StickyUpDownEqualMagnitude;
                energy_rate_graph.dataset_settings.high_watermark = 0.;
                energy_rate_graph.dataset_settings.low_watermark = 0.;
                energy_rate_graph.dataset_settings.fill = FillingSettings::FillToZero;
                energy_rate_graph.dataset_settings.rounding_settings = RoundingSettings::Pow10;
                this.energy_rate_graph
                    .set_force_horizontal_line_count_divisible_by(2.);
            } else {
                energy_rate_graph = DatasetGroup::new();
                energy_rate_graph.dataset_settings.high_watermark = 1.;

                this.energy_rate_label.set_text(&i18n("Percentage"));
                this.energy_rate_max_y.set_text(&i18n("100%"));
                this.energy_rate_min_y.set_visible(false);
            }
            this.energy_rate_graph.add_dataset(energy_rate_graph);

            if power_supply && battery.history.len() >= 2 {
                update_history(this, battery)
            } else {
                let mut history_graph = DatasetGroup::new();
                history_graph.dataset_settings.high_watermark = 0.;
                history_graph.dataset_settings.low_watermark = 0.;

                this.history_graph.add_dataset(history_graph);
                this.history_graph.set_data_points(1);

                this.history_box.set_visible(false);
            }

            true
        }

        pub fn update_readings(
            this: &super::PerformancePageBattery,
            battery: &Battery,
            index: Option<usize>,
        ) -> bool {
            let this = this.imp();

            if let Some(v) = &battery.kind {
                if let Some(index) = index {
                    this.title_battery_name.set_text(&format!(
                        "{} {}",
                        batterykind_to_str(v),
                        index,
                    ));
                } else {
                    this.title_battery_name.set_text(&batterykind_to_str(v));
                }
            } else {
                if let Some(index) = index {
                    this.title_battery_name
                        .set_text(&i18n_f("Battery {}", &[&index.to_string()]));
                } else {
                    this.title_battery_name.set_text(&i18n("Battery"));
                }
            }

            if let Some(percentage) = this.percentage.get() {
                percentage.set_text(&i18n_f(
                    "{}%",
                    &[&format!("{:.0}", battery.percentage * 100.)],
                ));
            }

            if let Some(voltage) = this.voltage.get() {
                if let Some(v) = &battery.voltage {
                    voltage.set_text(&crate::to_human_readable_nice(*v, &DataType::Volts));
                }
            }

            if let Some(energy) = this.energy.get() {
                if let Some(v) = &battery.energy {
                    energy.set_text(&crate::to_human_readable_nice(
                        *v as f32 / 1000.,
                        &DataType::WattHours,
                    ));
                }
            }

            if let Some(power) = this.power.get() {
                if let Some(mut v) = &battery.power {
                    if let Some(v2) = &battery.state {
                        if *v2 == 2 && v != 0.0 {
                            v = -v
                        }
                    }
                    power.set_text(&crate::to_human_readable_nice(v, &DataType::Watts));
                    power.set_visible(true);
                } else {
                    power.set_visible(false);
                }
            }

            if let Some(time_to) = this.time_to.get() {
                if let Some(time_to_direction) = this.time_to_direction.get() {
                    if let Some(time_to_box) = this.time_to_box.get() {
                        if let Some(v) = &battery.time_to_full {
                            time_to_box.set_visible(true);
                            time_to.set_text(&to_long_human_readable_time(*v as u64));
                            time_to_direction.set_text(&i18n("Full"));
                        } else if let Some(v) = &battery.time_to_empty {
                            time_to_box.set_visible(true);
                            time_to.set_text(&to_long_human_readable_time(*v as u64));
                            time_to_direction.set_text(&i18n("Empty"));
                        } else {
                            time_to_box.set_visible(false)
                        }
                    }
                }
            }

            if let Some(state) = this.state.get() {
                if let Some(v) = &battery.state {
                    state.set_text(&batterystate_to_str(v))
                } else {
                    state.set_text(&i18n("Unknown"))
                }
            }

            if let Some(charge_cycles) = this.charge_cycles.get() {
                if let Some(v) = &battery.charge_cycles {
                    charge_cycles.set_text(&v.to_string())
                }
            }

            if battery.charge_threshold_supported != 0 {
                if let Some(charge_threshold_enabled) = this.charge_threshold_enabled.get() {
                    if battery.charge_threshold_enabled {
                        if (battery.charge_threshold_supported
                            & UPOWER_CHARGE_THRESHOLD_SETTINGS_SUPPORTED_FLAG_FIRMWARE)
                            != 0
                        {
                            // 2nd bit is firmware controlled
                            charge_threshold_enabled.set_text(&i18n("Firmware"))
                        } else {
                            charge_threshold_enabled.set_text(&i18n("Yes"))
                        }
                    } else {
                        charge_threshold_enabled.set_text(&i18n("Yes"))
                    }
                }

                if let Some(charge_start_threshold) = this.charge_start_threshold.get() {
                    if let Some(v) = battery.charge_start_threshold {
                        if battery.charge_threshold_enabled {
                            charge_start_threshold.set_text(&i18n_f("{}%", &[&v.to_string()]));
                            charge_start_threshold.set_visible(true)
                        } else {
                            charge_start_threshold.set_visible(false)
                        }
                    } else {
                        charge_start_threshold.set_visible(false)
                    }
                }

                if let Some(charge_end_threshold) = this.charge_end_threshold.get() {
                    if let Some(v) = battery.charge_end_threshold {
                        if battery.charge_threshold_enabled {
                            charge_end_threshold.set_text(&i18n_f("{}%", &[&v.to_string()]));
                            charge_end_threshold.set_visible(true)
                        } else {
                            charge_end_threshold.set_visible(false)
                        }
                    } else {
                        charge_end_threshold.set_visible(false)
                    }
                }
            }

            if battery.power_supply.unwrap_or(false) && battery.state.is_some() {
                if let Some(mut v) = &battery.power {
                    if let Some(v2) = &battery.state {
                        if *v2 == 2 {
                            v = -v
                        }
                    }
                    this.energy_rate_graph.add_data_point(vec![vec![v]]);
                }
                this.energy_rate_max_y
                    .set_text(&crate::to_human_readable_nice(
                        this.energy_rate_graph.get_dataset_max_scale(0),
                        &DataType::Watts,
                    ));

                this.energy_rate_min_y
                    .set_text(&crate::to_human_readable_nice(
                        this.energy_rate_graph.get_dataset_min_scale(0),
                        &DataType::Watts,
                    ));
            } else {
                this.energy_rate_graph
                    .add_data_point(vec![vec![battery.percentage]])
            }

            if battery.history_changed && battery.history.len() >= 2 {
                update_history(this, battery)
            }

            true
        }

        pub fn update_animations(
            this: &super::PerformancePageBattery,
            ticks: AnimationFrame,
        ) -> bool {
            let this = this.imp();

            this.energy_rate_graph.update_animation(ticks);

            true
        }

        // TODO: replace with macros
        fn data_summary(&self) -> String {
            let unsupported = i18n("Unsupported");
            let unsupported = glib::GString::from(unsupported);

            format!(
                r#"Battery

    {} {}

    Type:                   {}
    Energy Full:            {}
    Technology:             {}
    capacity:               {}
    Energy Full (design):   {}
    Energy Empty            {}
    Voltage Max (design):   {}
    Voltage Min (design):   {}
    Powering System:        {}

    Percentage:             {}
    State:                  {}{}
    Charge Cycles:          {}
    Power Output:           {}
    Voltage:                {}

    Charge Threshold:       {}
    Charge Start Threshold: {}
    Charge End Threshold:   {}"#,
                self.title_battery_model.text(),
                self.serial
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.kind
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.energy_full
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.technology
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.capacity
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.energy_full_design
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.energy_empty
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.voltage_max_design
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.voltage_min_design
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.power_supply
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.percentage
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.state
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                if let Some(time_to_box) = self.time_to_box.get() {
                    if time_to_box.is_visible() {
                        format!(
                            "\n    Time to {:<5}:          {}",
                            self.time_to_direction
                                .get()
                                .map(|s| s.text())
                                .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                                .unwrap_or(unsupported.clone()),
                            self.time_to
                                .get()
                                .map(|s| s.text())
                                .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                                .unwrap_or(unsupported.clone()),
                        )
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                },
                self.charge_cycles
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.power
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.voltage
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.charge_threshold_enabled
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.charge_start_threshold
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
                self.charge_end_threshold
                    .get()
                    .map(|s| s.text())
                    .map(|s| if s.is_empty() { unsupported.clone() } else { s })
                    .unwrap_or(unsupported.clone()),
            )
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePageBattery {
        const NAME: &'static str = "PerformancePageBattery";
        type Type = super::PerformancePageBattery;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePageBattery {
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
            let this = obj.upcast_ref::<super::PerformancePageBattery>().clone();

            Self::configure_actions(&this);
            Self::configure_context_menu(&this);

            let sidebar_content_builder = gtk::Builder::from_resource(
                "/io/missioncenter/MissionCenter/ui/performance_page/battery_details.ui",
            );

            let _ = self.infobar_content.set(
                sidebar_content_builder
                    .object::<gtk::Grid>("root")
                    .expect("Could not find `root` object in details pane"),
            );

            let _ = self.percentage.set(
                sidebar_content_builder
                    .object::<gtk::Label>("percentage")
                    .expect("Could not find `percentage` object in details pane"),
            );
            let _ = self.energy.set(
                sidebar_content_builder
                    .object::<gtk::Label>("energy")
                    .expect("Could not find `energy` object in details pane"),
            );
            let _ = self.power.set(
                sidebar_content_builder
                    .object::<gtk::Label>("power")
                    .expect("Could not find `power` object in details pane"),
            );
            let _ = self.voltage.set(
                sidebar_content_builder
                    .object::<gtk::Label>("voltage")
                    .expect("Could not find `voltage` object in details pane"),
            );
            let _ = self.voltage_box.set(
                sidebar_content_builder
                    .object::<gtk::Box>("voltage_box")
                    .expect("Could not find `voltage_box` object in details pane"),
            );
            let _ = self.time_to.set(
                sidebar_content_builder
                    .object::<gtk::Label>("time_to")
                    .expect("Could not find `time_to` object in details pane"),
            );
            let _ = self.time_to_box.set(
                sidebar_content_builder
                    .object::<gtk::Box>("time_to_box")
                    .expect("Could not find `time_to_box` object in details pane"),
            );
            let _ = self.time_to_direction.set(
                sidebar_content_builder
                    .object::<gtk::Label>("time_to_direction")
                    .expect("Could not find `time_to_direction` object in details pane"),
            );
            let _ = self.state.set(
                sidebar_content_builder
                    .object::<gtk::Label>("state")
                    .expect("Could not find `state` object in details pane"),
            );
            let _ = self.charge_cycles.set(
                sidebar_content_builder
                    .object::<gtk::Label>("charge_cycles")
                    .expect("Could not find `charge_cycles` object in details pane"),
            );
            let _ = self.serial.set(
                sidebar_content_builder
                    .object::<gtk::Label>("serial")
                    .expect("Could not find `serial` object in details pane"),
            );
            let _ = self.kind.set(
                sidebar_content_builder
                    .object::<gtk::Label>("kind")
                    .expect("Could not find `kind` object in details pane"),
            );
            let _ = self.power_supply.set(
                sidebar_content_builder
                    .object::<gtk::Label>("power_supply")
                    .expect("Could not find `power_supply` object in details pane"),
            );
            let _ = self.technology.set(
                sidebar_content_builder
                    .object::<gtk::Label>("technology")
                    .expect("Could not find `technology` object in details pane"),
            );
            let _ = self.capacity.set(
                sidebar_content_builder
                    .object::<gtk::Label>("capacity")
                    .expect("Could not find `capacity` object in details pane"),
            );
            let _ = self.energy_empty.set(
                sidebar_content_builder
                    .object::<gtk::Label>("energy_empty")
                    .expect("Could not find `energy_empty` object in details pane"),
            );
            let _ = self.energy_full.set(
                sidebar_content_builder
                    .object::<gtk::Label>("energy_full")
                    .expect("Could not find `energy_full` object in details pane"),
            );
            let _ = self.energy_full_design.set(
                sidebar_content_builder
                    .object::<gtk::Label>("energy_full_design")
                    .expect("Could not find `energy_full_design` object in details pane"),
            );
            let _ = self.voltage_min_design.set(
                sidebar_content_builder
                    .object::<gtk::Label>("voltage_min_design")
                    .expect("Could not find `voltage_min_design` object in details pane"),
            );
            let _ = self.voltage_max_design.set(
                sidebar_content_builder
                    .object::<gtk::Label>("voltage_max_design")
                    .expect("Could not find `voltage_max_design` object in details pane"),
            );
            let _ = self.charge_threshold_enabled.set(
                sidebar_content_builder
                    .object::<gtk::Label>("charge_threshold_enabled")
                    .expect("Could not find `charge_threshold_enabled` object in details pane"),
            );
            let _ = self.charge_start_threshold.set(
                sidebar_content_builder
                    .object::<gtk::Label>("charge_start_threshold")
                    .expect("Could not find `charge_start_threshold` object in details pane"),
            );
            let _ = self.charge_end_threshold.set(
                sidebar_content_builder
                    .object::<gtk::Label>("charge_end_threshold")
                    .expect("Could not find `charge_end_threshold` object in details pane"),
            );
        }
    }

    impl WidgetImpl for PerformancePageBattery {}

    impl BoxImpl for PerformancePageBattery {}
}

glib::wrapper! {
    pub struct PerformancePageBattery(ObjectSubclass<imp::PerformancePageBattery>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PageExt for PerformancePageBattery {
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

impl PerformancePageBattery {
    pub fn new(name: &str, settings: &gio::Settings) -> Self {
        let this: Self = glib::Object::builder().property("name", name).build();

        fn update_refresh_rate_sensitive_labels(
            this: &PerformancePageBattery,
            settings: &gio::Settings,
        ) {
            let data_points = settings.int("performance-page-data-points") as u32;
            let delay = settings.uint64("app-update-interval-u64");
            let graph_max_duration =
                (((delay as f64) * INTERVAL_STEP) * (data_points as f64)).round() as u32;

            let this = this.imp();

            let time_string = &to_short_human_readable_time(graph_max_duration);

            this.energy_rate_max_duration.set_text(time_string);
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

        this.imp().energy_rate_graph.connect_to_settings(settings);
        this.imp()
            .history_graph
            .connect_to_smooth_settings(settings);

        this
    }

    pub fn set_static_information(&self, battery_info: &Battery) -> bool {
        imp::PerformancePageBattery::set_static_information(self, battery_info)
    }

    pub fn update_readings(&self, battery_info: &Battery, index: Option<usize>) -> bool {
        imp::PerformancePageBattery::update_readings(self, battery_info, index)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePageBattery::update_animations(self, ticks)
    }
}

fn update_history(
    this: &crate::performance_page::battery::imp::PerformancePageBattery,
    battery: &Battery,
) {
    const TOTAL_SECS: u32 = 3600 * 24 * 7;
    let datapoints = battery.history.len();

    let mut his = Vec::with_capacity(datapoints);
    let mut his_interpol = Vec::new();
    if let Some(mut first) = battery.history.last() {
        if first.y.is_nan() {
            if let Some(f) = battery
                .history
                .len()
                .checked_sub(2)
                .map(|i| &battery.history[i])
            {
                first = f
            } else {
                return;
            }
        }
        his_interpol.push((TOTAL_SECS as f32, first.y));
        his_interpol.push((first.x, first.y));
        his_interpol.push((first.x, f32::NAN));
    }
    for d in battery.history.windows(3).rev() {
        if d[1].y.is_nan() {
            his_interpol.push((d[0].x, d[0].y));
            his_interpol.push((d[2].x, d[2].y));
            his_interpol.push((d[2].x, f32::NAN));
        }
        his.push((d[1].x, d[1].y))
    }
    if let Some(last) = battery.history.first() {
        his.push((last.x, last.y));
    }
    his.push((0.0, battery.percentage));

    let mut history_graph = DatasetGroup::new_with_datas(vec![his]);
    history_graph.dataset_settings.high_watermark = 1.;
    history_graph.dataset_settings.vertical_dropoff_lines = false;

    let mut history_graph_interpol = DatasetGroup::new_with_datas(vec![his_interpol]);
    history_graph_interpol.dataset_settings.high_watermark = 1.;
    history_graph_interpol.dataset_settings.dashed = true;
    history_graph_interpol.dataset_settings.opacity = 0.1;
    history_graph_interpol
        .dataset_settings
        .vertical_dropoff_lines = false;

    this.history_graph.set_data_points(TOTAL_SECS);
    this.history_graph.clear_datasets();
    this.history_graph.add_dataset(history_graph);
    this.history_graph
        .set_force_vertical_line_count_divisible_by(7.);
    this.history_graph.add_dataset(history_graph_interpol);
    this.history_graph.update_animation(AnimationFrame {
        progress: 0.0,
        grid_offset: 0,
    });
}

// According to https://upower.freedesktop.org/docs/Device.html

fn batterystate_to_str(state: &i32) -> String {
    match state {
        1 => i18n("Charging"),
        2 => i18n("Discharging"),
        3 => i18n("Empty"),
        4 | 5 | 6 => i18n("Full"),
        _ => String::new(),
    }
}

//const UPOWER_CHARGE_THRESHOLD_SETTINGS_SUPPORTED_FLAG_START: u32 = 1;
//const UPOWER_CHARGE_THRESHOLD_SETTINGS_SUPPORTED_FLAG_END: u32 = 2;
const UPOWER_CHARGE_THRESHOLD_SETTINGS_SUPPORTED_FLAG_FIRMWARE: u32 = 4;

fn batterykind_to_str(kind: &i32) -> String {
    match kind {
        1 => i18n("LinePower"),
        2 => i18n("Battery"),
        3 => i18n("UPS"),
        4 => i18n("Monitor"),
        5 => i18n("Mouse"),
        6 => i18n("Keyboard"),
        7 => i18n("PDA"),
        8 => i18n("Phone"),
        9 => i18n("Media player"),
        10 => i18n("Tablet"),
        11 => i18n("Computer"),
        12 => i18n("Gaming Input"),
        13 => i18n("Pen"),
        14 => i18n("Touchpad"),
        15 => i18n("Modem"),
        16 => i18n("Network"),
        17 => i18n("Headset"),
        18 => i18n("Speakers"),
        19 => i18n("Headphones"),
        20 => i18n("Video"),
        21 => i18n("Other Audio"),
        22 => i18n("Remote Control"),
        23 => i18n("Printer"),
        34 => i18n("Scanner"),
        35 => i18n("Camera"),
        36 => i18n("Wearable"),
        37 => i18n("Toy"),
        38 => i18n("Generic bluetooth"),
        _ => String::new(),
    }
}

fn batterytechnology_to_str(kind: &i32) -> String {
    match kind {
        1 => i18n("Lithium Ion"),
        2 => i18n("Lithium Polymer"),
        3 => i18n("Lithium Iron Phosphate"),
        4 => i18n("Lead Acid"),
        5 => i18n("Nickel Cadmium"),
        6 => i18n("Nickel Metal Hydride"),
        _ => String::new(),
    }
}
