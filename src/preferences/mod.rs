/* preferences/mod.rs
 *
 * Copyright 2023 Romeo Calota
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

mod apps_services_page;
mod general_page;
mod performance_page;
mod units_page;

macro_rules! connect_switch_to_setting_impl {
    ($switch_row: expr, $setting: literal) => {
        $switch_row.connect_active_notify({
            move |switch_row| {
                if let Err(e) = settings!().set_boolean($setting, switch_row.is_active()) {
                    gtk::glib::g_critical!(
                        "MissionCenter::Preferences",
                        "Failed to set {} setting: {}",
                        $setting,
                        e
                    );
                }
            }
        });
    };
}
pub(crate) use connect_switch_to_setting_impl as connect_switch_to_setting;

macro_rules! connect_toggle_pair_to_setting_impl {
    ($toggle_group: expr, $toggle_truthy: expr, $setting: literal) => {
        $toggle_group.connect_notify_local(Some("active"), {
            let toggle_truthy = $toggle_truthy.downgrade();
            move |toggle_group, _| {
                let Some(toggle_truthy) = toggle_truthy.upgrade() else {
                    return;
                };

                let active_index = toggle_group.active();
                let active_toggle = toggle_group.toggle(active_index);
                let truthy_active = active_toggle.as_ref() == Some(&toggle_truthy);
                if let Err(e) = settings!().set_boolean($setting, truthy_active) {
                    gtk::glib::g_critical!(
                        "MissionCenter::Preferences",
                        "Failed to set {} setting: {}",
                        $setting,
                        e
                    );
                }
            }
        });
    };
}
pub(crate) use connect_toggle_pair_to_setting_impl as connect_toggle_pair_to_setting;

pub const MAX_POINTS: i32 = 600;
pub const MIN_POINTS: i32 = 10;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/preferences/window.ui")]
    pub struct PreferencesDialog {}

    impl Default for PreferencesDialog {
        fn default() -> Self {
            Self {}
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesDialog {
        const NAME: &'static str = "PreferencesDialog";
        type Type = super::PreferencesDialog;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesDialog {}

    impl WidgetImpl for PreferencesDialog {}

    impl AdwDialogImpl for PreferencesDialog {}

    impl PreferencesDialogImpl for PreferencesDialog {}
}

glib::wrapper! {
    pub struct PreferencesDialog(ObjectSubclass<imp::PreferencesDialog>)
        @extends adw::PreferencesDialog, adw::Dialog, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PreferencesDialog {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();

        this.add(&general_page::PreferencesGeneralPage::new());
        this.add(&performance_page::PreferencesPerformancePage::new());
        this.add(&apps_services_page::PreferencesAppsServicesPage::new());
        this.add(&units_page::PreferencesUnitsPage::new());

        this
    }
}
