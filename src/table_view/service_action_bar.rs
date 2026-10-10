/* table_view/service_action_bar.rs
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

use std::cell::Cell;

use adw::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::{gio, glib, subclass::prelude::*};

use crate::table_view::row_model::{ContentType, RowModel};
use crate::table_view::TableView;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::ServiceActionBar)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/table_view/service_action_bar.ui")]
    pub struct ServiceActionBar {
        #[template_child]
        pub service_start_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub service_stop_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub service_restart_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub service_details_label: TemplateChild<gtk::Label>,

        #[property(get)]
        is_snap: Cell<bool>,
    }

    impl Default for ServiceActionBar {
        fn default() -> Self {
            Self {
                service_start_label: Default::default(),
                service_stop_label: Default::default(),
                service_restart_label: Default::default(),
                service_details_label: Default::default(),

                is_snap: Cell::new(false),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ServiceActionBar {
        const NAME: &'static str = "ServiceActionBar";
        type Type = super::ServiceActionBar;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ServiceActionBar {
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
                self.is_snap.set(true);
                self.obj().notify_is_snap();
            }
        }
    }

    impl WidgetImpl for ServiceActionBar {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl BoxImpl for ServiceActionBar {}

    impl ServiceActionBar {
        pub fn collapse(&self) {
            self.service_stop_label.set_visible(false);
            self.service_start_label.set_visible(false);
            self.service_restart_label.set_visible(false);
            self.service_details_label.set_visible(false);
        }

        pub fn expand(&self) {
            self.service_stop_label.set_visible(true);
            self.service_start_label.set_visible(true);
            self.service_restart_label.set_visible(true);
            self.service_details_label.set_visible(true);
        }

        pub fn handle_changed_selection(&self, row_model: &RowModel) {
            match row_model.content_type() {
                ContentType::Service => {
                    self.obj().set_visible(true);
                }
                ContentType::SectionHeader => {}
                _ => {
                    self.obj().set_visible(false);
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct ServiceActionBar(ObjectSubclass<imp::ServiceActionBar>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl ServiceActionBar {
    pub fn set_column_view(&self, column_view: &TableView) {
        let handle_selection_change = |this: &Self, column_view: TableView| {
            let selected_item = column_view.selected_item();
            match selected_item.content_type() {
                ContentType::Service => {
                    this.set_visible(true);
                }
                ContentType::SectionHeader => {}
                _ => {
                    this.set_visible(false);
                }
            }
        };
        handle_selection_change(self, column_view.clone());

        column_view.connect_selected_item_notify({
            let this = self.downgrade();
            move |column_view| {
                if let Some(this) = this.upgrade() {
                    handle_selection_change(&this, column_view.clone());
                }
            }
        });
    }
}
