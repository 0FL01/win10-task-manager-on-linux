/* first_run_dialog.rs
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

use adw::prelude::AdwDialogExt;
use adw::subclass::prelude::*;

use gtk::prelude::{ActionGroupExt, ActionMapExt, ButtonExt, DisplayExt, WidgetExt};
use gtk::{
    gio,
    glib::{self, g_critical, g_warning},
    AlertDialog,
};

use crate::{app, i18n};

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate, Default)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/first_run_dialog.ui")]
    pub struct FirstRunDialog {
        #[template_child]
        pub close: TemplateChild<gtk::Button>,
        #[template_child]
        pub run_script: TemplateChild<adw::SplitButton>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FirstRunDialog {
        const NAME: &'static str = "FirstRunDialog";
        type Type = super::FirstRunDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FirstRunDialog {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for FirstRunDialog {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl AdwDialogImpl for FirstRunDialog {
        fn closed(&self) {}
    }
}

glib::wrapper! {
    pub struct FirstRunDialog(ObjectSubclass<imp::FirstRunDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FirstRunDialog {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder()
            .property("follows-content-size", true)
            .build();

        let imp = this.imp();

        let dialog = this.clone();
        imp.close.connect_clicked(move |_| {
            dialog.close();
        });

        this.connect_map(|dialog| {
            if let Some(widget) = dialog.default_widget() {
                dialog.set_focus(Some(&widget));
            }
        });

        this
    }
    pub fn run() {
        let app = app!();

        let Some(window) = app.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to show about dialog"
            );
            return;
        };
        let Ok(magpie) = app.sys_info() else {
            g_warning!("MissionCenter::Application", "Failed to get magpie client");
            return;
        };
        let dialog = Self::new();

        if let Some((file, elevation_command)) = magpie.setup_script_name() {
            let app_clone = app.clone();
            let window_clone = window.clone();
            let file_clone = file.clone();

            let action = gio::SimpleAction::new("view_script", None);
            app.add_action(&action);

            action.connect_activate(move |_, _| {
                let Ok(magpie) = app_clone.sys_info() else {
                    g_warning!("MissionCenter::Application", "Failed to get magpie client");
                    return;
                };

                if let Err(output) = magpie.setup_script_open() {
                    let error_dialog = AlertDialog::builder()
                        .modal(true)
                        .message(&i18n("Failed to Open Script"))
                        .detail(&format!("{}:\n{}", &file_clone, &output,))
                        .default_button(1)
                        .buttons([i18n("Copy File Location"), i18n("Close")])
                        .build();

                    let display = gtk::prelude::WidgetExt::display(&window_clone);

                    error_dialog.choose(
                        Some(&window_clone),
                        None::<&gtk::gio::Cancellable>,
                        move |response| match response {
                            Ok(0) => display.clipboard().set_text(&output),
                            _ => (),
                        },
                    );
                }
            });

            let app_clone = app.clone();
            let window_clone = window.clone();
            let dialog_clone = dialog.clone();
            let action = gio::SimpleAction::new("run_script", None);
            app.add_action(&action);

            action.connect_activate(move |_, _| {
                let Ok(magpie) = app_clone.sys_info() else {
                    g_warning!("MissionCenter::Application", "Failed to get magpie client");
                    return;
                };

                if let Err(message) = magpie.setup_script_run() {
                    let error_dialog = AlertDialog::builder()
                        .modal(true)
                        .message(&i18n("Setup Script Failed"))
                        .detail(&message)
                        .buttons([i18n("Copy Output"), i18n("Retry"), i18n("Close")])
                        .default_button(1)
                        .build();

                    let display = gtk::prelude::WidgetExt::display(&window_clone);
                    let app_clone = app_clone.clone();

                    error_dialog.choose(
                        Some(&window_clone),
                        None::<&gtk::gio::Cancellable>,
                        move |response| match response {
                            Ok(0) => display.clipboard().set_text(&message),
                            Ok(1) => app_clone.activate_action("run_script", None),
                            _ => (),
                        },
                    );
                } else {
                    let restart_dialog = AlertDialog::builder()
                        .modal(true)
                        .message(&i18n("Success!"))
                        .detail(&i18n("Restart Mission Center to apply changes?"))
                        .buttons([i18n("Restart"), i18n("Close")])
                        .build();
                    let app_clone = app_clone.clone();

                    restart_dialog.choose(
                        Some(&window_clone),
                        None::<&gtk::gio::Cancellable>,
                        move |response| match response {
                            Ok(0) => app_clone.restart(),
                            _ => (),
                        },
                    );
                    dialog_clone.close();
                };
            });

            let app_clone = app.clone();
            let window_clone = window.clone();
            let dialog_clone = dialog.clone();
            let action = gio::SimpleAction::new("run_revert_script", None);
            app.add_action(&action);

            action.connect_activate(move |_, _| {
                let Ok(magpie) = app_clone.sys_info() else {
                    g_warning!("MissionCenter::Application", "Failed to get magpie client");
                    return;
                };

                if let Err(message) = magpie.setup_script_run_revert() {
                    let error_dialog = AlertDialog::builder()
                        .modal(true)
                        .message(&i18n("Revert Setup Script Failed"))
                        .detail(&message)
                        .buttons([i18n("Copy Output"), i18n("Retry"), i18n("Close")])
                        .default_button(1)
                        .build();

                    let display = gtk::prelude::WidgetExt::display(&window_clone);
                    let app_clone = app_clone.clone();

                    error_dialog.choose(
                        Some(&window_clone),
                        None::<&gtk::gio::Cancellable>,
                        move |response| match response {
                            Ok(0) => display.clipboard().set_text(&message),
                            Ok(1) => app_clone.activate_action("run_revert_script", None),
                            _ => (),
                        },
                    );
                } else {
                    dialog_clone.close();
                };
            });

            let file_clone = file.clone();
            let elevation_command_clone = elevation_command.clone();
            let display = gtk::prelude::WidgetExt::display(&window);
            let action = gio::SimpleAction::new("copy_script_command", None);
            action.connect_activate(move |_, _| {
                display
                    .clipboard()
                    .set_text(&format!("{} {}", elevation_command_clone, file_clone));
            });
            app.add_action(&action);

            let file_clone = file.clone();
            let elevation_command_clone = elevation_command.clone();
            let display = gtk::prelude::WidgetExt::display(&window);
            let action = gio::SimpleAction::new("copy_revert_script_command", None);
            action.connect_activate(move |_, _| {
                display.clipboard().set_text(&format!(
                    "{} {} --revert",
                    elevation_command_clone, file_clone
                ));
            });
            app.add_action(&action);

            let display = gtk::prelude::WidgetExt::display(&window);
            let action = gio::SimpleAction::new("copy_script_location", None);
            action.connect_activate(move |_, _| {
                display.clipboard().set_text(&file);
            });
            app.add_action(&action);
        }

        dialog.present(Some(&window));
    }
}
