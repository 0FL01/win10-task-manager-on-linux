/* apps_page/actions.rs
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

use adw::prelude::*;
use gtk::gio;

use crate::table_view::ProcessDetailsDialog;
use crate::table_view::TableView;
use crate::table_view::{ContentType, RowModel};

macro_rules! new_action {
    ($name: literal, $column_view: expr, $magpie_function: ident) => {{
        use gtk::prelude::*;
        use $crate::table_view::ContentType;

        let action = gio::SimpleAction::new($name, None);

        let selected_item = $column_view.selected_item();
        action.set_enabled(
            selected_item.content_type() == ContentType::Process
                || selected_item.content_type() == ContentType::App,
        );

        $column_view.connect_selected_item_notify({
            let action = action.downgrade();
            move |column_view| {
                let Some(action) = action.upgrade() else {
                    return;
                };

                let selected_item = column_view.selected_item();
                action.set_enabled(
                    selected_item.content_type() == ContentType::Process
                        || selected_item.content_type() == ContentType::App,
                );
            }
        });

        action.connect_activate({
            let column_view = $column_view.downgrade();
            move |_action, _| {
                let Some(column_view) = column_view.upgrade() else {
                    return;
                };

                let selected_item = column_view.selected_item();
                if selected_item.content_type() != ContentType::Process
                    && selected_item.content_type() != ContentType::App
                {
                    return;
                }

                if let Ok(magpie_client) = $crate::app!().sys_info() {
                    match selected_item.content_type() {
                        ContentType::Process => {
                            magpie_client.$magpie_function(vec![selected_item.pid()]);
                        }
                        ContentType::App => {
                            magpie_client.$magpie_function(app_pids(&selected_item));
                        }
                        _ => {}
                    }
                }
            }
        });
        action
    }};
}

pub fn action_stop(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("stop", column_view_frame, terminate_processes)
}

pub fn action_force_stop(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("force-stop", column_view_frame, kill_processes)
}

pub fn action_suspend(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("suspend", column_view_frame, suspend_processes)
}

pub fn action_continue(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("continue", column_view_frame, continue_processes)
}

pub fn action_hangup(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("hangup", column_view_frame, hangup_processes)
}

pub fn action_interrupt(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("interrupt", column_view_frame, interrupt_processes)
}

pub fn action_user_one(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("user-one", column_view_frame, user_signal_one_processes)
}

pub fn action_user_two(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("user-two", column_view_frame, user_signal_two_processes)
}

pub fn action_details(column_view_frame: &TableView) -> gio::SimpleAction {
    let action = gio::SimpleAction::new("details", None);

    let selected_item = column_view_frame.selected_item();
    action.set_enabled(
        selected_item.content_type() == ContentType::Process
            || selected_item.content_type() == ContentType::App,
    );

    column_view_frame.connect_selected_item_notify({
        let action = action.downgrade();
        move |column_view| {
            let Some(action) = action.upgrade() else {
                return;
            };

            let selected_item = column_view.selected_item();
            action.set_enabled(
                selected_item.content_type() == ContentType::Process
                    || selected_item.content_type() == ContentType::App,
            );
        }
    });

    action.connect_activate({
        let column_view_frame = column_view_frame.downgrade();
        move |_action, _| {
            let Some(column_view_frame) = column_view_frame.upgrade() else {
                return;
            };

            let selected_item = column_view_frame.selected_item();
            if selected_item.content_type() == ContentType::Process
                || selected_item.content_type() == ContentType::App
            {
                let dialog = ProcessDetailsDialog::new(selected_item);
                dialog.present(Some(&column_view_frame));
            }
        }
    });
    action
}

fn app_pids(row_model: &RowModel) -> Vec<u32> {
    let children = row_model.children();
    let mut result = Vec::with_capacity(children.n_items() as usize);

    for i in 0..children.n_items() {
        let Some(child) = children
            .item(i)
            .and_then(|i| i.downcast::<RowModel>().ok())
            .and_then(|rm| find_stoppable_child(&rm))
        else {
            continue;
        };
        result.push(child.pid());
    }

    result
}

fn find_stoppable_child(row_model: &RowModel) -> Option<RowModel> {
    if row_model.name() != "bwrap" {
        return Some(row_model.clone());
    }

    let children = row_model.children();
    for i in 0..children.n_items() {
        let Some(child) = children.item(i).and_then(|i| i.downcast::<RowModel>().ok()) else {
            continue;
        };
        if let Some(rm) = find_stoppable_child(&child) {
            return Some(rm);
        }
    }

    None
}
