/* crates/mc-ui/src/win10/services.rs
 *
 * Copyright 2026 Mission Center Developers
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

//! Services tab: flat list of user and system services (docs/win10-spec/spec.md, "Services tab").

use egui::{Align, Id, Pos2, Rect, Ui};
use mc_core::config::SortOrder;
use mc_core::i18n::i18n;
use mc_core::table::RowKind;
use mc_magpie::Request;

use super::flatlist::{FlatColumn, FlatList, FlatRow};
use super::menu;
use crate::cx::{Command, Cx, Dialog};
use crate::table::TableView;

/// Name, PID, Description, Status, Group (separators at x 181, 231, 813, 913, 1013)
const WIDTHS: [f32; 5] = [180., 49., 581., 99., 99.];
/// Persistent column ids (settings `services-page-*`)
const IDS: [&str; 5] = ["name", "pid", "description", "status", "group"];

struct Entry {
    key: String,
    id: u64,
    name: String,
    pid: u32,
    description: String,
    status: String,
    running: bool,
    group: String,
}

pub struct ServicesView {
    list: FlatList,
    entries: Vec<Entry>,
    rows: Vec<FlatRow>,
    menu: Option<(String, Pos2)>,
}

impl Default for ServicesView {
    fn default() -> Self {
        Self {
            list: FlatList::new("services", &IDS, &WIDTHS, 0),
            entries: vec![],
            rows: vec![],
            menu: None,
        }
    }
}

impl ServicesView {
    /// Saved sort, column widths and order.
    pub fn restore(&mut self, config: &mc_core::config::Config) {
        self.list.restore(
            &config.services_page_column_widths,
            &config.services_page_column_order,
            &config.services_page_sorting_column_name,
            config.services_page_sorting_order,
        );
    }

    /// Rebuilds the rows from the services table (after it received new readings).
    pub fn update(&mut self, table: &TableView) {
        self.entries = table
            .tree()
            .nodes
            .iter()
            .filter(|n| n.kind == RowKind::Service)
            .filter_map(|n| {
                let info = n.service.as_ref()?;
                let status = if info.running {
                    i18n("Running")
                } else if info.failed {
                    i18n("Failed")
                } else if info.enabled {
                    i18n("Stopped")
                } else {
                    i18n("Disabled")
                };
                Some(Entry {
                    key: n.key.clone(),
                    id: info.id,
                    name: n.name.clone(),
                    pid: n.pid,
                    description: info.description.clone(),
                    status,
                    running: info.running,
                    group: info.group.clone(),
                })
            })
            .collect();
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let column = self.list.sort_column;
        self.entries.sort_by(|a, b| {
            let ordering = match column {
                1 => a.pid.cmp(&b.pid),
                2 => a.description.cmp(&b.description),
                3 => a.status.cmp(&b.status),
                4 => a.group.cmp(&b.group),
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
            .then(a.key.cmp(&b.key));
            if self.list.sort_order == SortOrder::Descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        self.rows = self
            .entries
            .iter()
            .map(|e| FlatRow {
                key: e.key.clone(),
                icon: Some(mc_core::table::RowIcon::Service(
                    mc_core::table::ServiceIcon::Running,
                )),
                cells: vec![
                    e.name.clone(),
                    if e.pid == 0 {
                        String::new()
                    } else {
                        e.pid.to_string()
                    },
                    e.description.clone(),
                    e.status.clone(),
                    e.group.clone(),
                ],
                tooltips: vec![],
            })
            .collect();
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx, content: Rect) {
        let columns = [
            (i18n("Name"), Align::Min),
            (i18n("PID"), Align::Min),
            (i18n("Description"), Align::Min),
            (i18n("Status"), Align::Min),
            (i18n("Group"), Align::Min),
        ]
        .into_iter()
        .zip(self.list.list.widths.iter())
        .map(|((title, align), &width)| FlatColumn {
            title,
            width,
            align,
        })
        .collect::<Vec<_>>();
        let response = self.list.ui(ui, cx, content, &columns, &self.rows);
        if response.sorted {
            self.rebuild();
        }
        if response.changed {
            let (widths, order, sort) = self.list.saved();
            let c = &mut *cx.config;
            c.services_page_column_widths = widths;
            c.services_page_column_order = order;
            c.services_page_sorting_column_name = sort;
            c.services_page_sorting_order = self.list.sort_order;
        }
        if let Some(key) = response.double_clicked {
            cx.send(Command::OpenDialog(Dialog::ServiceDetails(key)));
        }
        if let Some(menu) = response.context_menu {
            self.menu = Some(menu);
        }
        if let Some((key, pos)) = self.menu.clone() {
            let Some(entry) = self.entries.iter().find(|e| e.key == key) else {
                self.menu = None;
                return;
            };
            let (id, running) = (entry.id, entry.running);
            let mut open = true;
            menu::context(ui, Id::new("services-menu"), pos, &mut open, |ui| {
                if menu::item(ui, &i18n("Start"), "", !running) {
                    cx.send(Command::Magpie(Request::StartService(id)));
                }
                if menu::item(ui, &i18n("Stop"), "", running) {
                    cx.send(Command::Magpie(Request::StopService(id)));
                }
                if menu::item(ui, &i18n("Restart"), "", running) {
                    cx.send(Command::Magpie(Request::RestartService(id)));
                }
                menu::separator(ui);
                if menu::item(ui, &i18n("Details"), "", true) {
                    cx.send(Command::OpenDialog(Dialog::ServiceDetails(key.clone())));
                }
            });
            if !open {
                self.menu = None;
            }
        }
    }
}
