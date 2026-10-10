/* services_page/widgets/smart_failure_dialog.rs
 *
 * Copyright 2024 Mission Center Devs
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

use adw::ResponseAppearance;
use adw::{prelude::*, subclass::prelude::*};
use glib::g_warning;
use gtk::glib;

use crate::i18n;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(
        resource = "/io/missioncenter/MissionCenter/ui/performance_page/disk_smart_failure_dialog.ui"
    )]
    pub struct SmartFailureDialog {}

    impl SmartFailureDialog {
        pub fn update_model(&self) {}
    }

    impl Default for SmartFailureDialog {
        fn default() -> Self {
            Self {}
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SmartFailureDialog {
        const NAME: &'static str = "SmartFailureDialog";
        type Type = super::SmartFailureDialog;
        type ParentType = adw::AlertDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SmartFailureDialog {
        fn constructed(&self) {
            self.parent_constructed();

            let close = "close";
            self.obj().add_response(close, &i18n::i18n("Close"));

            self.obj()
                .set_response_appearance(close, ResponseAppearance::Default);
        }
    }

    impl AdwAlertDialogImpl for SmartFailureDialog {
        fn response(&self, response: &str) {
            match response {
                "close" => {
                    self.obj().close();
                }
                other => {
                    g_warning!(
                        "MissionCenter::DetailsDialog",
                        "Unexpected response: {other}"
                    );
                }
            }
        }
    }

    impl WidgetImpl for SmartFailureDialog {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl AdwDialogImpl for SmartFailureDialog {}
}

glib::wrapper! {
    pub struct SmartFailureDialog(ObjectSubclass<imp::SmartFailureDialog>)
        @extends adw::AlertDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SmartFailureDialog {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder()
            .property("follows-content-size", true)
            .build();

        this
    }
}
