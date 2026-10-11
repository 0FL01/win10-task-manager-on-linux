/* crates/mc-ui/src/win10/processes.rs
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

//! Processes tab: Win10 list with a summary header, heat cells and groups
//! (docs/win10-spec/spec.md, "Processes tab").

use egui::{Align, Color32, Id, Pos2, Rect, Ui, pos2};
use mc_core::config::{Config, SortOrder};
use mc_core::config::{parse_widths, serialize_widths};
use mc_core::i18n::{i18n, n_};
use mc_core::table::{
    Column, Node, RowIcon, RowKind, VisibleRow, parse_column_order, serialize_column_order,
};
use mc_magpie::Request;

use super::listview::{self, ColumnHeader, HeaderKind, ListState};
use super::paint::{self, Font};
use super::{menu, metrics as m, palette as pal};
use crate::cx::{Command, Cx, Dialog};
use crate::table::TableView;

pub const ROW_H: f32 = 28.;
/// Shown on the Network column while per-process values come from TCP socket counters
pub const NETWORK_TCP_HINT: &str = n_(
    "Per-process values count TCP traffic of your processes (UDP such as QUIC and other users' processes are not visible). For complete values install nethogs and use Options → Enable per-process network usage…",
);
/// Shown on the Network column while per-process network data is unavailable
pub const NETWORK_HINT: &str = n_(
    "Per-process network usage needs nethogs with packet capture permissions. Install nethogs, then use Options → Enable per-process network usage… and restart the app. The column total shows all network interfaces.",
);
const NAME_W: f32 = 234.;
const VALUE_W: f32 = 72.;
const INDENT: f32 = 22.;
/// Own design: capacities for the heat of rate columns (100 MB/s disk, 100 Mbit/s network)
const DISK_HEAT_FULL: f32 = 100e6;
const NETWORK_HEAT_FULL: f32 = 12.5e6;

/// Numeric columns the user can show (header context menu), in display order.
const OPTIONAL: [Column; 11] = [
    Column::Pid,
    Column::Cpu,
    Column::Memory,
    Column::Swap,
    Column::Drive,
    Column::Network,
    Column::Gpu,
    Column::GpuEngine,
    Column::GpuMemory,
    Column::GpuDedicatedMemory,
    Column::GpuSharedMemory,
];

pub fn column_title(column: Column) -> String {
    match column {
        Column::Name => i18n("Name"),
        Column::Pid => i18n("PID"),
        Column::Cpu => i18n("CPU"),
        Column::Memory => i18n("Memory"),
        Column::Swap => i18n("Swap"),
        Column::Drive => i18n("Disk"),
        Column::Network => i18n("Network"),
        Column::Gpu => i18n("GPU"),
        Column::GpuEngine => i18n("GPU engine"),
        Column::GpuMemory => i18n("GPU Memory"),
        Column::GpuDedicatedMemory => i18n("Dedicated GPU memory"),
        Column::GpuSharedMemory => i18n("Shared GPU memory"),
    }
}

/// One displayed row (group headers dropped when not grouping).
#[derive(Debug, Clone, Copy)]
struct Row {
    visible: VisibleRow,
    level: u16,
}

#[derive(Default)]
pub struct ProcessesPage {
    list: ListState,
    columns: Vec<Column>,
    rows: Vec<Row>,
    rows_source: usize,
    rows_group: bool,
    context: Option<(String, Pos2)>,
    header_menu: Option<Pos2>,
    /// Settings the columns were built from (visible, order, widths)
    columns_key: Option<(String, String, String)>,
}

impl ProcessesPage {
    /// Visible columns in the saved order with the saved widths; recomputed only when the
    /// settings change (a resize in progress lives in `list.widths` until it ends).
    fn sync_columns(&mut self, config: &Config) {
        // Unchanged settings: nothing to do (compared in place, no per-frame allocation)
        if let Some((visible, order, widths)) = &self.columns_key
            && !self.columns.is_empty()
            && *visible == config.apps_page_visible_columns
            && *order == config.apps_page_column_order
            && *widths == config.apps_page_column_widths
        {
            return;
        }
        // "Remember column order" off: default order and widths at start (changes made while
        // running still apply)
        let remember = config.apps_page_remember_column_order || self.columns_key.is_some();
        let (order, widths) = if remember {
            (
                config.apps_page_column_order.clone(),
                config.apps_page_column_widths.clone(),
            )
        } else {
            (String::new(), String::new())
        };
        let key = (config.apps_page_visible_columns.clone(), order, widths);
        let visible: Vec<Column> = config
            .apps_page_visible_columns
            .split(';')
            .filter_map(Column::from_id)
            .collect();
        let mut columns = vec![Column::Name];
        columns.extend(
            parse_column_order(&key.1)
                .into_iter()
                .filter(|c| *c != Column::Name && visible.contains(c)),
        );
        let saved = parse_widths(&key.2);
        self.list.widths = columns
            .iter()
            .map(|c| {
                saved.get(c.id()).copied().unwrap_or(match c {
                    Column::Name => NAME_W,
                    Column::GpuEngine => m::GPU_ENGINE_W,
                    Column::GpuDedicatedMemory | Column::GpuSharedMemory => m::GPU_MEMORY_W,
                    _ => VALUE_W,
                })
            })
            .collect();
        self.columns = columns;
        self.columns_key = Some(key);
    }

    /// Stores a header drop (column order) and finished resizes (widths) in the settings.
    fn save_columns(&self, config: &mut Config, moved: Option<(usize, usize)>, resized: bool) {
        if let Some((from, to)) = moved {
            let mut shown = self.columns.clone();
            listview::apply_move(&mut shown, from, to);
            // Hidden columns keep their relative place after the shown ones
            let mut order = shown;
            for c in parse_column_order(&config.apps_page_column_order) {
                if !order.contains(&c) {
                    order.push(c);
                }
            }
            config.apps_page_column_order = serialize_column_order(&order);
        }
        if resized {
            let mut saved = parse_widths(&config.apps_page_column_widths);
            for (c, w) in self.columns.iter().zip(&self.list.widths) {
                saved.insert(c.id().to_owned(), *w);
            }
            let mut entries: Vec<(&str, f32)> =
                saved.iter().map(|(id, w)| (id.as_str(), *w)).collect();
            entries.sort_by_key(|(id, _)| {
                Column::from_id(id).and_then(|c| Column::ALL.iter().position(|x| *x == c))
            });
            config.apps_page_column_widths = serialize_widths(entries);
        }
    }

    fn sync_rows(&mut self, table: &mut TableView, group: bool) {
        let dirty = table.rows_dirty;
        table.refresh_rows();
        if !dirty
            && self.rows_source == table.rows.len()
            && self.rows_group == group
            && !self.rows.is_empty()
        {
            return;
        }
        self.rows_source = table.rows.len();
        self.rows_group = group;
        self.rows = table
            .rows
            .iter()
            .filter(|r| group || table.tree.nodes[r.node].kind != RowKind::SectionHeader)
            .map(|&visible| Row {
                visible,
                level: if table.tree.nodes[visible.node].kind == RowKind::SectionHeader {
                    0
                } else {
                    visible.depth.saturating_sub(1)
                },
            })
            .collect();
    }

    fn heat_percent(table: &TableView, column: Column, node: &Node) -> Option<f32> {
        let s = &node.stats;
        let share = |v: f64, total: u64| {
            if total == 0 {
                0.
            } else {
                (v * 100. / total as f64) as f32
            }
        };
        Some(match column {
            Column::Cpu => s.cpu_usage,
            Column::Memory => share(s.memory_usage as f64, table.mem_total),
            Column::Swap => share(s.swap_usage as f64, table.swap_total),
            Column::Drive => s.disk_usage * 100. / DISK_HEAT_FULL,
            Column::Network => s.network_usage * 100. / NETWORK_HEAT_FULL,
            Column::Gpu => node.gpu.usage.percent?,
            Column::GpuMemory => share(node.gpu.memory? as f64, table.gpu_mem_total?),
            Column::GpuDedicatedMemory => {
                share(node.gpu.dedicated? as f64, table.gpu_dedicated_total?)
            }
            Column::GpuSharedMemory => share(node.gpu.shared? as f64, table.gpu_shared_total?),
            Column::Name | Column::Pid | Column::GpuEngine => return None,
        })
    }

    fn summary(table: &TableView, column: Column) -> (Option<String>, bool) {
        let t = &table.totals;
        let pct = |v: f32| (Some(format!("{}%", v.round() as u32)), v >= 90.);
        match column {
            Column::Cpu => pct(t.cpu_percent),
            Column::Memory => pct(t.memory_percent),
            Column::Swap => pct(t.swap_percent),
            Column::Drive => pct(t.drive_percent),
            Column::Gpu => (
                Some(table.gpu_summary.clone()),
                t.gpu_percent.is_some_and(|v| v >= 90.),
            ),
            Column::GpuMemory => t
                .gpu_memory_percent
                .map(pct)
                .unwrap_or((Some("—".into()), false)),
            // System-wide (available without per-process data, includes traffic not attributed
            // to a process): link utilisation like the reference, the rate when no link speed is
            // known
            Column::Network if t.system_network_capacity_bytes_ps > 0. => {
                pct(t.system_network_bytes_ps * 100. / t.system_network_capacity_bytes_ps)
            }
            Column::Network => (
                Some(mc_core::format::to_human_readable_nice(
                    t.system_network_bytes_ps,
                    &mc_core::format::DataType::NetworkBytesPerSecond,
                    &table.units,
                )),
                false,
            ),
            Column::Name
            | Column::Pid
            | Column::GpuEngine
            | Column::GpuDedicatedMemory
            | Column::GpuSharedMemory => (None, false),
        }
    }

    fn cell_text(table: &TableView, column: Column, node: &Node) -> String {
        match column {
            Column::Cpu => {
                let v = node.stats.cpu_usage;
                if v < 0.05 {
                    "0%".into()
                } else {
                    format!("{v:.1}%")
                }
            }
            Column::Pid if node.kind == RowKind::App => String::new(),
            // Per-process network data unavailable (no nethogs or no capture permissions)
            Column::Network if table.hide_network => "—".into(),
            _ => table.cell_text(column, node),
        }
    }

    /// Draws the tab into `content` (between the tab strip and the footer).
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx, table: &mut TableView, content: Rect) {
        self.sync_columns(cx.config);
        let group = cx.config.apps_page_group_by_type;
        table.handle_keys(ui);
        self.sync_rows(table, group);
        let horizontal = listview::horizontal_scrollbar(ui, content, &mut self.list);

        let header_rect = Rect::from_min_max(
            content.min,
            pos2(
                content.right() - m::SCROLLBAR_W,
                content.top() + HeaderKind::Summary.height(),
            ),
        );
        let body = Rect::from_min_max(
            pos2(content.left(), header_rect.bottom() + 1.),
            pos2(
                content.right() - m::SCROLLBAR_W,
                content.bottom() - horizontal,
            ),
        );
        let track = Rect::from_min_max(
            pos2(body.right(), body.top()),
            pos2(content.right(), body.bottom()),
        );
        paint::hline(
            ui.painter(),
            content.left(),
            content.right() - 1.,
            header_rect.bottom(),
            pal::LIST_FRAME,
        );

        // Header
        let headers: Vec<ColumnHeader> = self
            .columns
            .iter()
            .map(|&c| {
                let (summary, alert) = Self::summary(table, c);
                ColumnHeader {
                    title: column_title(c),
                    summary,
                    align: if matches!(c, Column::Name | Column::GpuEngine) {
                        Align::Min
                    } else {
                        Align::Max
                    },
                    sort: if table.options.sort_column == Some(c) {
                        table.options.sort_order
                    } else {
                        SortOrder::None
                    },
                    alert,
                }
            })
            .collect();
        let response = listview::header(
            ui,
            header_rect,
            HeaderKind::Summary,
            &headers,
            &mut self.list,
        );
        if let Some(i) = response.clicked {
            let column = self.columns[i];
            let first = if column == Column::Name {
                SortOrder::Ascending
            } else {
                SortOrder::Descending
            };
            table.sort_by_starting(column, first, cx.config);
        }
        if let Some(pos) = response.context_menu {
            self.header_menu = Some(pos);
        }
        if response.moved.is_some() || response.resized {
            self.save_columns(cx.config, response.moved, response.resized);
        }
        if let Some(i) = self.columns.iter().position(|c| *c == Column::Network) {
            let (x0, sep) = self.list.columns(content.left())[i];
            let cell =
                Rect::from_min_max(pos2(x0, header_rect.top()), pos2(sep, header_rect.bottom()));
            let rate = mc_core::format::to_human_readable_nice(
                table.totals.system_network_bytes_ps,
                &mc_core::format::DataType::NetworkBytesPerSecond,
                &table.units,
            );
            let mut tip = mc_core::i18n::i18n_f("All network interfaces: {}", &[&rate]);
            if table.hide_network {
                tip = format!("{tip}\n\n{}", i18n(NETWORK_HINT));
            } else if table.network_tcp_only {
                tip = format!("{tip}\n\n{}", i18n(NETWORK_TCP_HINT));
            }
            ui.interact(
                cell.intersect(header_rect),
                Id::new("network-header-tip"),
                egui::Sense::hover(),
            )
            .on_hover_text(tip);
        }
        if let Some(i) = self.columns.iter().position(|c| *c == Column::Gpu) {
            let (x0, sep) = self.list.columns(content.left())[i];
            let cell =
                Rect::from_min_max(pos2(x0, header_rect.top()), pos2(sep, header_rect.bottom()))
                    .intersect(header_rect);
            if cell.is_positive() {
                ui.interact(cell, Id::new("gpu-header-tip"), egui::Sense::hover())
                    .on_hover_text(table.gpu_usage.tooltip());
            }
        }

        // Rows
        self.sync_rows(table, group);
        let count = self.rows.len();
        if let Some(index) = table.scroll_to.take()
            && let Some(row) = self.rows.iter().position(|r| {
                table
                    .rows
                    .get(index)
                    .is_some_and(|v| v.node == r.visible.node)
            })
        {
            self.list.scroll_to_row(row, ROW_H, body.height());
        }
        listview::scrollbar(
            ui,
            track,
            &mut self.list,
            ROW_H,
            count as f32 * ROW_H,
            body.height(),
        );
        let (range, rows) = listview::rows(
            ui,
            Id::new("processes-rows"),
            body,
            &mut self.list,
            ROW_H,
            count,
        );
        let painter = ui.painter().with_clip_rect(body);
        let spans = self.list.columns(content.left());
        let selected = table.selected.clone();

        let mut toggle = None;
        for index in range {
            let row = self.rows[index];
            let node = &table.tree.nodes[row.visible.node];
            let top = listview::row_top(body, &self.list, ROW_H, index);
            let bottom = top + ROW_H - 1.;
            let is_selected = selected.as_deref() == Some(node.key.as_str());
            let highlight = if is_selected {
                Some(pal::SELECTION)
            } else if rows.hovered == Some(index) {
                Some(pal::HOVER)
            } else {
                None
            };
            if let Some(color) = highlight {
                paint::fill(&painter, body.left(), top, body.right() - 1., bottom, color);
            }

            if node.kind == RowKind::SectionHeader {
                let label = format!("{} ({})", group_title(node), node.children.len());
                paint::text(
                    &painter,
                    body.left() + 8.,
                    top + 9.,
                    &label,
                    Font::Body,
                    pal::GROUP_TEXT,
                    Align::Min,
                );
                continue;
            }

            for (i, &column) in self.columns.iter().enumerate() {
                let (x0, sep) = spans[i];
                if let Some(percent) = Self::heat_percent(table, column, node) {
                    if highlight.is_none() {
                        paint::fill(&painter, x0, top, sep - 1., bottom, heat(percent));
                    }
                    paint::vline(&painter, sep, top, bottom, pal::HEAT_SEPARATOR);
                    if i > 0 && Self::heat_percent(table, self.columns[i - 1], node).is_none() {
                        paint::vline(&painter, x0 - 1., top, bottom, pal::HEAT_SEPARATOR);
                    }
                }
                if column == Column::Name {
                    let indent = row.level as f32 * INDENT;
                    let cell = painter.with_clip_rect(
                        Rect::from_min_max(pos2(x0, top), pos2(sep - 1., bottom + 1.))
                            .intersect(body),
                    );
                    if row.visible.has_children {
                        let ex = x0 + 10. + indent;
                        expander(&cell, ex, top, row.visible.expanded);
                        if rows.clicked == Some(index)
                            && ui
                                .input(|i| i.pointer.interact_pos())
                                .is_some_and(|p| p.x >= ex - 4. && p.x <= ex + 12.)
                        {
                            toggle = Some(node.key.clone());
                        }
                    }
                    let icon_rect = Rect::from_min_size(
                        pos2(x0 + 29. + indent, top + 6.),
                        egui::vec2(16., 16.),
                    );
                    row_icon(&cell, ui.ctx(), cx, &node.icon, icon_rect);
                    let name_x = x0 + 52. + indent;
                    let name = paint::elide(&painter, &node.name, Font::Body, sep - 6. - name_x);
                    paint::text(
                        &cell,
                        name_x,
                        top + 9.,
                        &name,
                        Font::Body,
                        pal::TEXT,
                        Align::Min,
                    );
                } else {
                    let text = Self::cell_text(table, column, node);
                    let rect =
                        Rect::from_min_max(pos2(x0, top), pos2(sep, bottom + 1.)).intersect(body);
                    let cell = painter.with_clip_rect(rect);
                    let align = if column == Column::GpuEngine {
                        Align::Min
                    } else {
                        Align::Max
                    };
                    let text = paint::elide(&cell, &text, Font::Body, sep - x0 - 10.);
                    paint::text(
                        &cell,
                        if align == Align::Min {
                            x0 + 8.
                        } else {
                            sep - 5.
                        },
                        top + 9.,
                        &text,
                        Font::Body,
                        pal::TEXT,
                        align,
                    );
                    if rect.is_positive() && ui.rect_contains_pointer(rect) {
                        let tooltip = match column {
                            Column::Gpu | Column::GpuEngine => Some(node.gpu.usage.tooltip()),
                            Column::GpuMemory => {
                                Some(node.gpu.memory_tooltip(node.gpu.memory_state))
                            }
                            Column::GpuDedicatedMemory => {
                                Some(node.gpu.memory_tooltip(node.gpu.dedicated_state))
                            }
                            Column::GpuSharedMemory => {
                                Some(node.gpu.memory_tooltip(node.gpu.shared_state))
                            }
                            _ => None,
                        };
                        if let Some(tooltip) = tooltip {
                            ui.interact(
                                rect,
                                Id::new((&node.key, column.id(), "gpu-tip")),
                                egui::Sense::hover(),
                            )
                            .on_hover_text(tooltip);
                        }
                    }
                }
            }
        }

        if let Some(index) = rows.clicked {
            table.selected = Some(table.tree.nodes[self.rows[index].visible.node].key.clone());
        }
        if let Some(index) = rows.double_clicked {
            let node = &table.tree.nodes[self.rows[index].visible.node];
            if node.children.is_empty() {
                cx.send(Command::OpenDialog(Dialog::ProcessDetails(
                    node.key.clone(),
                )));
            } else {
                toggle = Some(node.key.clone());
            }
        }
        if let Some(key) = toggle {
            let expanded = self
                .rows
                .iter()
                .any(|r| table.tree.nodes[r.visible.node].key == key && r.visible.expanded);
            table.set_expanded(&key, !expanded);
        }
        if let Some((index, pos)) = rows.context_menu {
            let key = table.tree.nodes[self.rows[index].visible.node].key.clone();
            table.selected = Some(key.clone());
            self.context = Some((key, pos));
        }

        self.row_menu(ui, cx, table);
        self.columns_menu(ui, cx);
    }

    fn row_menu(&mut self, ui: &mut Ui, cx: &mut Cx, table: &mut TableView) {
        let Some((key, pos)) = self.context.clone() else {
            return;
        };
        let Some(node) = table.node(&key) else {
            self.context = None;
            return;
        };
        let has_children = !node.children.is_empty();
        let expanded = self
            .rows
            .iter()
            .any(|r| table.tree.nodes[r.visible.node].key == key && r.visible.expanded);
        let is_section = node.kind == RowKind::SectionHeader;
        let mut open = true;
        menu::context(ui, Id::new("processes-row-menu"), pos, &mut open, |ui| {
            if has_children
                && menu::item(
                    ui,
                    &i18n(if expanded { "Collapse" } else { "Expand" }),
                    "",
                    true,
                )
            {
                table.set_expanded(&key, !expanded);
            }
            if !is_section {
                if has_children {
                    menu::separator(ui);
                }
                if menu::item(ui, &i18n("End task"), "Del", true) {
                    cx.commands
                        .extend(table.process_action(Request::TerminateProcesses));
                }
                if menu::item(ui, &i18n("Kill"), "", true) {
                    cx.commands
                        .extend(table.process_action(Request::KillProcesses));
                }
                menu::separator(ui);
                if menu::item(ui, &i18n("Properties"), "", true) {
                    cx.send(Command::OpenDialog(Dialog::ProcessDetails(key.clone())));
                }
            }
        });
        if !open {
            self.context = None;
        }
    }

    fn columns_menu(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let Some(pos) = self.header_menu else {
            return;
        };
        let mut open = true;
        let config = &mut *cx.config;
        menu::context(
            ui,
            Id::new("processes-columns-menu"),
            pos,
            &mut open,
            |ui| {
                for column in OPTIONAL {
                    let shown = self.columns.contains(&column);
                    if menu::check(ui, &column_title(column), shown) {
                        let mut visible: Vec<Column> = self
                            .columns
                            .iter()
                            .copied()
                            .filter(|c| *c != Column::Name)
                            .collect();
                        if shown {
                            visible.retain(|c| *c != column);
                        } else {
                            visible.push(column);
                        }
                        config.apps_page_visible_columns = serialize_column_order(&visible);
                    }
                }
            },
        );
        if !open {
            self.header_menu = None;
        }
    }
}

fn group_title(node: &Node) -> String {
    match node.key.as_str() {
        "processes" => i18n(n_("Background processes")),
        _ => node.name.clone(),
    }
}

/// Heat colour of a cell from the share of its column's capacity (own levels above "low").
pub fn heat(percent: f32) -> Color32 {
    if percent <= 0. {
        pal::HEAT_ZERO
    } else if percent < 5. {
        pal::HEAT_LOW
    } else if percent < 20. {
        pal::HEAT_LEVELS[0]
    } else if percent < 50. {
        pal::HEAT_LEVELS[1]
    } else if percent < 80. {
        pal::HEAT_LEVELS[2]
    } else {
        pal::HEAT_LEVELS[3]
    }
}

/// Group expander: `>` 5×9 when collapsed, `v` 9×5 when expanded (`#a6a6a6`).
fn expander(painter: &egui::Painter, x: f32, top: f32, expanded: bool) {
    for r in 0..9 {
        let d = (r.min(8 - r)) as f32;
        if expanded {
            paint::hline(
                painter,
                x - 2. + r as f32,
                x - 2. + r as f32,
                top + 11. + d,
                pal::EXPANDER,
            );
        } else {
            paint::hline(
                painter,
                x + 1. + d,
                x + 1. + d,
                top + 9. + r as f32,
                pal::EXPANDER,
            );
        }
    }
}

/// Paints a row icon: app icons come from the icon theme; other processes get our own
/// generic window glyph.
pub fn row_icon(
    painter: &egui::Painter,
    ctx: &egui::Context,
    cx: &mut Cx,
    icon: &RowIcon,
    rect: Rect,
) {
    let texture = match icon {
        RowIcon::App(id) => cx.icons.app(ctx, id, rect.width()).map(|t| t.id()),
        _ => None,
    };
    match (texture, icon) {
        (Some(id), _) => {
            painter.image(
                id,
                rect,
                Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
                Color32::WHITE,
            );
        }
        (None, RowIcon::None) => {}
        (None, _) => process_glyph(painter, rect),
    }
}

/// Own generic process glyph: a small window.
pub fn process_glyph(painter: &egui::Painter, rect: Rect) {
    let (x, y) = (rect.left(), rect.top());
    let frame = Color32::from_rgb(0x4a, 0x7c, 0xb5);
    paint::fill(painter, x + 1., y + 2., x + 14., y + 4., frame);
    paint::fill(painter, x + 2., y + 5., x + 13., y + 12., pal::WHITE);
    paint::outline(painter, x + 1., y + 2., x + 14., y + 13., frame);
}

/// Row height of the compact ("Fewer details") app list (own design).
pub const COMPACT_ROW_H: f32 = 24.;

impl ProcessesPage {
    /// "Fewer details": a plain list of running apps.
    pub fn compact_ui(&mut self, ui: &mut Ui, cx: &mut Cx, table: &mut TableView, content: Rect) {
        let apps: Vec<usize> = table
            .tree
            .roots
            .iter()
            .map(|&r| &table.tree.nodes[r])
            .filter(|n| n.key == "apps")
            .flat_map(|n| n.children.iter().copied())
            .collect();
        let mut apps = apps;
        apps.sort_by_key(|&i| table.tree.nodes[i].name.to_lowercase());

        let body = Rect::from_min_max(
            pos2(content.left(), content.top() + 1.),
            pos2(content.right() - m::SCROLLBAR_W, content.bottom()),
        );
        let track = Rect::from_min_max(pos2(body.right(), body.top()), content.max);
        let count = apps.len();
        listview::scrollbar(
            ui,
            track,
            &mut self.list,
            COMPACT_ROW_H,
            count as f32 * COMPACT_ROW_H,
            body.height(),
        );
        let (range, rows) = listview::rows(
            ui,
            Id::new("compact-rows"),
            body,
            &mut self.list,
            COMPACT_ROW_H,
            count,
        );
        let painter = ui.painter().with_clip_rect(body);
        for index in range {
            let node = &table.tree.nodes[apps[index]];
            let top = listview::row_top(body, &self.list, COMPACT_ROW_H, index);
            if table.selected.as_deref() == Some(node.key.as_str()) {
                paint::fill(
                    &painter,
                    body.left(),
                    top,
                    body.right() - 1.,
                    top + COMPACT_ROW_H - 1.,
                    pal::SELECTION,
                );
            } else if rows.hovered == Some(index) {
                paint::fill(
                    &painter,
                    body.left(),
                    top,
                    body.right() - 1.,
                    top + COMPACT_ROW_H - 1.,
                    pal::HOVER,
                );
            }
            let icon = Rect::from_min_size(pos2(body.left() + 8., top + 4.), egui::vec2(16., 16.));
            row_icon(&painter, ui.ctx(), cx, &node.icon, icon);
            paint::text(
                &painter,
                body.left() + 32.,
                top + 7.,
                &node.name,
                Font::Body,
                pal::TEXT,
                Align::Min,
            );
        }
        if let Some(index) = rows.clicked.or(rows.context_menu.map(|(i, _)| i)) {
            table.selected = Some(table.tree.nodes[apps[index]].key.clone());
        }
        if let Some((index, pos)) = rows.context_menu {
            self.context = Some((table.tree.nodes[apps[index]].key.clone(), pos));
        }
        self.row_menu(ui, cx, table);
    }
}
