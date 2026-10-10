/* performance_page/widgets/eject_failure_row.rs
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

use std::cell::{Cell, RefCell};

use adw::glib::g_warning;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::{self, g_critical, WeakRef};

use crate::app;
use crate::performance_page::widgets::EjectFailureDialog;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(
        resource = "/io/missioncenter/MissionCenter/ui/performance_page/disk_eject_failure_row.ui"
    )]
    pub struct EjectFailureRow {
        #[template_child]
        icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub pid: TemplateChild<gtk::Label>,
        #[template_child]
        pub name: TemplateChild<gtk::Label>,
        #[template_child]
        pub open_files: TemplateChild<gtk::Label>,
        #[template_child]
        pub kill: TemplateChild<gtk::Button>,

        pub raw_pid: Cell<u32>,
        pub dialog: RefCell<WeakRef<EjectFailureDialog>>,
    }

    impl EjectFailureRow {
        pub fn set_icon(&self, app_id: String) {
            app!().apply_app_icon(&self.icon.get(), app_id, 48);
        }
    }

    impl Default for EjectFailureRow {
        fn default() -> Self {
            Self {
                icon: Default::default(),
                name: Default::default(),
                pid: Default::default(),
                open_files: Default::default(),
                kill: Default::default(),

                raw_pid: Cell::new(0),
                dialog: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EjectFailureRow {
        const NAME: &'static str = "EjectFailureRow";
        type Type = super::EjectFailureRow;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EjectFailureRow {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for EjectFailureRow {}

    impl BinImpl for EjectFailureRow {}
}

#[derive(Clone)]
pub struct EjectFailureRowBuilder {
    pid: u32,
    app_id: Option<String>,
    name: glib::GString,
    id: String,

    files_open: Vec<String>,

    dialog: WeakRef<EjectFailureDialog>,
}

impl EjectFailureRowBuilder {
    pub fn new() -> Self {
        Self {
            pid: 0,
            app_id: Default::default(),
            name: glib::GString::default(),
            id: String::from(""),

            files_open: vec![],

            dialog: WeakRef::default(),
        }
    }

    pub fn pid(mut self, pid: u32) -> Self {
        self.pid = pid;
        self
    }

    pub fn app_id(mut self, app_id: String) -> Self {
        self.app_id = Some(app_id);
        self
    }

    pub fn name(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = id.into();
        self
    }

    pub fn files_open(mut self, files_open: Vec<String>) -> Self {
        self.files_open = files_open;
        self
    }

    pub fn dialog(mut self, dialog: &EjectFailureDialog) -> Self {
        self.dialog = dialog.downgrade();
        self
    }

    pub fn build(self) -> EjectFailureRow {
        let this = EjectFailureRow::new();
        {
            let this = this.imp();

            if let Some(app_id) = self.app_id {
                this.set_icon(app_id);
            }
            this.pid.set_label(&self.pid.to_string());
            this.name.set_label(self.name.as_str());
            this.open_files
                .set_label(self.files_open.join("\n").as_str());
            this.raw_pid.set(self.pid);
            this.dialog.replace(self.dialog);

            this.kill.connect_clicked({
                let this = this.obj().downgrade();
                move |_| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let this = this.imp();

                    let Some(dialog) = this.dialog.borrow().upgrade() else {
                        g_critical!(
                            "MissionCenter::EjectFailureRow",
                            "Failed to get parent dialog",
                        );
                        return;
                    };

                    let app = app!();
                    let Ok(magpie) = app.sys_info() else {
                        g_warning!(
                            "MissionCenter::EjectFailureDialog",
                            "Failed to get magpie client"
                        );
                        return;
                    };

                    magpie.kill_process(this.raw_pid.get());
                    match magpie.eject_disk(dialog.disk_id()) {
                        Ok(_) => {
                            dialog.close();
                        }
                        Err(e) => {
                            dialog.update_model(e);
                        }
                    }
                }
            });
        }

        this
    }
}

glib::wrapper! {
    pub struct EjectFailureRow(ObjectSubclass<imp::EjectFailureRow>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl EjectFailureRow {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();

        this
    }
}
