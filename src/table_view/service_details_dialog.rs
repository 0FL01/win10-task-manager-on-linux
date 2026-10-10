/* table_view/service_details_dialog.rs
 *
 * Copyright 2025 Mission Center Developers
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
use std::num::NonZeroU32;

use adw::gio;
use adw::{prelude::*, subclass::prelude::*};
use gtk::glib::{self, g_warning, ParamSpec, Properties, SignalHandlerId, Value};

use crate::services_page::actions;
use crate::table_view::row_model::RowModel;
use crate::table_view::TableView;
use crate::{app, i18n::*};

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::ServiceDetailsDialog)]
    #[derive(gtk::CompositeTemplate)]
    #[template(
        resource = "/io/missioncenter/MissionCenter/ui/table_view/service_details_dialog.ui"
    )]
    pub struct ServiceDetailsDialog {
        #[template_child]
        group_state: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        box_buttons: TemplateChild<gtk::Box>,
        #[template_child]
        restart: TemplateChild<gtk::Button>,
        #[template_child]
        label_name: TemplateChild<gtk::Label>,
        #[template_child]
        label_description: TemplateChild<gtk::Label>,
        #[template_child]
        label_running: TemplateChild<gtk::Label>,
        #[template_child]
        switch_enabled: TemplateChild<adw::SwitchRow>,

        #[template_child]
        group_process: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        label_pid: TemplateChild<gtk::Label>,
        #[template_child]
        label_user: TemplateChild<gtk::Label>,
        #[template_child]
        label_group: TemplateChild<gtk::Label>,
        #[template_child]
        file_location: TemplateChild<gtk::Label>,

        #[template_child]
        logs_overlay: TemplateChild<gtk::Overlay>,
        #[template_child]
        logs_expander: TemplateChild<gtk::Expander>,
        #[template_child]
        logs_buffer: TemplateChild<gtk::TextBuffer>,

        pub list_item: OnceCell<RowModel>,

        #[property(get, set)]
        pub enabled: Cell<bool>,
        #[property(get, construct_only)]
        pub column_view: RefCell<TableView>,

        copy_logs_button: gtk::Button,

        list_item_running_notify: Cell<u64>,
        list_item_enabled_notify: Cell<u64>,
        list_item_enabled_user_change: Cell<bool>,
    }

    impl Default for ServiceDetailsDialog {
        fn default() -> Self {
            Self {
                group_state: TemplateChild::default(),
                box_buttons: TemplateChild::default(),
                restart: TemplateChild::default(),
                label_name: TemplateChild::default(),
                label_description: TemplateChild::default(),
                label_running: TemplateChild::default(),
                switch_enabled: TemplateChild::default(),

                group_process: TemplateChild::default(),
                label_pid: TemplateChild::default(),
                label_user: TemplateChild::default(),
                label_group: TemplateChild::default(),

                file_location: TemplateChild::default(),
                logs_overlay: TemplateChild::default(),
                logs_expander: TemplateChild::default(),
                logs_buffer: TemplateChild::default(),

                list_item: OnceCell::new(),

                enabled: Cell::new(false),
                column_view: RefCell::new(glib::Object::builder().build()),

                copy_logs_button: gtk::Button::new(),

                list_item_running_notify: Cell::new(0),
                list_item_enabled_notify: Cell::new(0),
                list_item_enabled_user_change: Cell::new(true),
            }
        }
    }

    impl ServiceDetailsDialog {
        fn list_item(&self) -> RowModel {
            unsafe { self.list_item.get().unwrap_unchecked().clone() }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ServiceDetailsDialog {
        const NAME: &'static str = "ServiceDetailsDialog";
        type Type = super::ServiceDetailsDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ServiceDetailsDialog {
        fn properties() -> &'static [ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
            self.derived_set_property(id, value, pspec)
        }

        fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
            self.derived_property(id, pspec)
        }

        fn constructed(&self) {
            self.parent_constructed();

            if let Some(_) = std::env::var_os("SNAP_CONTEXT") {
                self.switch_enabled.set_sensitive(false);
                self.box_buttons.set_visible(false);
                self.restart.set_visible(false);
            }

            self.switch_enabled.connect_active_notify({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();

                        if !this.list_item_enabled_user_change.get() {
                            this.list_item_enabled_user_change.set(true);
                            return;
                        }

                        let list_item = this.list_item();
                        match app!().sys_info().and_then(move |sys_info| {
                            match this.switch_enabled.is_active() {
                                // Emitted after the switch is toggled
                                true => sys_info.enable_service(list_item.service_id()),
                                false => sys_info.disable_service(list_item.service_id()),
                            }

                            Ok(())
                        }) {
                            Err(e) => {
                                g_warning!(
                                    "MissionCenter::ServiceDetailsDialog",
                                    "Failed to get `sys_info`: {}",
                                    e
                                );
                            }
                            _ => {}
                        }
                    }
                }
            });

            self.copy_logs_button.set_margin_top(14);
            self.copy_logs_button.set_margin_end(2);
            self.copy_logs_button.set_valign(gtk::Align::Start);
            self.copy_logs_button.set_halign(gtk::Align::End);
            self.copy_logs_button.add_css_class("flat");
            self.copy_logs_button.set_icon_name("edit-copy-symbolic");
            self.copy_logs_button
                .set_tooltip_text(Some(&i18n("Copy logs to clipboard")));

            self.copy_logs_button.connect_clicked({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        let clipboard = this.clipboard();

                        let this = this.imp();
                        let logs = this.logs_buffer.property::<glib::GString>("text");

                        clipboard.set_text(logs.as_str());
                    }
                }
            });

            self.logs_overlay.add_overlay(&self.copy_logs_button);

            let column_view = self.column_view.borrow();
            let column_view = &*column_view;

            let service_actions = gio::SimpleActionGroup::new();
            service_actions.add_action(&actions::action_start(column_view));
            service_actions.add_action(&actions::action_stop(column_view));
            service_actions.add_action(&actions::action_restart(column_view));
            self.obj()
                .insert_action_group("service", Some(&service_actions));
        }
    }

    impl WidgetImpl for ServiceDetailsDialog {
        fn realize(&self) {
            self.parent_realize();

            self.logs_buffer.set_text("");
            self.logs_expander.set_visible(false);

            self.logs_expander.set_expanded(false);

            self.list_item_enabled_user_change.set(false);

            let list_item = self.list_item();

            self.label_name.set_text(&list_item.name());
            self.label_description.set_text(&list_item.description());
            let running = if list_item.service_running() {
                i18n("Running")
            } else if list_item.service_failed() {
                i18n("Failed")
            } else {
                i18n("Stopped")
            };
            self.label_running.set_text(&running);
            self.switch_enabled.set_active(list_item.service_enabled());

            let mut group_empty = true;
            let pid = list_item.pid();
            if pid > 0 {
                group_empty = false;
                self.label_pid.set_text(&list_item.pid().to_string());
            } else {
                self.label_pid.set_text(&i18n("N/A"));
            }

            let user = list_item.user();
            if !user.is_empty() {
                group_empty = false;
                self.label_user.set_text(&list_item.user());
            } else {
                self.label_user.set_text(&i18n("N/A"));
            }

            let group = list_item.group();
            if !group.is_empty() {
                group_empty = false;
                self.label_group.set_text(&list_item.group());
            } else {
                self.label_group.set_text(&i18n("N/A"));
            }

            let location = list_item.file_path();
            if !location.is_empty() {
                group_empty = false;
                self.file_location.set_text(&list_item.file_path());
            } else {
                self.file_location.set_text(&i18n("Unknown"));
            }

            if group_empty {
                self.group_process.set_visible(false);
            } else {
                self.group_process.set_visible(true);
            }

            let logs = app!().sys_info().and_then(|sys_info| {
                Ok(sys_info.service_logs(list_item.service_id(), NonZeroU32::new(pid)))
            });

            match logs {
                Ok(logs) => {
                    if !logs.is_empty() {
                        self.logs_buffer.set_text(&logs);
                        self.logs_expander.set_visible(true);
                    }
                }
                Err(e) => {
                    g_warning!(
                        "MissionCenter::ServiceDetailsDialog",
                        "Failed to get `sys_info`: {}",
                        e
                    );
                }
            }

            let notify = list_item.connect_service_running_notify({
                let this = self.obj().downgrade();
                move |li| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        let text = if li.service_running() {
                            i18n("Running")
                        } else if li.service_failed() {
                            i18n("Failed")
                        } else {
                            i18n("Stopped")
                        };
                        this.label_running.set_text(&text);
                    }
                }
            });
            self.list_item_running_notify.set(from_signal_id(notify));

            let notify = list_item.connect_service_running_notify({
                let this = self.obj().downgrade();
                move |li| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();

                        if li.service_enabled() != this.switch_enabled.is_active() {
                            this.list_item_enabled_user_change.set(false);
                            this.switch_enabled
                                .set_active(this.list_item().service_enabled());
                        }
                    }
                }
            });
            self.list_item_enabled_notify.set(from_signal_id(notify));
        }
    }

    impl AdwDialogImpl for ServiceDetailsDialog {
        fn closed(&self) {
            let list_item = self.list_item();
            list_item.disconnect(to_signal_id(self.list_item_running_notify.get()));
            list_item.disconnect(to_signal_id(self.list_item_enabled_notify.get()));
        }
    }
}

glib::wrapper! {
    pub struct ServiceDetailsDialog(ObjectSubclass<imp::ServiceDetailsDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ServiceDetailsDialog {
    pub fn new(column_view: &TableView) -> Self {
        let this: Self = glib::Object::builder()
            .property("follows-content-size", true)
            .property("column-view", Some(column_view))
            .build();
        let _ = this.imp().list_item.set(column_view.selected_item());

        this
    }
}

fn to_signal_id(id: u64) -> SignalHandlerId {
    unsafe { std::mem::transmute(id) }
}

fn from_signal_id(id: SignalHandlerId) -> u64 {
    unsafe { id.as_raw() }
}
