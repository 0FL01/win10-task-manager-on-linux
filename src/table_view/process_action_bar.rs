/* table_view/process_action_bar.rs
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

use crate::table_view::row_model::ContentType;
use crate::table_view::TableView;
use adw::prelude::*;
use gtk::{gio, glib, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/table_view/process_action_bar.ui")]
    pub struct ProcessActionBar {
        #[template_child]
        pub stop_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub force_stop_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub details_label: TemplateChild<gtk::Label>,
    }

    impl Default for ProcessActionBar {
        fn default() -> Self {
            Self {
                stop_label: Default::default(),
                force_stop_label: Default::default(),
                details_label: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ProcessActionBar {
        const NAME: &'static str = "ProcessActionBar";
        type Type = super::ProcessActionBar;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ProcessActionBar {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for ProcessActionBar {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl BoxImpl for ProcessActionBar {}

    impl ProcessActionBar {
        pub fn collapse(&self) {
            self.stop_label.set_visible(false);
            self.force_stop_label.set_visible(false);
            self.details_label.set_visible(false);
        }

        pub fn expand(&self) {
            self.stop_label.set_visible(true);
            self.force_stop_label.set_visible(true);
            self.details_label.set_visible(true);
        }
    }
}

glib::wrapper! {
    pub struct ProcessActionBar(ObjectSubclass<imp::ProcessActionBar>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl ProcessActionBar {
    pub fn set_column_view(&self, column_view: &TableView) {
        let handle_selection_change = |this: &Self, column_view: TableView| {
            let selected_item = column_view.selected_item();
            match selected_item.content_type() {
                ContentType::Process | ContentType::App => {
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
