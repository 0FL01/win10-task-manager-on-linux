/* preferences/apps_services_page.rs
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

use adw::{prelude::*, subclass::prelude::*, SwitchRow};
use gtk::{gio, glib};

use crate::settings;

use super::connect_switch_to_setting;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate, Default)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/preferences/apps_services_page.ui")]
    pub struct PreferencesAppsServicesPage {
        #[template_child]
        pub merged_process_stats: TemplateChild<SwitchRow>,
        #[template_child]
        pub remember_sorting: TemplateChild<SwitchRow>,
        #[template_child]
        pub remember_column_order: TemplateChild<SwitchRow>,
        #[template_child]
        pub core_count_affects_percentages: TemplateChild<SwitchRow>,
        #[template_child]
        pub show_column_separators: TemplateChild<SwitchRow>,
        #[template_child]
        pub hide_is_zero: TemplateChild<SwitchRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesAppsServicesPage {
        const NAME: &'static str = "PreferencesAppsServicesPage";
        type Type = super::PreferencesAppsServicesPage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesAppsServicesPage {
        fn constructed(&self) {
            self.parent_constructed();

            connect_switch_to_setting!(self.merged_process_stats, "apps-page-merged-process-stats");
            connect_switch_to_setting!(self.remember_sorting, "apps-page-remember-sorting");
            connect_switch_to_setting!(
                self.remember_column_order,
                "apps-page-remember-column-order"
            );
            connect_switch_to_setting!(
                self.core_count_affects_percentages,
                "apps-page-core-count-affects-percentages"
            );
            connect_switch_to_setting!(
                self.show_column_separators,
                "apps-page-show-column-separators"
            );
            connect_switch_to_setting!(self.hide_is_zero, "apps-page-hide-is-zero");
        }
    }

    impl WidgetImpl for PreferencesAppsServicesPage {}

    impl PreferencesPageImpl for PreferencesAppsServicesPage {}
}

glib::wrapper! {
    pub struct PreferencesAppsServicesPage(ObjectSubclass<imp::PreferencesAppsServicesPage>)
        @extends adw::PreferencesPage, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PreferencesAppsServicesPage {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();

        let imp = this.imp();
        let settings = settings!();

        imp.merged_process_stats
            .set_active(settings.boolean("apps-page-merged-process-stats"));
        imp.remember_sorting
            .set_active(settings.boolean("apps-page-remember-sorting"));
        imp.remember_column_order
            .set_active(settings.boolean("apps-page-remember-column-order"));
        imp.core_count_affects_percentages
            .set_active(settings.boolean("apps-page-core-count-affects-percentages"));
        imp.show_column_separators
            .set_active(settings.boolean("apps-page-show-column-separators"));
        imp.hide_is_zero
            .set_active(settings.boolean("apps-page-hide-is-zero"));

        this
    }
}
