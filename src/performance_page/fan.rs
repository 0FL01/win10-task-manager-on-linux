/* performance_page/fan.rs
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

use adw;
use adw::subclass::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::{gio, glib, prelude::*};

use magpie_types::fan::Fan;

use crate::application::INTERVAL_STEP;
use crate::i18n::*;
use crate::performance_page::widgets::{
    AnimationFrame, DatasetGroup, FillingSettings, GraphWidget, GraphWidgetSettingsExt,
    ScalingSettings,
};
use crate::performance_page::{PageExt, MK_TO_0_C};
use crate::to_short_human_readable_time;

pub const TEMPERATURE_HIGH_WATERMARK: f32 = 45.;
pub const TEMPERATURE_LOW_WATERMARK: f32 = 35.;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePageFan)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/fan.ui")]
    pub struct PerformancePageFan {
        #[template_child]
        pub title_fan_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub title_temp_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub speed_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub temp_graph: TemplateChild<GraphWidget>,

        #[template_child]
        pub speed_max_y: TemplateChild<gtk::Label>,
        #[template_child]
        pub speed_graph_max_duration: TemplateChild<gtk::Label>,
        #[template_child]
        pub speed_box: TemplateChild<gtk::Box>,

        #[template_child]
        pub temp_max_y: TemplateChild<gtk::Label>,
        #[template_child]
        pub temp_graph_max_duration: TemplateChild<gtk::Label>,
        #[template_child]
        pub temp_graph_box: TemplateChild<gtk::Box>,

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

        pub legend_speed: OnceCell<gtk::Picture>,
        pub speed: OnceCell<gtk::Label>,
        pub bow_pwm: OnceCell<gtk::Box>,
        pub box_temp: OnceCell<gtk::Box>,
        pub legend_pwm: OnceCell<gtk::Picture>,
        pub pwm: OnceCell<gtk::Label>,
        pub temp: OnceCell<gtk::Label>,
    }

    impl Default for PerformancePageFan {
        fn default() -> Self {
            Self {
                title_fan_name: Default::default(),
                title_temp_name: Default::default(),
                speed_graph: Default::default(),
                temp_graph: Default::default(),
                speed_max_y: Default::default(),
                speed_graph_max_duration: Default::default(),
                speed_box: Default::default(),
                temp_max_y: Default::default(),
                temp_graph_max_duration: Default::default(),
                temp_graph_box: Default::default(),
                context_menu: Default::default(),

                name: RefCell::new(String::new()),
                base_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                summary_mode: Cell::new(false),

                infobar_content: Default::default(),

                legend_speed: Default::default(),
                speed: Default::default(),
                bow_pwm: Default::default(),
                box_temp: Default::default(),
                legend_pwm: Default::default(),
                pwm: Default::default(),
                temp: Default::default(),
            }
        }
    }

    impl PerformancePageFan {
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

    impl PerformancePageFan {
        fn configure_actions(this: &super::PerformancePageFan) {
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

        fn configure_context_menu(this: &super::PerformancePageFan) {
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

    impl PerformancePageFan {
        pub fn set_static_information(this: &super::PerformancePageFan, fan: &Fan) -> bool {
            let this = this.imp();

            if let Some(legend_send) = this.legend_speed.get() {
                legend_send
                    .set_resource(Some("/io/missioncenter/MissionCenter/line-solid-net.svg"));
            }

            if let Some(legend_pwm) = this.legend_pwm.get() {
                legend_pwm
                    .set_resource(Some("/io/missioncenter/MissionCenter/line-dashed-net.svg"));
            }

            if let Some(temp_name) = &fan.temp_name {
                this.title_temp_name.set_text(temp_name);
            }

            if fan.pwm_percent.is_none() {
                if let Some(box_pwm) = this.bow_pwm.get() {
                    box_pwm.set_visible(false);
                }

                if let Some(legend_speed) = this.legend_speed.get() {
                    legend_speed.set_visible(false);
                }
            }

            if fan.temp_amount.is_none() {
                this.temp_graph_box.set_visible(false);

                if let Some(sidebar_temp_box) = this.box_temp.get() {
                    sidebar_temp_box.set_visible(false);
                }
            }

            if let Some(max_rpm) = fan.max_rpm {
                this.speed_max_y.set_text(&format!("{}", max_rpm));

                this.speed_graph
                    .set_dataset_scaling(0, ScalingSettings::Fixed);
                this.speed_graph.set_dataset_max_scale(0, max_rpm as f32);
            }
            true
        }

        pub fn update_readings(
            this: &super::PerformancePageFan,
            fan: &Fan,
            index: Option<usize>,
        ) -> bool {
            let this = this.imp();

            if let Some(fan_label) = &fan.fan_label {
                this.title_fan_name.set_text(fan_label);
            } else if let Some(index) = index {
                this.title_fan_name
                    .set_text(&i18n_f("Fan {}", &[&index.to_string()]));
            } else {
                this.title_fan_name.set_text(&i18n("Fan"));
            }

            if let Some(speed_send) = this.speed.get() {
                speed_send.set_text(&i18n_f("{} RPM", &[&format!("{}", fan.rpm)]));
            }

            if let Some(pwm) = this.pwm.get() {
                pwm.set_text(&i18n_f(
                    "{}%",
                    &[&format!(
                        "{:.0}",
                        fan.pwm_percent.clone().unwrap_or(0.) * 100.0
                    )],
                ));
            }

            if let Some(fan_temp_mk) = fan.temp_amount {
                let fan_temp_c = (fan_temp_mk as i32 + MK_TO_0_C) as f32 / 1000.;
                if let Some(temp) = this.temp.get() {
                    temp.set_text(&i18n_f("{} °C", &[&format!("{:.1}", fan_temp_c)]));
                }

                this.temp_graph.add_data_point(vec![vec![fan_temp_c]]);
                this.temp_max_y
                    .set_text(&format!("{} °C", this.temp_graph.get_dataset_max_scale(0)));
            }

            this.speed_graph.add_data_point(vec![
                vec![fan.rpm as f32],
                vec![fan.pwm_percent.unwrap_or(0.)],
            ]);

            if fan.max_rpm.is_none() {
                this.speed_max_y.set_text(&i18n_f(
                    "{} RPM",
                    &[&this.speed_graph.get_dataset_max_scale(0).to_string()],
                ));
            }

            true
        }

        pub fn update_animations(this: &super::PerformancePageFan, ticks: AnimationFrame) -> bool {
            let this = this.imp();

            this.speed_graph.update_animation(ticks);
            this.temp_graph.update_animation(ticks);

            true
        }

        fn data_summary(&self) -> String {
            let unsupported = i18n("Unsupported");
            let unsupported = glib::GString::from(unsupported);

            format!(
                r#"Fan

    {}
    {}

    Speed:               {}
    PWM Percentage:      {}
    Current Temperature: {}"#,
                self.title_fan_name.text(),
                self.title_temp_name.text(),
                self.speed
                    .get()
                    .map(|s| s.text())
                    .unwrap_or(unsupported.clone()),
                self.pwm
                    .get()
                    .and_then(|pwm| if !pwm.is_visible() { None } else { Some(pwm) })
                    .map(|s| s.text())
                    .unwrap_or(unsupported.clone()),
                self.temp
                    .get()
                    .and_then(|temp| if !temp.is_visible() { None } else { Some(temp) })
                    .map(|s| s.text())
                    .unwrap_or(unsupported)
            )
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePageFan {
        const NAME: &'static str = "PerformancePageFan";
        type Type = super::PerformancePageFan;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePageFan {
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
            let this = obj.upcast_ref::<super::PerformancePageFan>().clone();

            Self::configure_actions(&this);
            Self::configure_context_menu(&this);

            let sidebar_content_builder = gtk::Builder::from_resource(
                "/io/missioncenter/MissionCenter/ui/performance_page/fan_details.ui",
            );

            let _ = self.infobar_content.set(
                sidebar_content_builder
                    .object::<gtk::Grid>("root")
                    .expect("Could not find `root` object in details pane"),
            );

            let _ = self.legend_speed.set(
                sidebar_content_builder
                    .object::<gtk::Picture>("legend_speed")
                    .expect("Could not find `legend_speed` object in details pane"),
            );
            let _ = self.legend_pwm.set(
                sidebar_content_builder
                    .object::<gtk::Picture>("legend_pwm")
                    .expect("Could not find `legend_pwm` object in details pane"),
            );

            let _ = self.bow_pwm.set(
                sidebar_content_builder
                    .object::<gtk::Box>("box_pwm")
                    .expect("Could not find `box_pwm` object in details pane"),
            );

            let _ = self.box_temp.set(
                sidebar_content_builder
                    .object::<gtk::Box>("box_temp")
                    .expect("Could not find `box_temp` object in details pane"),
            );

            let _ = self.speed.set(
                sidebar_content_builder
                    .object::<gtk::Label>("speed")
                    .expect("Could not find `speed` object in details pane"),
            );
            let _ = self.pwm.set(
                sidebar_content_builder
                    .object::<gtk::Label>("pwm")
                    .expect("Could not find `pwm` object in details pane"),
            );
            let _ = self.temp.set(
                sidebar_content_builder
                    .object::<gtk::Label>("temp")
                    .expect("Could not find `temp` object in details pane"),
            );
        }
    }

    impl WidgetImpl for PerformancePageFan {}

    impl BoxImpl for PerformancePageFan {}
}

glib::wrapper! {
    pub struct PerformancePageFan(ObjectSubclass<imp::PerformancePageFan>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PageExt for PerformancePageFan {
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

impl PerformancePageFan {
    pub fn new(name: &str, settings: &gio::Settings) -> Self {
        let this: Self = glib::Object::builder().property("name", name).build();

        fn update_refresh_rate_sensitive_labels(
            this: &PerformancePageFan,
            settings: &gio::Settings,
        ) {
            let data_points = settings.int("performance-page-data-points") as u32;
            let delay = settings.uint64("app-update-interval-u64");
            let graph_max_duration =
                (((delay as f64) * INTERVAL_STEP) * (data_points as f64)).round() as u32;

            let this = this.imp();

            let time_string = &to_short_human_readable_time(graph_max_duration);

            this.speed_graph_max_duration.set_text(time_string);
            this.temp_graph_max_duration.set_text(time_string);
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

        let mut temp_dataset = DatasetGroup::new();
        temp_dataset.dataset_settings.scaling_settings = ScalingSettings::StickyUpDown;
        temp_dataset.dataset_settings.high_watermark = TEMPERATURE_HIGH_WATERMARK;
        temp_dataset.dataset_settings.low_watermark = TEMPERATURE_LOW_WATERMARK;

        this.imp().temp_graph.add_dataset(temp_dataset);

        let mut speed_dataset = DatasetGroup::new();
        speed_dataset.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
        let mut rpm_dataset = DatasetGroup::new();
        rpm_dataset.dataset_settings.dashed = true;
        rpm_dataset.dataset_settings.fill = FillingSettings::None;
        rpm_dataset.dataset_settings.high_watermark = 1.;

        this.imp().speed_graph.add_dataset(speed_dataset);
        this.imp().speed_graph.add_dataset(rpm_dataset);

        this.imp().temp_graph.connect_to_settings(settings);
        this.imp().speed_graph.connect_to_settings(settings);

        this
    }

    pub fn set_static_information(&self, fan_info: &Fan) -> bool {
        imp::PerformancePageFan::set_static_information(self, fan_info)
    }

    pub fn update_readings(&self, fan_info: &Fan, index: Option<usize>) -> bool {
        imp::PerformancePageFan::update_readings(self, fan_info, index)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePageFan::update_animations(self, ticks)
    }
}
