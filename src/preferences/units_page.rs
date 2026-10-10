/* preferences/units_page.rs
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

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::settings;

use super::connect_toggle_pair_to_setting;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate, Default)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/preferences/units_page.ui")]
    pub struct PreferencesUnitsPage {
        #[template_child]
        pub toggle_group_memory_unit: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub toggle_memory_unit_bits: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_memory_unit_bytes: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_group_memory_base: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub toggle_memory_base_2: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_memory_base_10: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_group_drive_unit: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub toggle_drive_unit_bits: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_drive_unit_bytes: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_group_drive_base: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub toggle_drive_base_2: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_drive_base_10: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_group_net_unit: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub toggle_net_unit_bits: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_net_unit_bytes: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_group_net_base: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub toggle_net_base_2: TemplateChild<adw::Toggle>,
        #[template_child]
        pub toggle_net_base_10: TemplateChild<adw::Toggle>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesUnitsPage {
        const NAME: &'static str = "PreferencesUnitsPage";
        type Type = super::PreferencesUnitsPage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesUnitsPage {
        fn constructed(&self) {
            self.parent_constructed();

            connect_toggle_pair_to_setting!(
                self.toggle_group_memory_unit,
                self.toggle_memory_unit_bytes,
                "performance-page-memory2-use-bytes"
            );
            connect_toggle_pair_to_setting!(
                self.toggle_group_memory_base,
                self.toggle_memory_base_2,
                "performance-page-memory2-use-base2"
            );
            connect_toggle_pair_to_setting!(
                self.toggle_group_drive_unit,
                self.toggle_drive_unit_bytes,
                "performance-page-drive-use-bytes"
            );
            connect_toggle_pair_to_setting!(
                self.toggle_group_drive_base,
                self.toggle_drive_base_2,
                "performance-page-drive-use-base2"
            );
            connect_toggle_pair_to_setting!(
                self.toggle_group_net_unit,
                self.toggle_net_unit_bytes,
                "performance-page-network-use-bytes"
            );
            connect_toggle_pair_to_setting!(
                self.toggle_group_net_base,
                self.toggle_net_base_2,
                "performance-page-network-use-base2"
            );
        }
    }

    impl WidgetImpl for PreferencesUnitsPage {}

    impl PreferencesPageImpl for PreferencesUnitsPage {}
}

glib::wrapper! {
    pub struct PreferencesUnitsPage(ObjectSubclass<imp::PreferencesUnitsPage>)
        @extends adw::PreferencesPage, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PreferencesUnitsPage {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();

        let imp = this.imp();
        let settings = settings!();

        imp.toggle_group_memory_unit
            .set_active(!settings.boolean("performance-page-memory2-use-bytes") as u32);
        imp.toggle_group_memory_base
            .set_active(settings.boolean("performance-page-memory2-use-base2") as u32);
        imp.toggle_group_drive_unit
            .set_active(!settings.boolean("performance-page-drive-use-bytes") as u32);
        imp.toggle_group_drive_base
            .set_active(settings.boolean("performance-page-drive-use-base2") as u32);
        imp.toggle_group_net_unit
            .set_active(!settings.boolean("performance-page-network-use-bytes") as u32);
        imp.toggle_group_net_base
            .set_active(settings.boolean("performance-page-network-use-base2") as u32);

        this
    }
}
