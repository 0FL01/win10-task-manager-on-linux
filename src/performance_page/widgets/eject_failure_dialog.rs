/* services_page/widgets/eject_failure_dialog.rs
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

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use adw::ResponseAppearance;
use adw::{prelude::*, subclass::prelude::*};
use glib::{g_critical, g_warning};
use gtk::glib;

use magpie_types::apps::App;
use magpie_types::disks::error_eject_failed::Blocker;
use magpie_types::disks::ErrorEjectFailed;

use crate::performance_page::widgets::eject_failure_row::EjectFailureRowBuilder;
use crate::{app, i18n};

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(
        resource = "/io/missioncenter/MissionCenter/ui/performance_page/disk_eject_failure_dialog.ui"
    )]
    pub struct EjectFailureDialog {
        #[template_child]
        column_view: TemplateChild<gtk::ListBox>,

        pub disk_id: OnceCell<String>,
        pub error: RefCell<ErrorEjectFailed>,
    }

    impl EjectFailureDialog {
        pub fn update_model(&self, disk_id: &str, error: &ErrorEjectFailed) {
            let model = self.column_view.get();
            model.remove_all();

            let parsed_results = Self::parse_error(error);

            for (appname, (app_obj, blockers)) in parsed_results {
                for blocker in blockers {
                    let row_builder = EjectFailureRowBuilder::new()
                        .id(disk_id)
                        .app_id(app_obj.id.clone())
                        .pid(blocker.pid)
                        .name(&appname)
                        .dialog(&self.obj());

                    if !blocker.files.is_empty() {
                        model.append(
                            &row_builder
                                .clone()
                                .files_open(blocker.files.clone())
                                .build(),
                        );
                    }

                    if !blocker.dirs.is_empty() {
                        model.append(&row_builder.files_open(blocker.dirs).build());
                    }
                }
            }
        }

        fn parse_error(error: &ErrorEjectFailed) -> HashMap<String, (App, Vec<Blocker>)> {
            let mut result = HashMap::new();

            let Some(window) = app!().window() else {
                g_critical!(
                    "MissionCenter::Application",
                    "No active window, when trying to show eject dialog"
                );

                return result;
            };

            if error.blockers.is_empty() {
                return result;
            }

            let apps = window.imp().apps_page.running_apps();

            for blocker in error.blockers.iter().map(|b| b.clone()) {
                if let Some(blocking_app) = apps.values().find(|a| a.pids.contains(&blocker.pid)) {
                    if let Some((_, blocking)) = result.get_mut(&blocking_app.name) {
                        blocking.push(blocker);
                    } else {
                        result.insert(
                            blocking_app.name.clone(),
                            (blocking_app.clone(), vec![blocker]),
                        );
                    }
                } else {
                    if let Some((_, blocking)) = result.get_mut("") {
                        blocking.push(blocker);
                    } else {
                        result.insert("".parse().unwrap(), (Default::default(), vec![blocker]));
                    }
                }
            }

            result
        }
    }

    impl Default for EjectFailureDialog {
        fn default() -> Self {
            Self {
                column_view: Default::default(),

                disk_id: OnceCell::new(),
                error: RefCell::new(ErrorEjectFailed::default()),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EjectFailureDialog {
        const NAME: &'static str = "EjectFailureDialog";
        type Type = super::EjectFailureDialog;
        type ParentType = adw::AlertDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EjectFailureDialog {
        fn constructed(&self) {
            self.parent_constructed();

            let close = "close";
            let retry = "retry";
            let kill = "kill";
            self.obj().add_response(close, &i18n::i18n("Close"));
            self.obj().add_response(retry, &i18n::i18n("Retry"));
            self.obj().add_response(kill, &i18n::i18n("Kill All"));

            self.obj()
                .set_response_appearance(close, ResponseAppearance::Default);
            self.obj()
                .set_response_appearance(retry, ResponseAppearance::Default);
            self.obj()
                .set_response_appearance(kill, ResponseAppearance::Destructive);
        }
    }

    impl AdwAlertDialogImpl for EjectFailureDialog {
        fn response(&self, response: &str) {
            let app = app!();
            let Ok(magpie) = app.sys_info() else {
                g_warning!(
                    "MissionCenter::EjectFailureDialog",
                    "Failed to get magpie client"
                );
                return;
            };

            let disk_id = self.disk_id.get().unwrap();
            let mut error = self.error.borrow_mut();

            match response {
                "retry" => match magpie.eject_disk(disk_id) {
                    Ok(_) => {
                        self.obj().close();
                    }
                    Err(e) => {
                        self.update_model(disk_id, &e);
                    }
                },
                "kill" => {
                    magpie.kill_processes(error.blockers.iter().map(|b| b.pid).collect());
                    match magpie.eject_disk(disk_id) {
                        Ok(_) => {
                            self.obj().close();
                        }
                        Err(e) => {
                            self.update_model(disk_id, &e);
                            *error = e;
                        }
                    }
                }
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

    impl WidgetImpl for EjectFailureDialog {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl AdwDialogImpl for EjectFailureDialog {}
}

glib::wrapper! {
    pub struct EjectFailureDialog(ObjectSubclass<imp::EjectFailureDialog>)
        @extends adw::AlertDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl EjectFailureDialog {
    pub fn new(disk_id: String, error: ErrorEjectFailed) -> Self {
        let this: Self = glib::Object::builder()
            .property("follows-content-size", true)
            .build();
        {
            let this = this.imp();
            this.update_model(&disk_id, &error);
            this.disk_id.set(disk_id).unwrap();
            this.error.replace(error);
        }

        this
    }

    pub fn disk_id(&self) -> &str {
        self.imp().disk_id.get().unwrap().as_str()
    }

    pub fn update_model(&self, error: ErrorEjectFailed) {
        let this = self.imp();

        let disk_id = self.disk_id();
        this.update_model(&disk_id, &error);
        this.error.replace(error);
    }
}
