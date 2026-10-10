/* preferences/general_page.rs
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
use adw::SpinRow;
use glib::g_critical;
use gtk::{gio, glib};

use crate::application::INTERVAL_STEP;
use crate::settings;

const MAX_INTERVAL_TICKS: u64 = 200;
const MIN_INTERVAL_TICKS: u64 = 10;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate, Default)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/preferences/general_page.ui")]
    pub struct PreferencesGeneralPage {
        #[template_child]
        pub update_interval: TemplateChild<SpinRow>,
    }

    impl PreferencesGeneralPage {
        pub fn configure_update_speed(&self) {
            let settings = settings!();

            let new_interval = (self.update_interval.value() / INTERVAL_STEP).round() as u64;

            if new_interval <= MAX_INTERVAL_TICKS && new_interval >= MIN_INTERVAL_TICKS {
                if settings
                    .set_uint64("app-update-interval-u64", new_interval)
                    .is_err()
                {
                    g_critical!(
                        "MissionCenter::Preferences",
                        "Failed to set update interval setting",
                    );
                }
            } else {
                g_critical!(
                    "MissionCenter::Preferences",
                    "Update interval out of bounds",
                );
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesGeneralPage {
        const NAME: &'static str = "PreferencesGeneralPage";
        type Type = super::PreferencesGeneralPage;
        type ParentType = adw::PreferencesPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PreferencesGeneralPage {
        fn constructed(&self) {
            self.parent_constructed();

            self.update_interval
                .downcast_ref::<SpinRow>()
                .unwrap()
                .connect_changed({
                    let this = self.obj().downgrade();
                    move |_| {
                        if let Some(this) = this.upgrade() {
                            this.imp().configure_update_speed();
                        }
                    }
                });
        }
    }

    impl WidgetImpl for PreferencesGeneralPage {}

    impl PreferencesPageImpl for PreferencesGeneralPage {}
}

glib::wrapper! {
    pub struct PreferencesGeneralPage(ObjectSubclass<imp::PreferencesGeneralPage>)
        @extends adw::PreferencesPage, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PreferencesGeneralPage {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();

        let settings = settings!();

        let update_interval_s = (settings.uint64("app-update-interval-u64") as f64) * INTERVAL_STEP;
        this.imp().update_interval.set_value(update_interval_s);

        this
    }
}
