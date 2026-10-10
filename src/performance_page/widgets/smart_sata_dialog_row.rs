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

use std::cell::Cell;

use crate::i18n::{i18n, i18n_f};
use crate::performance_page::MK_TO_0_C;
use gtk::subclass::prelude::WidgetImpl;
use gtk::{
    glib,
    glib::{prelude::*, subclass::prelude::*, Properties},
};
use std::cell::OnceCell;

mod imp {
    use super::*;

    #[derive(Default, Properties)]
    #[properties(wrapper_type = super::SmartSataDialogRow)]
    pub struct SmartSataDialogRow {
        #[property(get, set)]
        pub smart_id: Cell<u8>,
        #[property(get, set)]
        pub attribute: OnceCell<String>,
        #[property(get, set)]
        pub value: OnceCell<String>,
        #[property(get, set)]
        pub normalized: Cell<i32>,
        #[property(get, set)]
        pub threshold: Cell<i32>,
        #[property(get, set)]
        pub worst: Cell<i32>,
        #[property(get, set)]
        pub typee: OnceCell<String>,
        #[property(get, set)]
        pub updates: OnceCell<String>,
        #[property(get, set)]
        pub assessment: OnceCell<String>,
    }

    impl SmartSataDialogRow {}

    #[glib::object_subclass]
    impl ObjectSubclass for SmartSataDialogRow {
        const NAME: &'static str = "SmartSataDialogRow";
        type ParentType = glib::Object;
        type Type = super::SmartSataDialogRow;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SmartSataDialogRow {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for SmartSataDialogRow {}
}

glib::wrapper! {
    pub struct SmartSataDialogRow(ObjectSubclass<imp::SmartSataDialogRow>)
        @extends gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl SmartSataDialogRow {
    pub fn new(
        id: u8,
        attribute: String,
        value: i32,
        pretty: i64,
        units: i32,
        threshold: i32,
        worst: i32,
        typee: &str,
        updates: &str,
        assessment: &str,
    ) -> Self {
        glib::Object::builder()
            .property("smart_id", id)
            .property("attribute", attribute)
            .property(
                "value",
                &match units {
                    0 => i18n("N/A"),
                    2 => crate::to_long_human_readable_time(pretty as u64 / 1000),
                    3 => i18n_f("{} sectors", &[&format!("{}", pretty)]),
                    4 => i18n_f(
                        "{} °C",
                        &[&format!("{}", (pretty as i32 + MK_TO_0_C) / 1000)],
                    ),
                    _ => format!("{}", pretty),
                },
            )
            .property("normalized", value)
            .property("threshold", threshold)
            .property("worst", worst)
            .property("typee", typee)
            .property("updates", updates)
            .property("assessment", assessment)
            .build()
    }
}
