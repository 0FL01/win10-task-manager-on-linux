/* crates/mc-ui/src/win10/flatlist.rs
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

//! Flat Win32 list of the Details and Services tabs: frame line, single-line header, 19 px rows
//! (docs/win10-spec/spec.md, "Details tab", "Services tab").

use egui::{Align, Id, Pos2, Rect, Ui, pos2, vec2};
use mc_core::config::SortOrder;
use mc_core::table::RowIcon;

use mc_core::config::{parse_widths, serialize_widths};

use super::listview::{self, ColumnHeader, HeaderKind, ListState};
use super::paint::{self, Font};
use super::{metrics as m, palette as pal, processes};
use crate::cx::Cx;

pub const ROW_H: f32 = 19.;
/// Frame line 12 px below the tab strip (y 89)
const FRAME_DY: f32 = 12.;

pub struct FlatColumn {
    pub title: String,
    pub width: f32,
    pub align: Align,
}

pub struct FlatRow {
    pub key: String,
    pub icon: Option<RowIcon>,
    pub cells: Vec<String>,
}

#[derive(Default)]
pub struct FlatResponse {
    /// Header clicked: the list's sort changed
    pub sorted: bool,
    /// Sort, column order or widths changed (to be saved)
    pub changed: bool,
    pub double_clicked: Option<String>,
    pub context_menu: Option<(String, Pos2)>,
}

pub struct FlatList {
    pub list: ListState,
    /// Sorted column (logical index, as in the page's column definitions)
    pub sort_column: usize,
    pub sort_order: SortOrder,
    pub selected: Option<String>,
    id: &'static str,
    /// Persistent ids of the logical columns
    ids: &'static [&'static str],
    /// Widths per logical column
    widths: Vec<f32>,
    /// Display order: logical column index per position (the first column stays first)
    order: Vec<usize>,
}

impl FlatList {
    pub fn new(
        id: &'static str,
        ids: &'static [&'static str],
        widths: &[f32],
        sort_column: usize,
    ) -> Self {
        Self {
            list: ListState::new(widths),
            sort_column,
            sort_order: SortOrder::Ascending,
            selected: None,
            id,
            ids,
            widths: widths.to_vec(),
            order: (0..widths.len()).collect(),
        }
    }

    /// Applies saved widths (`id=w;…`), order (`id;…`) and sort.
    pub fn restore(&mut self, widths: &str, order: &str, sort: &str, sort_order: SortOrder) {
        let saved = parse_widths(widths);
        for (i, id) in self.ids.iter().enumerate() {
            if let Some(w) = saved.get(*id) {
                self.widths[i] = w.max(24.);
            }
        }
        let mut new_order: Vec<usize> = order
            .split(';')
            .filter_map(|id| self.ids.iter().position(|x| *x == id))
            .filter(|i| *i != 0)
            .collect();
        new_order.dedup();
        let mut full = vec![0];
        full.extend(new_order.iter().copied());
        for i in 1..self.ids.len() {
            if !full.contains(&i) {
                full.push(i);
            }
        }
        self.order = full;
        if let Some(i) = self.ids.iter().position(|x| *x == sort) {
            self.sort_column = i;
            self.sort_order = if sort_order == SortOrder::Descending {
                SortOrder::Descending
            } else {
                SortOrder::Ascending
            };
        }
    }

    /// Current (widths, order, sort column id) for the settings.
    pub fn saved(&self) -> (String, String, String) {
        let widths = serialize_widths(self.ids.iter().copied().zip(self.widths.iter().copied()));
        let order = self
            .order
            .iter()
            .map(|i| self.ids[*i])
            .collect::<Vec<_>>()
            .join(";");
        (widths, order, self.ids[self.sort_column].to_owned())
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        cx: &mut Cx,
        content: Rect,
        columns: &[FlatColumn],
        rows: &[FlatRow],
    ) -> FlatResponse {
        let mut out = FlatResponse::default();
        let frame_y = content.top() + FRAME_DY;
        paint::hline(
            ui.painter(),
            content.left(),
            content.right() - 1.,
            frame_y,
            pal::LIST_FRAME,
        );
        let header = Rect::from_min_max(
            pos2(content.left(), frame_y + 1.),
            pos2(
                content.right() - m::SCROLLBAR_W,
                frame_y + 1. + HeaderKind::Flat.height(),
            ),
        );
        let body = Rect::from_min_max(
            pos2(content.left(), header.bottom()),
            pos2(content.right() - m::SCROLLBAR_W, content.bottom()),
        );
        let track = Rect::from_min_max(pos2(body.right(), frame_y + 1.), content.max);

        // Header in display order
        let headers: Vec<ColumnHeader> = self
            .order
            .iter()
            .map(|&l| ColumnHeader {
                title: columns[l].title.clone(),
                summary: None,
                align: columns[l].align,
                sort: if l == self.sort_column {
                    self.sort_order
                } else {
                    SortOrder::None
                },
                alert: false,
            })
            .collect();
        if !self.list.is_resizing() {
            self.list.widths = self.order.iter().map(|&l| self.widths[l]).collect();
        }
        let response = listview::header(ui, header, HeaderKind::Flat, &headers, &mut self.list);
        if response.resized {
            for (pos, &l) in self.order.iter().enumerate() {
                self.widths[l] = self.list.widths[pos];
            }
            out.changed = true;
        }
        if let Some((from, to)) = response.moved {
            listview::apply_move(&mut self.order, from, to);
            out.changed = true;
        }
        if let Some(i) = response.clicked.map(|pos| self.order[pos]) {
            out.changed = true;
            if i == self.sort_column {
                self.sort_order = match self.sort_order {
                    SortOrder::Ascending => SortOrder::Descending,
                    _ => SortOrder::Ascending,
                };
            } else {
                self.sort_column = i;
                self.sort_order = SortOrder::Ascending;
            }
            out.sorted = true;
        }

        self.handle_keys(ui, rows, body.height());
        let count = rows.len();
        listview::scrollbar(
            ui,
            track,
            &mut self.list,
            ROW_H,
            count as f32 * ROW_H,
            body.height(),
        );
        let (range, interaction) = listview::rows(
            ui,
            Id::new((self.id, "rows")),
            body,
            &mut self.list,
            ROW_H,
            count,
        );
        let painter = ui.painter().with_clip_rect(body);
        let spans = self.list.columns(content.left());
        for index in range {
            let row = &rows[index];
            let top = listview::row_top(body, &self.list, ROW_H, index);
            if self.selected.as_deref() == Some(row.key.as_str()) {
                paint::fill(
                    &painter,
                    body.left(),
                    top,
                    body.right() - 1.,
                    top + ROW_H - 1.,
                    pal::SELECTION,
                );
            } else if interaction.hovered == Some(index) {
                paint::fill(
                    &painter,
                    body.left(),
                    top,
                    body.right() - 1.,
                    top + ROW_H - 1.,
                    pal::HOVER,
                );
            }
            for (i, &l) in self.order.iter().enumerate() {
                let (column, text) = (&columns[l], &row.cells[l]);
                let (x0, sep) = spans[i];
                let cell = painter.with_clip_rect(
                    Rect::from_min_max(pos2(x0, top), pos2(sep - 1., top + ROW_H)).intersect(body),
                );
                let mut x = x0 + 7.;
                if i == 0
                    && let Some(icon) = &row.icon
                {
                    let rect = Rect::from_min_size(pos2(x0 + 5., top + 2.), vec2(16., 16.));
                    match icon {
                        RowIcon::Service(_) => gear(&cell, rect, pal::SERVICES_ICON),
                        _ => processes::row_icon(&cell, ui.ctx(), cx, icon, rect),
                    }
                    x = x0 + 23.;
                }
                match column.align {
                    Align::Max => paint::text(
                        &cell,
                        sep - 7.,
                        top + 5.,
                        text,
                        Font::Body,
                        pal::TEXT,
                        Align::Max,
                    ),
                    _ => {
                        let text = paint::elide(&painter, text, Font::Body, sep - 4. - x);
                        paint::text(&cell, x, top + 5., &text, Font::Body, pal::TEXT, Align::Min)
                    }
                };
            }
        }
        if let Some(index) = interaction.clicked {
            self.selected = Some(rows[index].key.clone());
        }
        if let Some(index) = interaction.double_clicked {
            out.double_clicked = Some(rows[index].key.clone());
        }
        if let Some((index, pos)) = interaction.context_menu {
            self.selected = Some(rows[index].key.clone());
            out.context_menu = Some((rows[index].key.clone(), pos));
        }
        out
    }

    fn handle_keys(&mut self, ui: &Ui, rows: &[FlatRow], view_h: f32) {
        if rows.is_empty() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if !(up || down) {
            return;
        }
        let current = self
            .selected
            .as_deref()
            .and_then(|k| rows.iter().position(|r| r.key == k));
        let next = match current {
            Some(i) if up => i.saturating_sub(1),
            Some(i) => (i + 1).min(rows.len() - 1),
            None => 0,
        };
        self.selected = Some(rows[next].key.clone());
        self.list.scroll_to_row(next, ROW_H, view_h);
    }
}

/// Own service glyph: a small gear.
pub fn gear(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let c = rect.center();
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::TAU / 8.;
        let (s, co) = a.sin_cos();
        painter.line_segment(
            [c + vec2(co, s) * 4.5, c + vec2(co, s) * 7.],
            egui::Stroke::new(2.2, color),
        );
    }
    painter.circle_stroke(c, 4.2, egui::Stroke::new(2., color));
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: [&str; 4] = ["name", "pid", "cpu", "memory"];

    #[test]
    fn restore_and_save_round_trip() {
        let mut list = FlatList::new("t", &IDS, &[100., 50., 40., 80.], 0);
        list.restore(
            "pid=70;memory=120;bogus=5",
            "memory;name;cpu",
            "memory",
            SortOrder::Descending,
        );
        let (widths, order, sort) = list.saved();
        assert_eq!(widths, "name=100;pid=70;cpu=40;memory=120");
        // Name stays first; unknown/missing columns are appended
        assert_eq!(order, "name;memory;cpu;pid");
        assert_eq!(sort, "memory");
        assert_eq!(list.sort_order, SortOrder::Descending);

        let mut again = FlatList::new("t", &IDS, &[100., 50., 40., 80.], 0);
        again.restore(&widths, &order, &sort, SortOrder::Descending);
        assert_eq!(again.saved(), (widths, order, sort));
    }
}
