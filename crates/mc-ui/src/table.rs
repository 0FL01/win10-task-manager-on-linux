/* crates/mc-ui/src/table.rs
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

//! Virtualised tree table shared by the Apps and Services pages. The tree and the flattened
//! row list are rebuilt only when data, sorting, search, filter or expansion change; each frame
//! only lays out the rows inside the viewport.

use std::collections::HashSet;

use egui::{Key, Sense, Ui, Vec2};
use mc_core::config::{Config, SortOrder};
use mc_core::format::{DataType, UnitSettings, to_human_readable_nice};
use mc_core::table::{
    Column, ColumnTotals, Node, RowIcon, RowKind, Tree, ViewOptions, VisibleRow, toggle_expanded,
};
use mc_magpie::{Readings, Request};

use crate::cx::{Command, Cx};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Apps,
    Services,
}

pub struct TableView {
    pub(crate) kind: TableKind,
    pub(crate) tree: Tree,
    pub(crate) rows: Vec<VisibleRow>,
    pub(crate) rows_dirty: bool,
    pub(crate) expanded: HashSet<String>,
    /// Bumped whenever `expanded` changes (the app saves the stable keys then)
    pub(crate) expanded_version: u64,
    pub options: ViewOptions,
    pub selected: Option<String>,
    pub(crate) hide_network: bool,
    /// Per-process network values count TCP only (see `Readings::network_tcp_only`)
    pub(crate) network_tcp_only: bool,
    pub(crate) hide_swap: bool,
    pub(crate) totals: ColumnTotals,
    pub(crate) units: UnitSettings,
    pub(crate) scroll_to: Option<usize>,
    /// Capacities used for heat levels (bytes)
    pub(crate) mem_total: u64,
    pub(crate) swap_total: u64,
    pub(crate) gpu_mem_total: u64,
}

fn sort_from_config(config: &Config, kind: TableKind) -> (Option<Column>, SortOrder) {
    if !config.apps_page_remember_sorting {
        return (None, SortOrder::None);
    }
    let (name, order) = match kind {
        TableKind::Apps => (
            &config.apps_page_sorting_column_name,
            config.apps_page_sorting_order,
        ),
        TableKind::Services => (
            &config.services_page_sorting_column_name,
            config.services_page_sorting_order,
        ),
    };
    (Column::from_id(name), order)
}

impl TableView {
    pub fn new(kind: TableKind, config: &mut Config) -> Self {
        let (sort_column, sort_order) = sort_from_config(config, kind);
        if !config.apps_page_remember_sorting {
            match kind {
                TableKind::Apps => {
                    config.apps_page_sorting_column_name.clear();
                    config.apps_page_sorting_order = SortOrder::Ascending;
                }
                TableKind::Services => {
                    config.services_page_sorting_column_name.clear();
                    config.services_page_sorting_order = SortOrder::Ascending;
                }
            }
        }
        Self {
            kind,
            tree: Tree::default(),
            rows: vec![],
            rows_dirty: true,
            // Groups and apps keep their expansion across restarts (process keys contain pids)
            expanded: match kind {
                TableKind::Apps => config
                    .apps_page_expanded
                    .split(';')
                    .filter(|k| !k.is_empty())
                    .map(str::to_owned)
                    .collect(),
                TableKind::Services => HashSet::new(),
            },
            expanded_version: 0,
            options: ViewOptions {
                sort_column,
                sort_order: if sort_column.is_some() {
                    sort_order
                } else {
                    SortOrder::None
                },
                ..Default::default()
            },
            selected: None,
            hide_network: false,
            network_tcp_only: false,
            hide_swap: false,
            totals: ColumnTotals::default(),
            units: config.units(),
            scroll_to: None,
            mem_total: 0,
            swap_total: 0,
            gpu_mem_total: 0,
        }
    }

    pub fn update(&mut self, readings: &Readings, config: &Config) {
        let merged = config.apps_page_merged_process_stats;
        self.tree = match self.kind {
            TableKind::Apps => Tree::apps(readings, merged),
            TableKind::Services => Tree::services(readings, merged),
        };
        self.totals = ColumnTotals::from_readings(readings);
        self.hide_network = readings.network_stats_error.is_some();
        self.network_tcp_only = readings.network_tcp_only;
        self.hide_swap = readings.mem_info.swap_total == 0;
        self.mem_total = readings.mem_info.mem_total;
        self.swap_total = readings.mem_info.swap_total;
        self.gpu_mem_total = readings
            .gpus
            .values()
            .map(|g| g.total_memory.unwrap_or(0))
            .sum();
        self.units = config.units();
        self.rows_dirty = true;
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn node(&self, key: &str) -> Option<&Node> {
        self.tree.find_by_key(key).map(|i| &self.tree.nodes[i])
    }

    pub fn selected_node(&self) -> Option<&Node> {
        self.selected.as_deref().and_then(|k| self.node(k))
    }

    pub fn set_search(&mut self, search: &str) {
        if self.options.search != search {
            self.options.search = search.to_owned();
            self.rows_dirty = true;
        }
    }

    pub fn set_options_dirty(&mut self) {
        self.rows_dirty = true;
    }

    /// Expanded/collapsed keys that survive a restart (groups and apps), `;` separated.
    pub fn persistent_expanded(&self) -> String {
        let mut keys: Vec<&str> = self
            .expanded
            .iter()
            .map(String::as_str)
            .filter(|k| !k.contains("/p"))
            .collect();
        keys.sort_unstable();
        keys.join(";")
    }

    pub fn expand_all(&mut self) {
        self.expanded_version += 1;
        self.expanded = self
            .tree
            .nodes
            .iter()
            .filter(|n| n.kind != RowKind::SectionHeader && !n.children.is_empty())
            .map(|n| n.key.clone())
            .collect();
        self.rows_dirty = true;
    }

    pub fn collapse_all(&mut self) {
        self.expanded_version += 1;
        self.expanded.clear();
        for root in &self.tree.roots {
            let node = &self.tree.nodes[*root];
            self.expanded.insert(format!("!{}", node.key));
        }
        self.rows_dirty = true;
    }

    pub(crate) fn refresh_rows(&mut self) {
        if self.rows_dirty {
            self.rows = self.tree.flatten(&self.expanded, &self.options);
            self.rows_dirty = false;
        }
    }

    pub(crate) fn cell_text(&self, column: Column, node: &Node) -> String {
        let s = &node.stats;
        let u = &self.units;
        if node.kind == RowKind::SectionHeader {
            return String::new();
        }
        match column {
            Column::Name => node.name.clone(),
            Column::Pid => match node.kind {
                RowKind::Process => node.pid.to_string(),
                _ => String::new(),
            },
            Column::Cpu => format!("{}%", s.cpu_usage.round() as u32),
            Column::Memory => {
                to_human_readable_nice(s.memory_usage as f32, &DataType::MemoryBytes, u)
            }
            Column::Swap => to_human_readable_nice(s.swap_usage as f32, &DataType::MemoryBytes, u),
            Column::Drive => {
                to_human_readable_nice(s.disk_usage, &DataType::DriveBytesPerSecond, u)
            }
            Column::Network => {
                to_human_readable_nice(s.network_usage, &DataType::NetworkBytesPerSecond, u)
            }
            Column::Gpu => format!("{}%", s.gpu_usage.round() as u32),
            Column::GpuMemory => {
                to_human_readable_nice(s.gpu_memory_usage as f32, &DataType::MemoryBytes, u)
            }
        }
    }

    /// Sorts by `column`; a new column starts with `first`, the current one flips.
    pub(crate) fn sort_by_starting(
        &mut self,
        column: Column,
        first: SortOrder,
        config: &mut Config,
    ) {
        let order = if self.options.sort_column == Some(column) {
            match self.options.sort_order {
                SortOrder::Ascending => SortOrder::Descending,
                _ => SortOrder::Ascending,
            }
        } else {
            first
        };
        self.options.sort_column = Some(column);
        self.options.sort_order = order;
        self.rows_dirty = true;
        match self.kind {
            TableKind::Apps => {
                config.apps_page_sorting_column_name = column.id().into();
                config.apps_page_sorting_order = order;
            }
            TableKind::Services => {
                config.services_page_sorting_column_name = column.id().into();
                config.services_page_sorting_order = order;
            }
        }
    }

    /// PIDs targeted by process actions for the selected row.
    pub fn selected_pids(&self) -> Vec<u32> {
        let Some(index) = self
            .selected
            .as_deref()
            .and_then(|k| self.tree.find_by_key(k))
        else {
            return vec![];
        };
        let node = &self.tree.nodes[index];
        match node.kind {
            RowKind::Process => vec![node.pid],
            RowKind::App => node
                .children
                .iter()
                .map(|&c| self.find_stoppable_child(c))
                .collect(),
            RowKind::Service if node.pid != 0 => vec![node.pid],
            _ => vec![],
        }
    }

    /// Descends past `bwrap` sandbox wrappers (as the GTK `find_stoppable_child`).
    fn find_stoppable_child(&self, mut index: usize) -> u32 {
        loop {
            let node = &self.tree.nodes[index];
            if node.name == "bwrap"
                && let Some(&first) = node.children.first()
            {
                index = first;
                continue;
            }
            return node.pid;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        self.refresh_rows();
        if self.rows.is_empty() {
            return;
        }
        let current = self.selected.as_deref().and_then(|k| {
            self.rows
                .iter()
                .position(|r| self.tree.nodes[r.node].key == k)
        });
        let next = match current {
            Some(i) => (i as isize + delta).clamp(0, self.rows.len() as isize - 1) as usize,
            None => 0,
        };
        self.selected = Some(self.tree.nodes[self.rows[next].node].key.clone());
        self.scroll_to = Some(next);
    }

    pub(crate) fn set_expanded(&mut self, key: &str, expand: bool) {
        let Some(index) = self.tree.find_by_key(key) else {
            return;
        };
        let node = &self.tree.nodes[index];
        let is_expanded = if node.kind == RowKind::SectionHeader {
            !self.expanded.contains(&format!("!{}", node.key))
        } else {
            self.expanded.contains(&node.key)
        };
        if is_expanded != expand && !node.children.is_empty() {
            toggle_expanded(&mut self.expanded, node);
            self.expanded_version += 1;
            self.rows_dirty = true;
        }
    }

    pub(crate) fn handle_keys(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() || ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        let (up, down, left, right) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
            )
        });
        if up {
            self.move_selection(-1);
        }
        if down {
            self.move_selection(1);
        }
        if let Some(key) = self.selected.clone() {
            if right {
                self.set_expanded(&key, true);
            }
            if left {
                self.set_expanded(&key, false);
            }
        }
    }

    // Actions bound to buttons / shortcuts

    pub fn process_action(&self, make: fn(Vec<u32>) -> Request) -> Option<Command> {
        let node = self.selected_node()?;
        if !matches!(node.kind, RowKind::App | RowKind::Process) {
            return None;
        }
        let pids = self.selected_pids();
        (!pids.is_empty()).then(|| Command::Magpie(make(pids)))
    }

    pub fn service_action(&self, make: fn(u64) -> Request) -> Option<Command> {
        let id = self.selected_node()?.service.as_ref()?.id;
        (self.selected_node()?.kind == RowKind::Service).then(|| Command::Magpie(make(id)))
    }
}

/// Allocates `size`² points and paints the row icon (dialogs).
pub fn row_icon(ui: &mut Ui, cx: &mut Cx, icon: &RowIcon, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    match icon {
        RowIcon::Service(_) => {
            crate::win10::flatlist::gear(ui.painter(), rect, crate::win10::palette::SERVICES_ICON)
        }
        _ => crate::win10::processes::row_icon(ui.painter(), ui.ctx(), cx, icon, rect),
    }
}
