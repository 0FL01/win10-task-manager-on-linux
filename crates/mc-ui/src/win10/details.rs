/* crates/mc-ui/src/win10/details.rs
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

//! Details tab: flat list of every process (docs/win10-spec/spec.md, "Details tab").

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{Align, Id, Pos2, Rect, Ui};
use mc_core::config::SortOrder;
use mc_core::i18n::{i18n, n_};
use mc_core::table::RowIcon;
use mc_magpie::{Readings, Request};

use super::flatlist::{FlatColumn, FlatList, FlatRow};
use super::menu;
use crate::cx::{Command, Cx};

/// Name, PID, Status, User name, CPU, Memory (separators at x 146, 196, 713, 793, 833, 913)
const WIDTHS: [f32; 10] = [
    145.,
    49.,
    516.,
    79.,
    39.,
    79.,
    72.,
    super::metrics::GPU_ENGINE_W,
    super::metrics::GPU_MEMORY_W,
    super::metrics::GPU_MEMORY_W,
];
/// Persistent column ids (settings `details-page-*`)
const IDS: [&str; 10] = [
    "name",
    "pid",
    "status",
    "user",
    "cpu",
    "memory",
    "gpu",
    "gpu_engine",
    "dedicated_gpu_memory",
    "shared_gpu_memory",
];

struct Entry {
    pid: u32,
    name: String,
    status: String,
    user: String,
    cpu: f32,
    memory: u64,
    gpu: mc_core::gpu::GpuStats,
    icon: RowIcon,
}

fn status_text(state: i32) -> String {
    use mc_magpie::magpie_types::processes::ProcessState as S;
    i18n(match S::try_from(state).unwrap_or(S::Unknown) {
        S::Running => n_("Running"),
        S::Sleeping | S::Waking | S::Parked => n_("Sleeping"),
        S::SleepingUninterruptible => n_("Waiting"),
        S::Stopped | S::Tracing => n_("Suspended"),
        S::Zombie | S::Dead | S::WakeKill => n_("Zombie"),
        S::Unknown => n_("Unknown"),
    })
}

/// "25,364 K": memory in KiB with thousands separators.
pub fn kib(bytes: u64) -> String {
    let digits = (bytes / 1024).to_string();
    let mut out = String::with_capacity(digits.len() + 4);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out.push_str(" K");
    out
}

pub struct DetailsPage {
    list: FlatList,
    entries: Vec<Entry>,
    rows: Vec<FlatRow>,
    menu: Option<(String, Pos2)>,
    header_menu: Option<Pos2>,
}

impl Default for DetailsPage {
    fn default() -> Self {
        Self {
            list: FlatList::new("details", &IDS, &WIDTHS, 0),
            entries: vec![],
            rows: vec![],
            menu: None,
            header_menu: None,
        }
    }
}

impl DetailsPage {
    pub fn update(&mut self, readings: &Readings, gpu: &mc_core::gpu::GpuSnapshot) {
        let app_of: HashMap<u32, &str> = readings
            .running_apps
            .values()
            .flat_map(|app| app.pids.iter().map(move |&pid| (pid, app.id.as_str())))
            .collect();
        self.entries = readings
            .running_processes
            .values()
            .map(|p| Entry {
                pid: p.pid,
                name: p.name.clone(),
                status: status_text(p.state),
                user: readings
                    .process_users
                    .get(&p.pid)
                    .cloned()
                    .unwrap_or_else(|| i18n("N/A")),
                cpu: p.usage_stats.cpu_usage,
                memory: p.usage_stats.memory_usage,
                gpu: gpu.processes.get(&p.pid).cloned().unwrap_or_default(),
                icon: app_of
                    .get(&p.pid)
                    .map_or(RowIcon::Default, |id| RowIcon::App((*id).to_owned())),
            })
            .collect();
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let column = self.list.sort_column;
        self.entries.sort_by(|a, b| {
            let known = |e: &Entry| match column {
                6 => e.gpu.usage.percent.is_some(),
                7 => e.gpu.usage.engine.is_some(),
                8 => e.gpu.dedicated.is_some(),
                9 => e.gpu.shared.is_some(),
                _ => true,
            };
            if column >= 6 && known(a) != known(b) {
                return known(b).cmp(&known(a));
            }
            let ordering = match column {
                1 => a.pid.cmp(&b.pid),
                2 => a.status.cmp(&b.status),
                3 => a.user.cmp(&b.user),
                4 => a.cpu.partial_cmp(&b.cpu).unwrap_or(Ordering::Equal),
                5 => a.memory.cmp(&b.memory),
                6 => a
                    .gpu
                    .usage
                    .percent
                    .partial_cmp(&b.gpu.usage.percent)
                    .unwrap_or(Ordering::Equal),
                7 => a.gpu.usage.engine.cmp(&b.gpu.usage.engine),
                8 => a.gpu.dedicated.cmp(&b.gpu.dedicated),
                9 => a.gpu.shared.cmp(&b.gpu.shared),
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
            .then(a.pid.cmp(&b.pid));
            if self.list.sort_order == SortOrder::Descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        let mut tooltips: HashSet<Arc<str>> = HashSet::new();
        let mut shared = |text: String| -> Option<Arc<str>> {
            if let Some(text) = tooltips.get(text.as_str()) {
                return Some(text.clone());
            }
            let text: Arc<str> = text.into();
            tooltips.insert(text.clone());
            Some(text)
        };
        self.rows = self
            .entries
            .iter()
            .map(|e| FlatRow {
                key: e.pid.to_string(),
                icon: Some(e.icon.clone()),
                cells: vec![
                    e.name.clone(),
                    e.pid.to_string(),
                    e.status.clone(),
                    e.user.clone(),
                    format!("{:02}", e.cpu.round() as u32),
                    kib(e.memory),
                    e.gpu.usage.text(),
                    e.gpu.usage.engine_text(),
                    e.gpu.dedicated.map_or_else(|| "—".into(), kib),
                    e.gpu.shared.map_or_else(|| "—".into(), kib),
                ],
                tooltips: vec![
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    shared(e.gpu.usage.tooltip()),
                    shared(e.gpu.usage.tooltip()),
                    shared(e.gpu.memory_tooltip(e.gpu.dedicated_state)),
                    shared(e.gpu.memory_tooltip(e.gpu.shared_state)),
                ],
            })
            .collect();
    }

    /// Saved sort, column widths and order.
    pub fn restore(&mut self, config: &mc_core::config::Config) {
        self.list.set_visible(&config.details_page_visible_columns);
        self.list.restore(
            &config.details_page_column_widths,
            &config.details_page_column_order,
            &config.details_page_sorting_column_name,
            config.details_page_sorting_order,
        );
    }

    pub fn selected_pid(&self) -> Option<u32> {
        self.list.selected.as_deref().and_then(|k| k.parse().ok())
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx, content: Rect) {
        let columns = [
            (i18n("Name"), Align::Min),
            (i18n("PID"), Align::Min),
            (i18n("Status"), Align::Min),
            (i18n("User name"), Align::Min),
            (i18n("CPU"), Align::Max),
            (i18n("Memory"), Align::Max),
            (i18n("GPU"), Align::Max),
            (i18n("GPU engine"), Align::Min),
            (i18n("Dedicated GPU memory"), Align::Max),
            (i18n("Shared GPU memory"), Align::Max),
        ]
        .into_iter()
        .zip(WIDTHS.iter())
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
            c.details_page_column_widths = widths;
            c.details_page_column_order = order;
            c.details_page_sorting_column_name = sort;
            c.details_page_sorting_order = self.list.sort_order;
        }
        if let Some(menu) = response.context_menu {
            self.menu = Some(menu);
        }
        if let Some(pos) = response.header_context_menu {
            self.header_menu = Some(pos);
        }
        if let Some(pos) = self.header_menu {
            let mut open = true;
            menu::context(ui, Id::new("details-columns-menu"), pos, &mut open, |ui| {
                for (index, column) in columns.iter().enumerate() {
                    if index == 0 {
                        ui.add_enabled_ui(false, |ui| {
                            menu::check(ui, &column.title, true);
                        });
                    } else if menu::check(ui, &column.title, self.list.column_visible(index)) {
                        self.list.toggle_column(index);
                        cx.config.details_page_visible_columns = self.list.visible_columns();
                    }
                }
            });
            if !open {
                self.header_menu = None;
            }
        }
        if let Some((key, pos)) = self.menu.clone() {
            let pid: u32 = key.parse().unwrap_or_default();
            let mut open = true;
            menu::context(ui, Id::new("details-menu"), pos, &mut open, |ui| {
                if menu::item(ui, &i18n("End task"), "Del", true) {
                    cx.send(Command::Magpie(Request::TerminateProcesses(vec![pid])));
                }
                if menu::item(ui, &i18n("Kill"), "", true) {
                    cx.send(Command::Magpie(Request::KillProcesses(vec![pid])));
                }
                menu::separator(ui);
                if menu::item(ui, &i18n("Copy"), "", true) {
                    cx.send(Command::CopyText(key.clone()));
                }
            });
            if !open {
                self.menu = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_columns_sort_numeric_values_and_preserve_missing_data() {
        let mut readings = mc_magpie::synthetic_readings(3, 6);
        for gpu in readings.gpus.values_mut() {
            let telemetry = gpu.telemetry.as_mut().unwrap();
            telemetry.observations.retain(|p| p.pid != 100);
            telemetry
                .clients
                .retain(|c| c.owners.iter().all(|p| p.pid != 100));
        }
        let gpu =
            mc_core::gpu::GpuSnapshot::from_readings(&readings, &mut Default::default(), 1000);
        let mut details = super::DetailsPage::default();
        for sort_order in [
            mc_core::config::SortOrder::Ascending,
            mc_core::config::SortOrder::Descending,
        ] {
            details.list.sort_column = 6;
            details.list.sort_order = sort_order;
            details.update(&readings, &gpu);
            assert_eq!(details.entries.last().unwrap().pid, 100);
            assert_eq!(details.rows.last().unwrap().cells[6], "—");
            let values: Vec<_> = details
                .entries
                .iter()
                .filter_map(|e| e.gpu.usage.percent)
                .collect();
            assert!(values.windows(2).all(|v| {
                if sort_order == mc_core::config::SortOrder::Ascending {
                    v[0] <= v[1]
                } else {
                    v[0] >= v[1]
                }
            }));
        }
    }
    #[test]
    fn kib_groups_thousands() {
        assert_eq!(super::kib(25_972_736), "25,364 K");
        assert_eq!(super::kib(1024), "1 K");
        assert_eq!(super::kib(1_048_576_000), "1,024,000 K");
    }
}
