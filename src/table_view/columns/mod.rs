/* table_view/columns/mod.rs
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
use gtk::glib;
use gtk::prelude::*;
use std::cmp::Ordering;

use super::settings::get_hide_is_zero;
use crate::i18n::i18n;
use crate::table_view::row_model::RowModel;

pub use cpu::label_formatter as cpu_label_formatter;
pub use cpu::list_item_factory as cpu_list_item_factory;
pub use cpu::sorter as cpu_sorter;
pub use drive::label_formatter as drive_label_formatter;
pub use drive::list_item_factory as drive_list_item_factory;
pub use drive::sorter as drive_sorter;
pub use gpu::label_formatter as gpu_label_formatter;
pub use gpu::list_item_factory as gpu_list_item_factory;
pub use gpu::sorter as gpu_sorter;
pub use gpu_memory::label_formatter as gpu_memory_label_formatter;
pub use gpu_memory::list_item_factory as gpu_memory_list_item_factory;
pub use gpu_memory::sorter as gpu_memory_sorter;
pub use label_cell::LabelCell;
pub use memory::label_formatter as memory_label_formatter;
pub use memory::list_item_factory as memory_list_item_factory;
pub use memory::sorter as memory_sorter;
pub use name::list_item_factory as name_list_item_factory;
pub use name::sorter as name_sorter;
pub use name_cell::NameCell;
pub use network::label_formatter as network_label_formatter;
pub use network::list_item_factory as network_list_item_factory;
pub use network::sorter as network_sorter;
pub use pid::list_item_factory as pid_list_item_factory;
pub use pid::sorter as pid_sorter;
pub use swap::label_formatter as swap_label_formatter;
pub use swap::list_item_factory as swap_list_item_factory;
pub use swap::sorter as swap_sorter;

mod cpu;
mod drive;
mod gpu;
mod gpu_memory;
mod label_cell;
mod memory;
mod name;
mod name_cell;
mod network;
mod pid;
mod swap;

pub const IS_ZERO_OPACITY: f64 = 0.3;

#[macro_export]
macro_rules! label_cell_factory {
    ($property: literal, $setter: expr) => {{
        label_cell_factory!($property, ContentType::SectionHeader, $setter)
    }};

    ($property: literal, $skip_content: pat, $setter: expr) => {{
        use gtk::prelude::*;

        use crate::table_view::row_model::{ContentType, RowModel};

        let factory = gtk::SignalListItemFactory::new();

        factory.connect_setup(|_, list_item| {
            let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                return;
            };

            let label = LabelCell::new();
            let expander = gtk::TreeExpander::new();
            expander.set_child(Some(&label));

            expander.set_hide_expander(true);
            expander.set_indent_for_icon(false);
            expander.set_indent_for_depth(false);
            expander.set_halign(gtk::Align::End);

            list_item.set_child(Some(&expander));

            unsafe {
                list_item.set_data("expander", expander);
                list_item.set_data("label", label);
            }
        });

        factory.connect_bind(move |_, list_item| {
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

            let label = unsafe {
                list_item
                    .data::<LabelCell>("label")
                    .unwrap_unchecked()
                    .as_ref()
            };

            match model.content_type() {
                $skip_content => {
                    label.set_label("");
                    return;
                }
                _ => {}
            }

            let value = model.property_value($property);
            ($setter)(&label, value);

            label.bind(&model, $property, $setter);
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

            let label = unsafe {
                list_item
                    .data::<LabelCell>("label")
                    .unwrap_unchecked()
                    .as_ref()
            };
            label.unbind();
        });

        factory.connect_teardown(|_, list_item| {
            let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                return;
            };

            unsafe {
                let _ = list_item.steal_data::<gtk::TreeExpander>("expander");
                let _ = list_item.steal_data::<gtk::Label>("label");
            }
        });

        factory
    }};
}

pub fn adjust_view_header_alignment(column_view_titlebar: Option<gtk::Widget>) {
    let mut column_view_title = column_view_titlebar.and_then(|w| w.first_child());
    loop {
        let Some(view_title) = column_view_title.take() else {
            break;
        };
        column_view_title = view_title.next_sibling();

        let Some(container) = view_title.first_child() else {
            continue;
        };

        let Some(label) = container
            .first_child()
            .and_then(|l| l.downcast::<gtk::Label>().ok())
        else {
            continue;
        };

        // The `Name` column should be default aligned
        // The column that contains the context menu button should be default aligned
        if label.label().starts_with(&i18n("Name")) {
            label.set_margin_start(10);
            continue;
        }

        container.set_hexpand(true);
        container.set_width_request(75);
        label.set_halign(gtk::Align::End);
        label.set_justify(gtk::Justification::Right);

        if let Some(arrow) = label.next_sibling() {
            if let Some(container) = container.downcast_ref::<gtk::Box>() {
                container.reorder_child_after(&label, Some(&arrow));
                arrow.set_halign(gtk::Align::Start);
                arrow.set_hexpand(true);
            }
        }
    }
}

#[inline]
fn convert_order(sort_order: gtk::SortType, ordering: Ordering) -> Ordering {
    match ordering {
        Ordering::Less => {
            if sort_order == gtk::SortType::Ascending {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        Ordering::Equal => Ordering::Equal,
        Ordering::Greater => {
            if sort_order == gtk::SortType::Ascending {
                Ordering::Greater
            } else {
                Ordering::Less
            }
        }
    }
}

fn compare_column_entries_by(
    lhs: &glib::Object,
    rhs: &glib::Object,
    sort_order: gtk::SortType,
    compare_fn: fn(&RowModel, &RowModel) -> Ordering,
) -> Ordering {
    let Some(lhs) = lhs.downcast_ref::<RowModel>() else {
        return Ordering::Equal.into();
    };

    let Some(rhs) = rhs.downcast_ref::<RowModel>() else {
        return Ordering::Equal.into();
    };

    match lhs.section_type().cmp(&rhs.section_type()) {
        Ordering::Equal => {
            // continue
        }
        order => return convert_order(sort_order, order),
    }

    match lhs.content_type().cmp(&rhs.content_type()) {
        Ordering::Equal => compare_fn(lhs, rhs),
        order => convert_order(sort_order, order),
    }
}

fn sort_order(column_view: &gtk::ColumnView) -> gtk::SortType {
    column_view
        .sorter()
        .and_downcast_ref::<gtk::ColumnViewSorter>()
        .and_then(|sorter| Some(sorter.primary_sort_order()))
        .unwrap_or(gtk::SortType::Ascending)
}

pub fn set_label_zero(label: &LabelCell, usage: f32) {
    if get_hide_is_zero() && usage.abs() == 0.0 {
        label.set_opacity(IS_ZERO_OPACITY)
    } else {
        label.set_opacity(1.0)
    }
}
