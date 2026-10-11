/* crates/mc-ui/src/win10/performance.rs
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

//! Performance tab: device list with mini graphs and the Win10 device page
//! (docs/win10-spec/spec.md, "Performance tab").

use egui::{Align, Id, Pos2, Rect, Sense, Ui, pos2, vec2};
use mc_core::i18n::i18n;
use mc_core::sidebar::DeviceType;
use mc_graph::{Graph, GraphStyle};

use super::paint::{self, Font};
use super::{listview, menu, metrics as m, palette as pal};
use crate::cx::{Command, Cx};
use crate::perf::{GraphPanel, GraphTile, PageView, PerformancePage};

/// Reference device list width (separator at content left + 211); the separator can be dragged
/// wider (own design), the page keeps at least `PAGE_MIN_W`
const LIST_W: f32 = 211.;
const PAGE_MIN_W: f32 = 547.;
const LIST_TOP: f32 = 12.;
/// Page geometry relative to the content area
const TITLE_X: f32 = 237.;
const GRAPH_X: f32 = 240.;
const RIGHT_INSET: f32 = 25.;
const TITLE_CAP: f32 = 26.;
const MODEL_CAP: f32 = 34.;
const GRAPH_TOP: f32 = 74.;
const GRAPH_GAP: f32 = 4.;
/// Gap between a graph and the next caption's cap top
const PANEL_GAP: f32 = 24.;
const STAT_PITCH: f32 = 51.;
const STAT_VALUE_DY: f32 = 21.;
const STAT_COLUMN_GAP: f32 = 21.;
const INFO_X: f32 = 441.;
const INFO_VALUE_DX: f32 = 109.;
const INFO_PITCH: f32 = 18.;

pub fn device_colors(kind: DeviceType) -> pal::DeviceColors {
    match kind {
        DeviceType::Cpu | DeviceType::Unspecified => pal::CPU,
        DeviceType::Memory => pal::MEMORY,
        DeviceType::Disk => pal::DISK,
        DeviceType::Network { .. } => pal::NETWORK,
        DeviceType::Gpu => pal::GPU,
        DeviceType::Fan => pal::FAN,
        DeviceType::Battery => pal::BATTERY,
    }
}

fn style_graph(graph: &mut Graph, colors: pal::DeviceColors) {
    graph.set_base_color(colors.line);
    graph.set_style(GraphStyle::Win10 { fill: colors.fill });
}

#[derive(Default)]
pub struct PerformanceView {
    offset: f32,
    menu: Option<Pos2>,
    menu_slot: Option<usize>,
    gpu_scroll: listview::ListState,
    scroll_device: String,
    /// List width while the separator is dragged (saved to the settings on release)
    dragging: Option<f32>,
}

impl PerformanceView {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx, perf: &mut PerformancePage, content: Rect) {
        if !perf.loaded {
            return;
        }
        let max_w = (content.width() - PAGE_MIN_W).max(LIST_W);
        let list_w = self
            .dragging
            .unwrap_or(cx.config.performance_sidebar_width as f32)
            .clamp(LIST_W, max_w)
            .round();
        let list = Rect::from_min_max(content.min, pos2(content.left() + list_w, content.bottom()));
        if !perf.summary_mode {
            self.separator(ui, cx, list, list_w, max_w);
            self.list_ui(ui, cx, perf, list);
        }

        let Some(index) = perf.device_index(&perf.selected) else {
            return;
        };
        let page = if perf.summary_mode {
            content
        } else {
            Rect::from_min_max(pos2(list.right() + 1., content.top()), content.max)
        };
        let response = ui.interact(
            page,
            Id::new("perf-page"),
            if perf.summary_mode {
                Sense::click_and_drag()
            } else {
                Sense::click()
            },
        );
        if perf.summary_mode && response.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if response.secondary_clicked() {
            self.menu = response.interact_pointer_pos();
            self.menu_slot = None;
        }
        let mut toggle_summary = response.double_clicked();
        let kind = perf.devices[index].device_type();
        let colors = device_colors(kind);
        let tiles = perf.devices[index].win10_graph_tiles(cx.config);
        let view = perf.devices[index].win10_view(cx.config);
        // The page geometry is relative to the reference list width
        let shifted = Rect::from_min_max(
            pos2(content.left() + list_w - LIST_W, content.top()),
            content.max,
        );
        let graph_response = if perf.summary_mode {
            summary_ui(ui, page, view, &tiles, colors)
        } else if kind == DeviceType::Gpu {
            if self.scroll_device != perf.selected {
                self.gpu_scroll.offset = 0.;
                self.scroll_device.clone_from(&perf.selected);
            }
            gpu_page_ui(ui, page, view, &tiles, colors, &mut self.gpu_scroll)
        } else {
            page_ui(ui, shifted, view, colors);
            GraphResponse::default()
        };
        toggle_summary |= graph_response.double_clicked;
        if toggle_summary {
            cx.send(Command::ToggleSummaryMode);
        }
        if let Some((pos, slot)) = graph_response.menu {
            self.menu = Some(pos);
            self.menu_slot = slot;
        }

        if let Some(pos) = self.menu {
            let mut open = true;
            let device = &mut perf.devices[index];
            menu::context(ui, Id::new("perf-menu"), pos, &mut open, |ui| {
                if let Some(slot) = self.menu_slot {
                    device.win10_graph_menu(ui, cx, slot);
                    menu::separator(ui);
                }
                device.win10_menu(ui, cx);
                menu::separator(ui);
                if menu::check(ui, &i18n("Graph summary view"), perf.summary_mode) {
                    cx.send(Command::ToggleSummaryMode);
                }
                menu::separator(ui);
                if menu::item(
                    ui,
                    &mc_core::i18n::strip_mnemonic(&i18n("_Copy")),
                    "Ctrl+C",
                    true,
                ) {
                    cx.send(Command::CopyText(device.data_summary()));
                }
            });
            if !open {
                self.menu = None;
                self.menu_slot = None;
            }
        }
    }

    /// Device list separator; dragging it changes the list width.
    fn separator(&mut self, ui: &mut Ui, cx: &mut Cx, list: Rect, list_w: f32, max_w: f32) {
        let x = list.right();
        let grip = Rect::from_min_max(pos2(x - 3., list.top()), pos2(x + 4., list.bottom()));
        let response = ui.interact(grip, Id::new("perf-list-separator"), Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if response.drag_started() {
            self.dragging = Some(list_w);
        }
        if response.dragged()
            && let Some(width) = self.dragging.as_mut()
        {
            *width = (*width + response.drag_delta().x).clamp(LIST_W, max_w);
        }
        if response.drag_stopped()
            && let Some(width) = self.dragging.take()
        {
            cx.config.performance_sidebar_width = width.round() as i32;
        }
        let color = if response.hovered() || response.dragged() {
            pal::BUTTON_HOVER_BORDER
        } else {
            pal::TAB_BORDER
        };
        paint::vline(ui.painter(), x, list.top(), list.bottom() - 1., color);
    }

    fn list_ui(&mut self, ui: &mut Ui, cx: &mut Cx, perf: &mut PerformancePage, list: Rect) {
        let shown = perf.shown_names(cx.config);
        perf.keep_selection_shown(&shown, cx.config);
        let heights: Vec<f32> = shown
            .iter()
            .map(|name| {
                perf.device_index(name).map_or(60., |i| {
                    let s = perf.devices[i].summary();
                    let lines = 1 + [&s.info1, &s.info2]
                        .iter()
                        .filter(|t| !t.is_empty())
                        .count();
                    if lines >= 3 { 66. } else { 60. }
                })
            })
            .collect();
        let total: f32 = heights.iter().sum::<f32>() + LIST_TOP;
        if ui.rect_contains_pointer(list) {
            let delta: f32 = ui.input(|i| {
                i.raw
                    .events
                    .iter()
                    .map(|e| match e {
                        egui::Event::MouseWheel { unit, delta, .. } => match unit {
                            egui::MouseWheelUnit::Point => delta.y,
                            _ => delta.y * 60.,
                        },
                        _ => 0.,
                    })
                    .sum()
            });
            self.offset -= delta;
        }
        self.offset = self
            .offset
            .clamp(0., (total - list.height()).max(0.))
            .round();

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
        child.set_clip_rect(list.intersect(ui.clip_rect()));
        let painter = child.painter().clone();
        let mut top = list.top() + LIST_TOP - self.offset;
        let mut clicked = None;
        for (name, h) in shown.iter().zip(heights) {
            let item = Rect::from_min_max(pos2(list.left(), top), pos2(list.right(), top + h));
            top += h;
            if item.bottom() < list.top() || item.top() > list.bottom() {
                continue;
            }
            let Some(index) = perf.device_index(name) else {
                continue;
            };
            let response = child.interact(
                item.intersect(list),
                Id::new(("perf-device", name)),
                Sense::click(),
            );
            let selected = *name == perf.selected;
            let x0 = list.left() + 4.;
            let x1 = list.right() - 4.;
            if selected {
                paint::fill(
                    &painter,
                    x0,
                    item.top(),
                    x1,
                    item.bottom() - 1.,
                    pal::DEVICE_SELECTED,
                );
            } else if response.hovered() {
                paint::fill(&painter, x0, item.top(), x1, item.bottom() - 1., pal::HOVER);
            }
            if response.clicked() {
                clicked = Some(name.clone());
            }

            let device = &mut perf.devices[index];
            let colors = device_colors(device.device_type());
            let summary = device.summary_mut();
            style_graph(&mut summary.graph, colors);
            let graph = Rect::from_min_size(
                pos2(list.left() + 14., item.top() + 10.),
                vec2(60., h - 20.),
            );
            summary.graph.paint(&child, graph);

            let text = painter.with_clip_rect(Rect::from_min_max(
                pos2(list.left() + 85., item.top()),
                pos2(x1, item.bottom()),
            ));
            let room = x1 - (list.left() + 85.);
            let heading = paint::elide(&painter, &summary.heading, Font::DeviceName, room);
            paint::text(
                &text,
                list.left() + 85.,
                item.top() + 11.,
                &heading,
                Font::DeviceName,
                pal::TEXT,
                Align::Min,
            );
            let mut cap = item.top() + 29.;
            for info in [&summary.info1, &summary.info2] {
                if !info.is_empty() {
                    let info = paint::elide(&painter, info, Font::Body, room);
                    paint::text(
                        &text,
                        list.left() + 85.,
                        cap,
                        &info,
                        Font::Body,
                        pal::TEXT,
                        Align::Min,
                    );
                    cap += 15.;
                }
            }
        }
        if let Some(name) = clicked {
            perf.select(&name, cx.config);
        }
    }
}

#[derive(Default)]
struct GraphResponse {
    menu: Option<(Pos2, Option<usize>)>,
    double_clicked: bool,
}

/// GPU-only own adaptation: minimum usable graph heights and a vertical flow ensure every
/// statistic and actual driver field stays reachable even in a short window.
fn gpu_page_ui(
    ui: &mut Ui,
    page: Rect,
    mut view: PageView<'_>,
    tiles: &[GraphTile],
    colors: pal::DeviceColors,
    scroll: &mut listview::ListState,
) -> GraphResponse {
    let gx0 = page.left() + GRAPH_X - LIST_W - 1.;
    let gx1 = page.right() - RIGHT_INSET;
    let room = gx1 - gx0 + 1.;
    let painter = ui.painter().clone();
    let mut stat_rows = vec![];
    for row in &view.stats {
        let mut line = vec![];
        let mut width = 0.;
        for entry in row {
            let entry_w = paint::text_width(&painter, &entry.0, Font::Body).max(paint::text_width(
                &painter,
                &entry.1,
                Font::StatValue,
            ));
            if !line.is_empty() && width + entry_w > room {
                stat_rows.push(line);
                line = vec![];
                width = 0.;
            }
            line.push((entry, entry_w));
            width += entry_w + STAT_COLUMN_GAP;
        }
        if !line.is_empty() {
            stat_rows.push(line);
        }
    }
    let heights: Vec<f32> = view
        .panels
        .iter()
        .enumerate()
        .map(|(index, panel)| {
            if index == 0 {
                let rows = panel.graphs.len().div_ceil(panel.columns.max(1)).max(1);
                (m::GPU_ENGINE_MIN_H + m::GPU_TILE_CAPTION_H + GRAPH_GAP) * rows as f32 - GRAPH_GAP
            } else if index <= 2 {
                m::GPU_MEMORY_MIN_H
            } else {
                m::GPU_AUX_MIN_H
            }
        })
        .collect();
    let graphs_h = heights.iter().sum::<f32>()
        + heights.len().saturating_sub(1) as f32 * (m::GPU_PANEL_GAP + 13.);
    let stats_y = GRAPH_TOP + graphs_h + m::GPU_PANEL_GAP;
    let info_y = stats_y + stat_rows.len() as f32 * STAT_PITCH + m::GPU_INFO_GAP;
    let total_h = info_y + view.info.len() as f32 * INFO_PITCH + m::GPU_INFO_GAP;
    if ui.rect_contains_pointer(page) {
        scroll.offset -= wheel_delta(ui);
    }
    scroll.offset = scroll
        .offset
        .clamp(0., (total_h - page.height()).max(0.))
        .round();
    let track = Rect::from_min_max(pos2(page.right() - m::SCROLLBAR_W, page.top()), page.max);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("gpu-performance-page")
            .max_rect(page),
    );
    child.set_clip_rect(page.intersect(ui.clip_rect()));
    listview::scrollbar(
        &mut child,
        track,
        scroll,
        INFO_PITCH,
        total_h,
        page.height(),
    );
    let top = page.top() - scroll.offset;
    let painter = child.painter().clone();
    let title_x = page.left() + TITLE_X - LIST_W - 1.;
    paint::text(
        &painter,
        title_x,
        top + TITLE_CAP,
        &view.title,
        Font::Title,
        pal::TEXT,
        Align::Min,
    );
    let title_w = paint::text_width(&painter, &view.title, Font::Title);
    let model = paint::elide(
        &painter,
        &view.model,
        Font::Model,
        gx1 - title_x - title_w - 16.,
    );
    paint::text(
        &painter,
        gx1 + 1.,
        top + MODEL_CAP,
        &model,
        Font::Model,
        pal::TEXT,
        Align::Max,
    );
    let response = graph_panels_ui(
        &mut child,
        gx0,
        gx1,
        top + GRAPH_TOP,
        &mut view.panels,
        tiles,
        &heights,
        colors,
    );
    for (row, entries) in stat_rows.iter().enumerate() {
        let cap = top + stats_y + 5. + row as f32 * STAT_PITCH;
        let mut x = gx0;
        for ((label, value), width) in entries {
            paint::text(&painter, x, cap, label, Font::Body, pal::LABEL, Align::Min);
            let value = paint::elide(&painter, value, Font::StatValue, room);
            paint::text(
                &painter,
                x,
                cap + STAT_VALUE_DY,
                &value,
                Font::StatValue,
                pal::TEXT,
                Align::Min,
            );
            x += width + STAT_COLUMN_GAP;
        }
    }
    let label_w = view
        .info
        .iter()
        .map(|(label, _)| paint::text_width(&painter, label, Font::Body))
        .fold(0., f32::max)
        .min(room * 0.6);
    let value_x = gx0 + INFO_VALUE_DX.max(label_w + 12.);
    for (row, (label, value)) in view.info.iter().enumerate() {
        let cap = top + info_y + row as f32 * INFO_PITCH;
        let label_short = paint::elide(&painter, label, Font::Body, label_w);
        let value_short = paint::elide(&painter, value, Font::Body, gx1 + 1. - value_x);
        paint::text(
            &painter,
            gx0,
            cap,
            &label_short,
            Font::Body,
            pal::LABEL,
            Align::Min,
        );
        paint::text(
            &painter,
            value_x,
            cap,
            &value_short,
            Font::Body,
            pal::TEXT,
            Align::Min,
        );
        child
            .interact(
                Rect::from_min_max(pos2(gx0, cap - 3.), pos2(gx1 + 1., cap + 12.)),
                Id::new(("gpu-info", row)),
                Sense::hover(),
            )
            .on_hover_text(format!("{label} {value}"));
    }
    response
}

fn wheel_delta(ui: &Ui) -> f32 {
    ui.input(|input| {
        input
            .raw
            .events
            .iter()
            .map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => match unit {
                    egui::MouseWheelUnit::Point => delta.y,
                    _ => delta.y * 60.,
                },
                _ => 0.,
            })
            .sum()
    })
}

/// Graph-only summary; the caller also omits the sidebar, and App omits its shell chrome.
fn summary_ui(
    ui: &mut Ui,
    content: Rect,
    mut view: PageView<'_>,
    tiles: &[GraphTile],
    colors: pal::DeviceColors,
) -> GraphResponse {
    let area = content.shrink(m::GPU_COMPACT_INSET);
    let count = view.panels.len().max(1);
    let rows: Vec<_> = view
        .panels
        .iter()
        .map(|panel| panel.graphs.len().div_ceil(panel.columns.max(1)).max(1))
        .collect();
    let caption_h = if tiles.is_empty() {
        0.
    } else {
        rows.first().copied().unwrap_or(1) as f32 * m::GPU_TILE_CAPTION_H
    };
    let available =
        area.height() - 13. - count.saturating_sub(1) as f32 * (m::GPU_PANEL_GAP + 13.) - caption_h;
    let row_h = (available / rows.iter().sum::<usize>().max(1) as f32)
        .floor()
        .max(4.);
    let heights: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(index, rows)| row_h * *rows as f32 + if index == 0 { caption_h } else { 0. })
        .collect();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("perf-summary-graphs")
            .max_rect(content),
    );
    child.set_clip_rect(content.intersect(ui.clip_rect()));
    graph_panels_ui(
        &mut child,
        area.left(),
        area.right() - 1.,
        area.top() + 13.,
        &mut view.panels,
        tiles,
        &heights,
        colors,
    )
}

#[allow(clippy::too_many_arguments)]
fn graph_panels_ui(
    ui: &mut Ui,
    gx0: f32,
    gx1: f32,
    mut graph_top: f32,
    panels: &mut [GraphPanel<'_>],
    tiles: &[GraphTile],
    heights: &[f32],
    colors: pal::DeviceColors,
) -> GraphResponse {
    let mut out = GraphResponse::default();
    let painter = ui.painter().clone();
    for (index, (panel, height)) in panels.iter_mut().zip(heights).enumerate() {
        paint::text(
            &painter,
            gx0,
            graph_top - 13.,
            &panel.caption,
            Font::Caption,
            pal::LABEL,
            Align::Min,
        );
        paint::text(
            &painter,
            gx1 + 1.,
            graph_top - 13.,
            &panel.right,
            Font::Caption,
            pal::LABEL,
            Align::Max,
        );
        let columns = panel.columns.clamp(1, panel.graphs.len().max(1));
        let rows = panel.graphs.len().div_ceil(columns).max(1);
        let edge = |position: usize, count: usize, total: f32| {
            (position as f32 * (total + GRAPH_GAP) / count as f32).floor()
        };
        for (slot, graph) in panel.graphs.iter_mut().enumerate() {
            let x0 = gx0 + edge(slot % columns, columns, gx1 - gx0 + 1.);
            let x1 = gx0 + edge(slot % columns + 1, columns, gx1 - gx0 + 1.) - GRAPH_GAP;
            let band_top = graph_top + edge(slot / columns, rows, *height);
            let y1 = graph_top + edge(slot / columns + 1, rows, *height) - GRAPH_GAP;
            let tile = if index == 0 { tiles.get(slot) } else { None };
            let y0 = band_top
                + if tile.is_some() {
                    m::GPU_TILE_CAPTION_H
                } else {
                    0.
                };
            let rect = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
            if !ui.is_rect_visible(Rect::from_min_max(pos2(x0, band_top), rect.max)) {
                continue;
            }
            style_graph(graph, colors);
            graph.paint(ui, rect);
            let response = ui.interact(
                Rect::from_min_max(pos2(x0, band_top), rect.max),
                ui.id().with(("perf-graph", index, slot)),
                Sense::click(),
            );
            out.double_clicked |= response.double_clicked();
            let caption_clicked = tile.is_some()
                && response.clicked()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|pos| pos.y < y0);
            if (response.secondary_clicked() || caption_clicked)
                && let Some(pos) = response.interact_pointer_pos()
            {
                out.menu = Some((pos, tile.map(|tile| tile.slot)));
            }
            if let Some(tile) = tile {
                let right_w = paint::text_width(&painter, &tile.right, Font::Caption);
                let caption = paint::elide(
                    &painter,
                    &tile.caption,
                    Font::Caption,
                    x1 - x0 - right_w - 12. - m::GPU_DROPDOWN_W - m::GPU_DROPDOWN_GAP,
                );
                paint::text(
                    &painter,
                    x0,
                    band_top + 4.,
                    &caption,
                    Font::Caption,
                    pal::LABEL,
                    Align::Min,
                );
                let arrow_x =
                    x0 + paint::text_width(&painter, &caption, Font::Caption) + m::GPU_DROPDOWN_GAP;
                for row in 0..m::GPU_DROPDOWN_H {
                    let row = row as f32;
                    paint::hline(
                        &painter,
                        arrow_x + row,
                        arrow_x + m::GPU_DROPDOWN_W - 1. - row,
                        band_top + m::GPU_DROPDOWN_DY + row,
                        pal::LABEL,
                    );
                }
                paint::text(
                    &painter,
                    x1 + 1.,
                    band_top + 4.,
                    &tile.right,
                    Font::Caption,
                    pal::LABEL,
                    Align::Max,
                );
                response.on_hover_text(&tile.tooltip);
            }
        }
        graph_top += height + m::GPU_PANEL_GAP + 13.;
    }
    out
}

/// Draws a device page into `content` (device list excluded).
fn page_ui(ui: &mut Ui, content: Rect, mut view: PageView<'_>, colors: pal::DeviceColors) {
    let painter = ui.painter().clone();
    let left = content.left();
    let top = content.top();
    let gx0 = left + GRAPH_X;
    let gx1 = content.right() - RIGHT_INSET;

    paint::text(
        &painter,
        left + TITLE_X,
        top + TITLE_CAP,
        &view.title,
        Font::Title,
        pal::TEXT,
        Align::Min,
    );
    let title_w = paint::text_width(&painter, &view.title, Font::Title);
    let model_room = gx1 + 1. - (left + TITLE_X + title_w + 16.);
    let model = paint::elide(&painter, &view.model, Font::Model, model_room);
    paint::text(
        &painter,
        gx1 + 1.,
        top + MODEL_CAP,
        &model,
        Font::Model,
        pal::TEXT,
        Align::Max,
    );

    // Statistics and key/value rows at the bottom
    let stat_rows = view.stats.len().max(1) as f32;
    let stats_h = 5. + STAT_PITCH * (stat_rows - 1.) + STAT_VALUE_DY + 14.;
    let stats_top = (content.bottom() - PANEL_GAP - stats_h).round();
    let mut stats_right = gx0;
    for (r, row) in view.stats.iter().enumerate() {
        let cap = stats_top + 5. + STAT_PITCH * r as f32;
        let mut x = gx0;
        for (label, value) in row {
            paint::text(&painter, x, cap, label, Font::Body, pal::LABEL, Align::Min);
            paint::text(
                &painter,
                x,
                cap + STAT_VALUE_DY,
                value,
                Font::StatValue,
                pal::TEXT,
                Align::Min,
            );
            let w = paint::text_width(&painter, label, Font::Body).max(paint::text_width(
                &painter,
                value,
                Font::StatValue,
            ));
            stats_right = stats_right.max(x + w);
            x += (w + STAT_COLUMN_GAP).round();
        }
    }
    let mut label_x = (left + INFO_X).max((stats_right + 24.).round());
    let bottom_limit = content.bottom() - 12.;
    let mut i = 0;
    // A further column only when a label and a short value fit; otherwise the rest is hidden
    while i < view.info.len() && label_x + 160. <= gx1 {
        let fit = (((bottom_limit - (stats_top + 5.)) / INFO_PITCH).floor() as usize).max(1);
        let column = &view.info[i..(i + fit).min(view.info.len())];
        let label_w = column
            .iter()
            .map(|(l, _)| paint::text_width(&painter, l, Font::Body))
            .fold(0., f32::max);
        let value_w_max = column
            .iter()
            .map(|(_, v)| paint::text_width(&painter, v, Font::Body))
            .fold(0., f32::max);
        // Reference gap of 109 px when it fits, otherwise right after the longest label
        let tight = (label_x + label_w + 12.).round();
        let wide = label_x + INFO_VALUE_DX.max((label_w + 12.).round());
        let value_x = if wide + value_w_max <= gx1 + 1. {
            wide
        } else {
            tight
        };
        let mut value_w: f32 = 0.;
        for (n, (label, value)) in column.iter().enumerate() {
            let cap = stats_top + 5. + INFO_PITCH * n as f32;
            paint::text(
                &painter,
                label_x,
                cap,
                label,
                Font::Body,
                pal::LABEL,
                Align::Min,
            );
            let clip = painter.with_clip_rect(Rect::from_min_max(
                pos2(value_x, top),
                pos2(gx1 + 1., content.bottom()),
            ));
            let value = paint::elide(&painter, value, Font::Body, gx1 + 1. - value_x);
            paint::text(
                &clip,
                value_x,
                cap,
                &value,
                Font::Body,
                pal::TEXT,
                Align::Min,
            );
            value_w = value_w.max(paint::text_width(&painter, &value, Font::Body));
        }
        i += column.len();
        label_x = (value_x + value_w + 24.).round();
    }

    // Graph panels share the space between the caption and the statistics
    let panels = view.panels.len().max(1) as f32;
    let first_top = top + GRAPH_TOP;
    let graphs_bottom = stats_top - PANEL_GAP - 1.;
    let available = graphs_bottom - first_top + 1. - (panels - 1.) * (PANEL_GAP + 13.);
    let panel_h = (available / panels).floor().max(20.);
    let mut graph_top = first_top;
    for panel in view.panels.iter_mut() {
        let cap = graph_top - 13.;
        paint::text(
            &painter,
            gx0,
            cap,
            &panel.caption,
            Font::Caption,
            pal::LABEL,
            Align::Min,
        );
        paint::text(
            &painter,
            gx1 + 1.,
            cap,
            &panel.right,
            Font::Caption,
            pal::LABEL,
            Align::Max,
        );
        let n = panel.graphs.len().max(1);
        let cols = panel.columns.clamp(1, n);
        let rows = n.div_ceil(cols);
        let total_w = gx1 - gx0 + 1.;
        let edge = |i: usize, count: usize, total: f32| {
            ((i as f32) * (total + GRAPH_GAP) / count as f32).floor()
        };
        for (k, graph) in panel.graphs.iter_mut().enumerate() {
            let (c, r) = (k % cols, k / cols);
            let x0 = gx0 + edge(c, cols, total_w);
            let x1 = gx0 + edge(c + 1, cols, total_w) - GRAPH_GAP;
            let y0 = graph_top + edge(r, rows, panel_h);
            let y1 = graph_top + edge(r + 1, rows, panel_h) - GRAPH_GAP;
            style_graph(graph, colors);
            graph.paint(ui, Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)));
        }
        graph_top += panel_h + PANEL_GAP + 13.;
    }
}
