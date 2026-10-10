/* table_view/columns/name.rs
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

use adw::prelude::*;

use crate::table_view::columns::{compare_column_entries_by, sort_order, NameCell};
use crate::table_view::row_model::RowModel;
use crate::widgets::ListCell;

pub fn list_item_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        let name_cell = NameCell::new();

        let list_cell = ListCell::new("column-view.show-context-menu");
        list_cell.set_is_tree_view(true);
        list_cell.set_child(Some(&name_cell));

        let expander = gtk::TreeExpander::new();
        expander.set_child(Some(&list_cell));

        expander.set_hide_expander(false);
        expander.set_indent_for_icon(true);
        expander.set_indent_for_depth(true);
        expander.set_halign(gtk::Align::Start);
        expander.set_width_request(218);

        list_item.set_child(Some(&expander));

        unsafe {
            list_item.set_data("expander", expander);
            list_item.set_data("list_cell", list_cell);
            list_item.set_data("list_item", name_cell);
        }
    });

    factory.connect_bind(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        let Some(row) = list_item
            .item()
            .and_then(|item| item.downcast::<gtk::TreeListRow>().ok())
        else {
            return;
        };

        let expander = unsafe {
            list_item
                .data::<gtk::TreeExpander>("expander")
                .unwrap_unchecked()
                .as_ref()
        };
        expander.set_list_row(Some(&row));

        let Some(model) = expander
            .item()
            .and_then(|item| item.downcast::<RowModel>().ok())
        else {
            return;
        };

        let name_cell = unsafe {
            list_item
                .data::<NameCell>("list_item")
                .unwrap_unchecked()
                .as_ref()
        };

        let list_cell = unsafe {
            list_item
                .data::<ListCell>("list_cell")
                .unwrap_unchecked()
                .as_ref()
        };

        name_cell.bind(&model, list_cell, expander);
    });

    factory.connect_unbind(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        let expander = unsafe {
            list_item
                .data::<gtk::TreeExpander>("expander")
                .unwrap_unchecked()
                .as_ref()
        };
        expander.set_list_row(None);

        let name_cell = unsafe {
            list_item
                .data::<NameCell>("list_item")
                .unwrap_unchecked()
                .as_ref()
        };
        name_cell.unbind();
    });

    factory.connect_teardown(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };

        unsafe {
            let _ = list_item.steal_data::<gtk::TreeExpander>("expander");
            let _ = list_item.steal_data::<ListCell>("list_cell");
            let _ = list_item.steal_data::<NameCell>("list_item");
        }
    });

    factory
}

pub fn sorter(column_view: &gtk::ColumnView) -> impl IsA<gtk::Sorter> {
    let column_view = column_view.downgrade();
    gtk::CustomSorter::new(move |lhs, rhs| {
        let Some(column_view) = column_view.upgrade() else {
            return Ordering::Equal.into();
        };

        compare_column_entries_by(lhs, rhs, sort_order(&column_view), |lhs, rhs| {
            lhs.name().to_lowercase().cmp(&rhs.name().to_lowercase())
        })
        .into()
    })
}
