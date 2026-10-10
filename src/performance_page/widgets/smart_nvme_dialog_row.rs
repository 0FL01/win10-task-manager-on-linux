/* apps_page/view_model.rs
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

use gtk::subclass::prelude::WidgetImpl;
use gtk::{
    glib,
    glib::{prelude::*, subclass::prelude::*, Properties},
};
use std::cell::OnceCell;

mod imp {
    use super::*;

    #[derive(Default, Properties)]
    #[properties(wrapper_type = super::SmartNvmeDialogRow)]
    pub struct SmartNvmeDialogRow {
        #[property(get, set)]
        pub label: OnceCell<String>,
        #[property(get, set)]
        pub value: OnceCell<String>,
    }

    impl SmartNvmeDialogRow {}

    #[glib::object_subclass]
    impl ObjectSubclass for SmartNvmeDialogRow {
        const NAME: &'static str = "SmartNvmeDialogRow";
        type ParentType = glib::Object;
        type Type = super::SmartNvmeDialogRow;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SmartNvmeDialogRow {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for SmartNvmeDialogRow {}
}

glib::wrapper! {
    pub struct SmartNvmeDialogRow(ObjectSubclass<imp::SmartNvmeDialogRow>)
        @extends gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl SmartNvmeDialogRow {
    pub fn new(label: String, value: String) -> Self {
        glib::Object::builder()
            .property("label", label)
            .property("value", value)
            .build()
    }
}
