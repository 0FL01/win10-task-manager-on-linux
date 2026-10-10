/* table_view/columns/pid.rs
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

use std::cmp::Ordering;

use gtk::glib;
use gtk::prelude::*;

use super::{compare_column_entries_by, sort_order, LabelCell};
use crate::label_cell_factory;

pub fn list_item_factory() -> gtk::SignalListItemFactory {
    label_cell_factory!(
        "pid",
        ContentType::SectionHeader | ContentType::Service | ContentType::App,
        label_formatter
    )
}

pub fn sorter(column_view: &gtk::ColumnView) -> impl IsA<gtk::Sorter> {
    let column_view = column_view.downgrade();
    gtk::CustomSorter::new(move |lhs, rhs| {
        let Some(column_view) = column_view.upgrade() else {
            return Ordering::Equal.into();
        };

        compare_column_entries_by(lhs, rhs, sort_order(&column_view), |lhs, rhs| {
            lhs.pid().cmp(&rhs.pid())
        })
        .into()
    })
}

pub fn label_formatter(label: &LabelCell, value: glib::Value) {
    let pid: u32 = value.get().unwrap();
    label.set_label(&pid.to_string());
}
