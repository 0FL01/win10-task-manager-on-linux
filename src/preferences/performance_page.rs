/* preferences/performance_page.rs
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

use adw::{prelude::*, subclass::prelude::*, ExpanderRow, SwitchRow};
use glib::g_critical;
use gtk::{gio, glib, Scale};

use crate::settings;

use super::{connect_switch_to_setting, MAX_POINTS, MIN_POINTS};

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate, Default)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/preferences/performance_page.ui")]
    pub struct PreferencesPerformancePage {
        #[template_child]
        pub data_points: TemplateChild<Scale>,
        #[template_child]
        pub smooth_graphs: TemplateChild<SwitchRow>,
        #[template_child]
        pub sliding_graphs: TemplateChild<SwitchRow>,
        #[template_child]
        pub network_dynamic_scaling: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_disks: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_network: TemplateChild<ExpanderRow>,
        #[template_child]
        pub show_network_wired: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_network_wireless: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_network_vpn: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_network_virtual: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_network_other: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_gpus: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_fans: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_batteries: TemplateChild<SwitchRow>,
    }

    impl PreferencesPerformancePage {
        pub fn configure_data_points(&self) {
            let settings = settings!();

            let new_points = self.data_points.value() as i32;

            if new_points <= MAX_POINTS && new_points >= MIN_POINTS {
                if settings
                    .set_int("performance-page-data-points", new_points)
                    .is_err()
                {
                    g_critical!(
                        "MissionCenter::Preferences",
                        "Failed to set update points setting",
                    );
                }
            } else {
                g_critical!(
                    "MissionCenter::Preferences",
                    "Points interval out of bounds",
                );
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesPerformancePage {
        const NAME: &'static str = "PreferencesPerformancePage";
        type Type = super::PreferencesPerformancePage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesPerformancePage {
        fn constructed(&self) {
            self.parent_constructed();

            self.data_points
                .downcast_ref::<Scale>()
                .unwrap()
                .connect_value_changed({
                    let this = self.obj().downgrade();
                    move |_| {
                        if let Some(this) = this.upgrade() {
                            this.imp().configure_data_points();
                        }
                    }
                });

            connect_switch_to_setting!(self.smooth_graphs, "performance-smooth-graphs");
            connect_switch_to_setting!(self.sliding_graphs, "performance-sliding-graphs");
            connect_switch_to_setting!(
                self.network_dynamic_scaling,
                "performance-page-network-dynamic-scaling"
            );
            connect_switch_to_setting!(self.show_disks, "performance-show-disks");
            self.show_network.connect_enable_expansion_notify({
                move |expander| {
                    if let Err(e) = settings!()
                        .set_boolean("performance-show-network", expander.enables_expansion())
                    {
                        g_critical!(
                            "MissionCenter::Preferences",
                            "Failed to set performance-show-network setting: {}",
                            e
                        );
                    }
                }
            });
            connect_switch_to_setting!(self.show_network_wired, "performance-show-network-wired");
            connect_switch_to_setting!(
                self.show_network_wireless,
                "performance-show-network-wireless"
            );
            connect_switch_to_setting!(self.show_network_vpn, "performance-show-network-vpn");
            connect_switch_to_setting!(
                self.show_network_virtual,
                "performance-show-network-virtual"
            );
            connect_switch_to_setting!(self.show_network_other, "performance-show-network-other");
            connect_switch_to_setting!(self.show_gpus, "performance-show-gpus");
            connect_switch_to_setting!(self.show_fans, "performance-show-fans");
            connect_switch_to_setting!(self.show_batteries, "performance-show-batteries");
        }
    }

    impl WidgetImpl for PreferencesPerformancePage {}

    impl PreferencesPageImpl for PreferencesPerformancePage {}
}

glib::wrapper! {
    pub struct PreferencesPerformancePage(ObjectSubclass<imp::PreferencesPerformancePage>)
        @extends adw::PreferencesPage, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PreferencesPerformancePage {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();

        let imp = this.imp();
        let settings = settings!();

        let data_points = settings.int("performance-page-data-points");
        imp.data_points.set_value(data_points as f64);

        imp.smooth_graphs
            .set_active(settings.boolean("performance-smooth-graphs"));
        imp.sliding_graphs
            .set_active(settings.boolean("performance-sliding-graphs"));
        imp.network_dynamic_scaling
            .set_active(settings.boolean("performance-page-network-dynamic-scaling"));
        imp.show_disks
            .set_active(settings.boolean("performance-show-disks"));
        imp.show_network
            .set_enable_expansion(settings.boolean("performance-show-network"));
        imp.show_network_wired
            .set_active(settings.boolean("performance-show-network-wired"));
        imp.show_network_wireless
            .set_active(settings.boolean("performance-show-network-wireless"));
        imp.show_network_vpn
            .set_active(settings.boolean("performance-show-network-vpn"));
        imp.show_network_virtual
            .set_active(settings.boolean("performance-show-network-virtual"));
        imp.show_network_other
            .set_active(settings.boolean("performance-show-network-other"));
        imp.show_gpus
            .set_active(settings.boolean("performance-show-gpus"));
        imp.show_fans
            .set_active(settings.boolean("performance-show-fans"));
        imp.show_batteries
            .set_active(settings.boolean("performance-show-batteries"));

        this
    }
}
