/* services_page/actions.rs
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
use glib::{g_critical, WeakRef};
use gtk::{gio, glib};

use crate::app;
use crate::magpie_client::MagpieClient;
use crate::table_view::{ContentType, RowModel, ServiceDetailsDialog, TableView};

macro_rules! new_action {
    ($name: literal, $column_view: expr, $cond: expr) => {{
        use gtk::prelude::*;
        use $crate::table_view::ContentType;

        let action = gio::SimpleAction::new($name, None);

        let selected_item = $column_view.selected_item();
        action.set_enabled(
            selected_item.content_type() == ContentType::Service && ($cond)(&selected_item),
        );

        $column_view.connect_selected_item_notify({
            let action = action.downgrade();
            move |column_view| {
                let Some(action) = action.upgrade() else {
                    return;
                };

                let selected_item = column_view.selected_item();
                action.set_enabled(
                    selected_item.content_type() == ContentType::Service && ($cond)(&selected_item),
                );
            }
        });

        $column_view.connect_selected_item_running_notify({
            let action = action.downgrade();
            move |column_view| {
                let Some(action) = action.upgrade() else {
                    return;
                };

                let selected_item = column_view.selected_item();
                action.set_enabled(
                    selected_item.content_type() == ContentType::Service && ($cond)(&selected_item),
                );
            }
        });

        action.connect_activate({
            let column_view = $column_view.downgrade();
            move |_action, _| {
                make_magpie_request(&column_view, |magpie, service_id| {
                    paste::paste! {
                       magpie.[<$name _service>](service_id)
                    }
                });
            }
        });
        action
    }};
}

pub mod apps {
    pub use crate::apps_page::actions::*;
}

pub fn action_start(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("start", column_view_frame, |selected_item: &RowModel| {
        !selected_item.service_running()
    })
}

pub fn action_stop(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("stop", column_view_frame, |selected_item: &RowModel| {
        selected_item.service_running()
    })
}

pub fn action_restart(column_view_frame: &TableView) -> gio::SimpleAction {
    new_action!("restart", column_view_frame, |selected_item: &RowModel| {
        selected_item.service_running()
    })
}

pub fn action_details(column_view_frame: &TableView) -> gio::SimpleAction {
    let action = gio::SimpleAction::new("details", None);
    action.set_enabled(column_view_frame.selected_item().content_type() == ContentType::Service);

    column_view_frame.connect_selected_item_notify({
        let action = action.downgrade();
        move |column_view| {
            let Some(action) = action.upgrade() else {
                return;
            };

            let selected_item = column_view.selected_item();
            action.set_enabled(selected_item.content_type() == ContentType::Service);
        }
    });

    action.connect_activate({
        let column_view_frame = column_view_frame.downgrade();
        move |_action, _| {
            let Some(column_view_frame) = column_view_frame.upgrade() else {
                return;
            };

            let selected_item = column_view_frame.selected_item();
            if selected_item.content_type() == ContentType::Service {
                let dialog = ServiceDetailsDialog::new(&column_view_frame);
                dialog.present(Some(&column_view_frame));
            }
        }
    });
    action
}

fn make_magpie_request(column_view_frame: &WeakRef<TableView>, request: fn(&MagpieClient, u64)) {
    let app = app!();
    let Some(column_view_frame) = column_view_frame.upgrade() else {
        g_critical!(
            "MissionCenter::ServiceActions",
            "Failed to get ColumnView instance for action"
        );
        return;
    };

    let selected_item = column_view_frame.selected_item();
    match app.sys_info() {
        Ok(sys_info) => {
            request(&sys_info, selected_item.service_id());
        }
        Err(e) => {
            g_critical!(
                "MissionCenter::ServiceActionBar",
                "Failed to get sys_info from MissionCenterApplication: {e}",
            );
        }
    };
}
