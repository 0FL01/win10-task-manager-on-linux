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
use mc_graph::Graph;

use super::paint::{self, Font};
use super::{listview, menu, metrics as m, palette as pal};
use crate::cx::{Command, Cx};
use crate::perf::batteries::{BatteriesView, BatteryCard};
use crate::perf::{GraphPanel, GraphTile, PageView, PerformancePage};
use mc_magpie::magpie_types::battery::BatteryType;

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

/// The widest value of the same kind as `value` (own design): numbers with a unit or a fraction
/// become four digits (formatting keeps them under 1024), other integers at least three, and a
/// unit prefix becomes "M", so a statistic keeps its width as readings change.
fn widest_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 4);
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let after_colon = out.ends_with(':');
            let (mut digits, mut fraction) = (1, false);
            while let Some(&next) = chars.peek() {
                match next {
                    '0'..='9' => digits += 1,
                    '.' | ',' => fraction = true,
                    _ => break,
                }
                chars.next();
            }
            // Clock-like fields ("01:02:03:04") keep their digits; a number with a unit
            // ("138 KiB/s", "1.38 MiB/s") or a fraction takes four, plain counts three
            let mut ahead = chars.clone();
            let unit = ahead.next() == Some(' ') && ahead.next().is_some_and(char::is_alphabetic);
            let digits = match chars.peek() {
                Some(':') => digits,
                _ if after_colon => digits,
                _ if fraction || unit => digits.max(4),
                _ => digits.max(3),
            };
            out.extend(std::iter::repeat_n('0', digits));
        } else if out.ends_with(' ') && matches!(c, 'K' | 'k' | 'M' | 'G' | 'T' | 'P') {
            out.push('M');
        } else if out.ends_with(' ') && matches!(c, 'B' | 'b' | 'H' | 'W' | 'V') {
            // No prefix yet, a larger value will get one
            out.push('M');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// Width a statistic reserves: its label, its value and the widest value of the same kind.
fn stat_width(painter: &egui::Painter, label: &str, value: &str) -> f32 {
    paint::text_width(painter, label, Font::Body)
        .max(paint::text_width(painter, value, Font::StatValue))
        .max(paint::text_width(
            painter,
            &widest_value(value),
            Font::StatValue,
        ))
}

/// Aligns statistic rows as a table (own design): column k of every row takes the widest
/// entry of column k, unless that row would no longer fit in `room`.
fn align_columns(widths: &mut [Vec<f32>], room: f32) {
    let columns = widths.iter().map(Vec::len).max().unwrap_or(0);
    let max: Vec<f32> = (0..columns)
        .map(|k| {
            widths
                .iter()
                .filter_map(|row| row.get(k))
                .fold(0., |a: f32, &b| a.max(b))
        })
        .collect();
    for row in widths {
        let aligned = &max[..row.len()];
        let span =
            aligned.iter().sum::<f32>() + row.len().saturating_sub(1) as f32 * STAT_COLUMN_GAP;
        if span <= room {
            row.copy_from_slice(aligned);
        }
    }
}

pub fn device_colors(kind: DeviceType) -> pal::DeviceColors {
    match kind {
        DeviceType::Cpu | DeviceType::System | DeviceType::Unspecified => pal::CPU,
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
    graph.set_fill(colors.fill);
}

#[derive(Default)]
pub struct PerformanceView {
    offset: f32,
    menu: Option<Pos2>,
    menu_slot: Option<usize>,
    gpu_scroll: listview::ListState,
    scroll_device: String,
    memory_scroll: listview::ListState,
    memory_active: bool,
    memory_size: egui::Vec2,
    battery_scroll: listview::ListState,
    battery_selected: Option<String>,
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

        let Some(index) = perf
            .device_index(&perf.selected)
            .or_else(|| perf.device_index("cpu"))
        else {
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
        let memory_active = kind == DeviceType::Memory && !perf.summary_mode;
        if memory_active && (!self.memory_active || self.memory_size != page.size()) {
            self.memory_scroll = listview::ListState::default();
        }
        self.memory_size = page.size();
        self.memory_active = memory_active;
        let colors = device_colors(kind);
        let tiles = perf.devices[index].win10_graph_tiles(cx.config);
        // The page geometry is relative to the reference list width
        let shifted = Rect::from_min_max(
            pos2(content.left() + list_w - LIST_W, content.top()),
            content.max,
        );
        let graph_response = if kind == DeviceType::Battery && !perf.summary_mode {
            perf.devices[index]
                .win10_batteries_view(cx.config, self.battery_selected.as_deref())
                .map(|view| {
                    battery_page_ui(
                        ui,
                        page,
                        view,
                        &mut self.battery_scroll,
                        &mut self.battery_selected,
                    )
                })
                .unwrap_or_default()
        } else {
            let view = perf.devices[index].win10_view(cx.config);
            if kind == DeviceType::System {
                if self.scroll_device != perf.selected {
                    self.gpu_scroll.offset = 0.;
                    self.scroll_device.clone_from(&perf.selected);
                }
                system_page_ui(ui, page, view, &mut self.gpu_scroll);
                GraphResponse::default()
            } else if perf.summary_mode {
                summary_ui(ui, page, view, &tiles, colors, &perf.selected)
            } else if kind == DeviceType::Gpu {
                if self.scroll_device != perf.selected {
                    self.gpu_scroll.offset = 0.;
                    self.scroll_device.clone_from(&perf.selected);
                }
                gpu_page_ui(ui, page, view, &tiles, colors, &mut self.gpu_scroll)
            } else {
                if memory_active {
                    let count = view.panels.len().max(1) as f32;
                    let stack_h = count * m::MEMORY_GRAPH_MIN_H
                        + count * (PANEL_GAP + 13.)
                        + m::MEMORY_COMPOSITION_H
                        + swap_block_height(&view);
                    let base_h = shifted.height().max(
                        GRAPH_TOP
                            + stack_h
                            + PANEL_GAP
                            + stats_height(view.stats.len())
                            + PANEL_GAP,
                    );
                    let total_h = base_h;
                    if ui.rect_contains_pointer(page) {
                        self.memory_scroll.offset -= wheel_delta(ui);
                    }
                    self.memory_scroll.offset = self
                        .memory_scroll
                        .offset
                        .clamp(0., (total_h - page.height()).max(0.))
                        .round();
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(("memory-performance-page", &perf.selected))
                            .max_rect(page),
                    );
                    child.set_clip_rect(page.intersect(ui.clip_rect()));
                    let track = Rect::from_min_max(
                        pos2(page.right() - m::SCROLLBAR_W, page.top()),
                        page.max,
                    );
                    listview::scrollbar(
                        &mut child,
                        track,
                        &mut self.memory_scroll,
                        INFO_PITCH,
                        total_h,
                        page.height(),
                    );
                    let top = shifted.top() - self.memory_scroll.offset;
                    let virtual_page = Rect::from_min_max(
                        pos2(shifted.left(), top),
                        pos2(shifted.right(), top + base_h),
                    );
                    let module_height =
                        stats_height(view.stats.len()) - 5. - m::MEMORY_SLOT_FOOTER_RESERVE;
                    if let Some(slots) =
                        page_ui(&mut child, virtual_page, view, colors, kind, &perf.selected)
                    {
                        perf.devices[index].extra_ui(&mut child, slots, module_height, cx);
                    }
                } else {
                    page_ui(ui, shifted, view, colors, kind, &perf.selected);
                }
                GraphResponse::default()
            }
        };
        if graph_response.suppress_summary {
            toggle_summary = false;
        }
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
            let kind = device.device_type();
            let colors = device_colors(kind);
            let summary = device.summary_mut();
            style_graph(&mut summary.graph, colors);
            let graph = Rect::from_min_size(
                pos2(list.left() + 14., item.top() + 10.),
                vec2(60., h - 20.),
            );
            if kind == DeviceType::System {
                paint::outline(
                    &painter,
                    graph.left() + 8.,
                    graph.top() + 6.,
                    graph.right() - 8.,
                    graph.bottom() - 10.,
                    pal::LABEL,
                );
                paint::hline(
                    &painter,
                    graph.left() + 20.,
                    graph.right() - 20.,
                    graph.bottom() - 4.,
                    pal::LABEL,
                );
                paint::vline(
                    &painter,
                    graph.center().x,
                    graph.bottom() - 10.,
                    graph.bottom() - 4.,
                    pal::LABEL,
                );
            } else if kind == DeviceType::Battery {
                let origin = pos2(
                    (graph.center().x - m::BATTERY_SIDEBAR_SIZE[0] / 2.).round(),
                    (graph.center().y - m::BATTERY_SIDEBAR_SIZE[1] / 2.).round(),
                );
                battery_glyph(&painter, origin, m::BATTERY_SIDEBAR_OUTLINE);
            } else {
                summary.graph.paint(&child, graph);
            }

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
            let mut tooltip = format!("{}\n{}\n{}", summary.heading, summary.info1, summary.info2);
            if let Some(extra) = &summary.tooltip {
                tooltip.push_str("\n\n");
                tooltip.push_str(extra);
            }
            response.on_hover_text(tooltip);
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
    suppress_summary: bool,
}

/// Original battery glyph family. Straight edges use the same pixel meshes as other Win10 ink.
fn battery_glyph(painter: &egui::Painter, origin: Pos2, segments: &[([f32; 2], [f32; 2])]) {
    let ppp = painter.ctx().pixels_per_point();
    let origin = pos2(
        (origin.x * ppp).round() / ppp,
        (origin.y * ppp).round() / ppp,
    );
    for &([x0, y0], [x1, y1]) in segments {
        let (x0, y0, x1, y1) = (origin.x + x0, origin.y + y0, origin.x + x1, origin.y + y1);
        if y0 == y1 {
            paint::hline(painter, x0.min(x1), x0.max(x1), y0, pal::LABEL);
        } else if x0 == x1 {
            paint::vline(painter, x0, y0.min(y1), y0.max(y1), pal::LABEL);
        } else {
            paint::line(painter, (x0, y0), (x1, y1), pal::LABEL);
        }
    }
}

fn battery_card_ui(ui: &mut Ui, rect: Rect, card: &BatteryCard, selected: bool) -> egui::Response {
    let response = ui.interact(rect, Id::new(("battery-card", &card.key)), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::CollapsingHeader,
            ui.is_enabled(),
            selected,
            &card.accessible_label,
        )
    });
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_expanded(selected));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected || response.hovered() {
            paint::fill(
                painter,
                rect.left(),
                rect.top(),
                rect.right() - 1.,
                rect.bottom() - 1.,
                if selected { pal::SELECTION } else { pal::HOVER },
            );
        }
        paint::outline(
            painter,
            rect.left(),
            rect.top(),
            rect.right() - 1.,
            rect.bottom() - 1.,
            if response.has_focus() {
                pal::BUTTON_HOVER_BORDER
            } else {
                pal::HEADER_SEPARATOR_FLAT
            },
        );
        let segments = match card.kind {
            BatteryType::Bat | BatteryType::Ups => m::BATTERY_GLYPH,
            BatteryType::Mouse => m::BATTERY_MOUSE_GLYPH,
            BatteryType::Headset | BatteryType::Headphones => m::BATTERY_HEADPHONES_GLYPH,
            _ => m::BATTERY_GENERIC_GLYPH,
        };
        battery_glyph(
            painter,
            rect.min + vec2(m::BATTERY_CARD_PAD, m::BATTERY_CARD_PAD),
            segments,
        );
        let name_x =
            rect.left() + m::BATTERY_CARD_PAD + m::BATTERY_GLYPH_SIZE + m::BATTERY_CARD_NAME_GAP;
        let right = rect.right() - m::BATTERY_CARD_PAD;
        let name = paint::elide(painter, &card.name, Font::Body, right - name_x);
        paint::text(
            painter,
            name_x,
            rect.top() + m::BATTERY_CARD_NAME_DY,
            &name,
            Font::Body,
            pal::TEXT,
            Align::Min,
        );
        let left = rect.left() + m::BATTERY_CARD_PAD;
        paint::text(
            painter,
            left,
            rect.top() + m::BATTERY_CARD_PERCENT_DY,
            &card.percentage_label,
            Font::StatValue,
            pal::TEXT,
            Align::Min,
        );
        let state_x = left
            + paint::text_width(painter, &card.percentage_label, Font::StatValue)
            + m::BATTERY_CARD_PAD;
        let state = paint::elide(painter, &card.state, Font::Body, right - state_x);
        paint::text(
            painter,
            right,
            rect.top() + m::BATTERY_CARD_STATE_DY,
            &state,
            Font::Body,
            pal::LABEL,
            Align::Max,
        );
        // Already-resolved ink: exactly the same pair element as Graph's group strokes.
        let ink = card.colors[usize::from(ui.ctx().theme() == egui::Theme::Dark)];
        let y = rect.top() + m::BATTERY_CARD_BAR_DY;
        let bottom = y + m::BATTERY_CARD_BAR_H - 1.;
        for (from, to) in [
            ((left, y), (right - 1., y)),
            ((left, bottom), (right - 1., bottom)),
            ((left, y), (left, bottom)),
            ((right - 1., y), (right - 1., bottom)),
        ] {
            paint::line_ink(painter, from, to, ink);
        }
        if let Some(percentage) = card.percentage.filter(|p| *p > 0.) {
            let mut fill = egui::Mesh::default();
            fill.add_colored_rect(
                Rect::from_min_max(
                    pos2(left + 2., y + 2.),
                    pos2(left + 2. + (right - left - 4.) * percentage, bottom - 1.),
                ),
                ink,
            );
            painter.add(egui::Shape::mesh(fill));
        }
    }
    response.on_hover_text(&card.accessible_label)
}

fn battery_page_ui(
    ui: &mut Ui,
    page: Rect,
    mut view: BatteriesView<'_>,
    scroll: &mut listview::ListState,
    selected: &mut Option<String>,
) -> GraphResponse {
    if selected
        .as_ref()
        .is_some_and(|key| !view.cards.iter().any(|card| card.key == *key))
    {
        *selected = None;
    }
    let gx0 = page.left() + GRAPH_X - LIST_W - 1.;
    let gx1 = page.right() - RIGHT_INSET;
    let room = (gx1 - gx0 + 1.).max(1.);
    let columns = ((room + m::BATTERY_CARD_GAP) / (m::BATTERY_CARD_MIN_W + m::BATTERY_CARD_GAP))
        .floor()
        .max(1.) as usize;
    let card_w = ((room - (columns - 1) as f32 * m::BATTERY_CARD_GAP) / columns as f32).floor();
    let rows = view.cards.len().div_ceil(columns);
    let graph_h = (page.height() * 0.5)
        .clamp(m::BATTERY_GRAPH_MIN_H, m::BATTERY_GRAPH_MAX_H)
        .round();
    let cards_y = GRAPH_TOP + graph_h + m::BATTERY_SECTION_GAP;
    let cards_h =
        rows as f32 * m::BATTERY_CARD_H + rows.saturating_sub(1) as f32 * m::BATTERY_CARD_GAP;
    let details_y = cards_y + cards_h + m::BATTERY_SECTION_GAP;

    // Established flowing statistic typography; indices, not another device-data model.
    let mut stat_rows = vec![];
    let mut detail_h = 0.;
    if let Some(detail) = &view.detail {
        for (row, entries) in detail.stats.iter().enumerate() {
            let mut line = vec![];
            let mut width = 0.;
            for (column, (label, value)) in entries.iter().enumerate() {
                let entry_w = stat_width(ui.painter(), label, value).min(room);
                if !line.is_empty() && width + entry_w > room {
                    stat_rows.push(line);
                    line = vec![];
                    width = 0.;
                }
                line.push((row, column, entry_w));
                width += entry_w + STAT_COLUMN_GAP;
            }
            if !line.is_empty() {
                stat_rows.push(line);
            }
        }
        let mut widths: Vec<Vec<f32>> = stat_rows
            .iter()
            .map(|line| line.iter().map(|entry| entry.2).collect())
            .collect();
        align_columns(&mut widths, room);
        for (line, widths) in stat_rows.iter_mut().zip(widths) {
            for (entry, width) in line.iter_mut().zip(widths) {
                entry.2 = width;
            }
        }
        detail_h = m::BATTERY_DETAIL_HEADING_H
            + detail.panels.len() as f32
                * (13. + m::BATTERY_DETAIL_GRAPH_H + m::BATTERY_SECTION_GAP)
            + stat_rows.len() as f32 * STAT_PITCH
            + m::BATTERY_SECTION_GAP
            + detail.info.len() as f32 * INFO_PITCH
            + m::BATTERY_SECTION_GAP;
    }
    let total_h = details_y + detail_h;
    if ui.rect_contains_pointer(page) {
        scroll.offset -= wheel_delta(ui);
    }
    scroll.offset = scroll
        .offset
        .clamp(0., (total_h - page.height()).max(0.))
        .round();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("battery-performance-page")
            .max_rect(page),
    );
    child.set_clip_rect(page.intersect(ui.clip_rect()));
    let track = Rect::from_min_max(pos2(page.right() - m::SCROLLBAR_W, page.top()), page.max);
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
    paint::text(
        &painter,
        page.left() + TITLE_X - LIST_W - 1.,
        top + TITLE_CAP,
        &view.overview.title,
        Font::Title,
        pal::TEXT,
        Align::Min,
    );
    paint::text(
        &painter,
        gx1 + 1.,
        top + MODEL_CAP,
        &view.overview.model,
        Font::Model,
        pal::TEXT,
        Align::Max,
    );
    let mut out = child
        .push_id("battery-overview", |ui| {
            graph_panels_ui(
                ui,
                gx0,
                gx1,
                top + GRAPH_TOP,
                &mut view.overview.panels,
                &[],
                &[graph_h],
                pal::BATTERY,
            )
        })
        .inner;

    for (index, card) in view.cards.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(
                gx0 + (index % columns) as f32 * (card_w + m::BATTERY_CARD_GAP),
                top + cards_y
                    + (index / columns) as f32 * (m::BATTERY_CARD_H + m::BATTERY_CARD_GAP),
            ),
            vec2(card_w, m::BATTERY_CARD_H),
        );
        // Register offscreen cards too: their clipped hit rectangles do not steal pointer input,
        // but Tab can still focus and reveal the entire inventory.
        let response = battery_card_ui(
            &mut child,
            rect,
            card,
            selected.as_deref() == Some(&card.key),
        );
        out.suppress_summary |= response.contains_pointer() || response.clicked();
        if response.clicked() {
            *selected = if selected.as_deref() == Some(&card.key) {
                None
            } else {
                Some(card.key.clone())
            };
            child.ctx().request_repaint();
        }
        if response.secondary_clicked() {
            out.menu = response.interact_pointer_pos().map(|pos| (pos, None));
        }
        if response.gained_focus() {
            let before = scroll.offset;
            if rect.top() < page.top() {
                scroll.offset -= page.top() - rect.top();
            } else if rect.bottom() > page.bottom() {
                scroll.offset += rect.bottom() - page.bottom();
            }
            scroll.offset = scroll
                .offset
                .clamp(0., (total_h - page.height()).max(0.))
                .round();
            if scroll.offset != before {
                child.ctx().request_repaint();
            }
        }
    }
    if let Some(mut detail) = view.detail {
        let y = top + details_y;
        let model = paint::elide(&painter, &detail.model, Font::Model, room);
        paint::text(
            &painter,
            gx0,
            y + 6.,
            &model,
            Font::Model,
            pal::TEXT,
            Align::Min,
        );
        paint::copy_value(
            &mut child,
            Rect::from_min_max(pos2(gx0, y), pos2(gx1 + 1., y + 25.)),
            &detail.model,
        );
        let mut cursor = y + m::BATTERY_DETAIL_HEADING_H;
        if !detail.panels.is_empty() {
            let heights = vec![m::BATTERY_DETAIL_GRAPH_H; detail.panels.len()];
            let graph_response = child
                .push_id("battery-detail", |ui| {
                    graph_panels_ui(
                        ui,
                        gx0,
                        gx1,
                        cursor + 13.,
                        &mut detail.panels,
                        &[],
                        &heights,
                        pal::BATTERY,
                    )
                })
                .inner;
            out.double_clicked |= graph_response.double_clicked;
            out.menu = out.menu.or(graph_response.menu);
            cursor +=
                heights.len() as f32 * (13. + m::BATTERY_DETAIL_GRAPH_H + m::BATTERY_SECTION_GAP);
        }
        for row in &stat_rows {
            let mut x = gx0;
            for &(source_row, source_column, width) in row {
                let (label, value) = &detail.stats[source_row][source_column];
                let label = paint::elide(&painter, label, Font::Body, width);
                let value = paint::elide(&painter, value, Font::StatValue, width);
                paint::text(
                    &painter,
                    x,
                    cursor + 5.,
                    &label,
                    Font::Body,
                    pal::LABEL,
                    Align::Min,
                );
                paint::text(
                    &painter,
                    x,
                    cursor + 5. + STAT_VALUE_DY,
                    &value,
                    Font::StatValue,
                    pal::TEXT,
                    Align::Min,
                );
                x += width + STAT_COLUMN_GAP;
            }
            cursor += STAT_PITCH;
        }
        cursor += m::BATTERY_SECTION_GAP;
        let label_w = detail
            .info
            .iter()
            .map(|(label, _)| paint::text_width(&painter, label, Font::Body))
            .fold(0., f32::max)
            .min(room * 0.6);
        let value_x = gx0 + INFO_VALUE_DX.max(label_w + 12.);
        for (label, value) in &detail.info {
            let short_label = paint::elide(&painter, label, Font::Body, label_w);
            let short_value = paint::elide(&painter, value, Font::Body, gx1 + 1. - value_x);
            paint::text(
                &painter,
                gx0,
                cursor,
                &short_label,
                Font::Body,
                pal::LABEL,
                Align::Min,
            );
            paint::text(
                &painter,
                value_x,
                cursor,
                &short_value,
                Font::Body,
                pal::TEXT,
                Align::Min,
            );
            paint::copy_value(
                &mut child,
                Rect::from_min_max(pos2(value_x, cursor - 3.), pos2(gx1 + 1., cursor + 12.)),
                value,
            );
            cursor += INFO_PITCH;
        }
    }
    out
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
            let entry_w = stat_width(&painter, &entry.0, &entry.1);
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
    let mut widths: Vec<Vec<f32>> = stat_rows
        .iter()
        .map(|line| line.iter().map(|entry| entry.1).collect())
        .collect();
    align_columns(&mut widths, room);
    for (line, widths) in stat_rows.iter_mut().zip(widths) {
        for (entry, width) in line.iter_mut().zip(widths) {
            entry.1 = width;
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
    paint::copy_value(
        &mut child,
        Rect::from_min_max(
            pos2(
                gx1 + 1. - paint::text_width(&painter, &model, Font::Model),
                top + MODEL_CAP - 3.,
            ),
            pos2(gx1 + 1., top + MODEL_CAP + 15.),
        ),
        &view.model,
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
        paint::copy_value(
            &mut child,
            Rect::from_min_max(pos2(value_x, cap - 3.), pos2(gx1 + 1., cap + 12.)),
            value,
        );
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

/// System has metadata only; reuse the measured title/typography and standard scrollbar.
fn system_page_ui(ui: &mut Ui, page: Rect, view: PageView<'_>, scroll: &mut listview::ListState) {
    let total_h = GRAPH_TOP + view.info.len() as f32 * INFO_PITCH + PANEL_GAP;
    if ui.rect_contains_pointer(page) {
        scroll.offset -= wheel_delta(ui);
    }
    scroll.offset = scroll
        .offset
        .clamp(0., (total_h - page.height()).max(0.))
        .round();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("system-performance-page")
            .max_rect(page),
    );
    child.set_clip_rect(page.intersect(ui.clip_rect()));
    let track = Rect::from_min_max(pos2(page.right() - m::SCROLLBAR_W, page.top()), page.max);
    listview::scrollbar(
        &mut child,
        track,
        scroll,
        INFO_PITCH,
        total_h,
        page.height(),
    );
    let painter = child.painter().clone();
    let x = page.left() + TITLE_X - LIST_W - 1.;
    let right = page.right() - RIGHT_INSET;
    let top = page.top() - scroll.offset;
    paint::text(
        &painter,
        x,
        top + TITLE_CAP,
        &view.title,
        Font::Title,
        pal::TEXT,
        Align::Min,
    );
    let label_w = view
        .info
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, _)| paint::text_width(&painter, k, Font::Body))
        .fold(INFO_VALUE_DX, f32::max)
        .min((right - x) * 0.5);
    let value_x = x + label_w + m::METADATA_VALUE_GAP;
    for (row, (label, value)) in view.info.iter().enumerate() {
        let cap = top + GRAPH_TOP + row as f32 * INFO_PITCH;
        let rect = Rect::from_min_max(pos2(x, cap - 3.), pos2(right, cap + 12.));
        if !child.is_rect_visible(rect) {
            continue;
        }
        let heading = value.is_empty();
        let label = paint::elide(
            &painter,
            label,
            Font::Body,
            if heading { right - x } else { label_w },
        );
        paint::text(
            &painter,
            x,
            cap,
            &label,
            Font::Body,
            if heading { pal::GROUP_TEXT } else { pal::LABEL },
            Align::Min,
        );
        if !heading {
            let short = paint::elide(&painter, value, Font::Body, right - value_x);
            paint::text(
                &painter,
                value_x,
                cap,
                &short,
                Font::Body,
                pal::TEXT,
                Align::Min,
            );
            paint::copy_value(
                &mut child,
                Rect::from_min_max(pos2(value_x, cap - 3.), pos2(right, cap + 12.)),
                value,
            );
        }
    }
}

/// Graph-only summary; the caller also omits the sidebar, and App omits its shell chrome.
fn summary_ui(
    ui: &mut Ui,
    content: Rect,
    mut view: PageView<'_>,
    tiles: &[GraphTile],
    colors: pal::DeviceColors,
    device: &str,
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
            .id_salt(("perf-summary-graphs", device))
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
                    let y = band_top + m::GPU_DROPDOWN_DY + row;
                    paint::fill(
                        &painter,
                        arrow_x + row,
                        y,
                        arrow_x + m::GPU_DROPDOWN_W - 1. - row,
                        y,
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
            } else if let Some(tooltip) = panel.tooltips.get(slot) {
                let mut response = response;
                response.rect = response.rect.intersect(ui.clip_rect());
                response.on_hover_text_at_pointer(tooltip);
            }
        }
        graph_top += height + m::GPU_PANEL_GAP + 13.;
    }
    out
}

/// Draws a device page into `content` (device list excluded).
fn page_ui(
    ui: &mut Ui,
    content: Rect,
    mut view: PageView<'_>,
    colors: pal::DeviceColors,
    kind: DeviceType,
    device: &str,
) -> Option<Rect> {
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
    paint::copy_value_with_tooltip(
        ui,
        Rect::from_min_max(
            pos2(
                gx1 + 1. - paint::text_width(&painter, &model, Font::Model),
                top + MODEL_CAP - 3.,
            ),
            pos2(gx1 + 1., top + MODEL_CAP + 15.),
        ),
        &view.model,
        view.model_tooltip,
    );

    // Statistics and key/value rows at the bottom
    let stats_h = stats_height(view.stats.len());
    let stats_top = (content.bottom() - PANEL_GAP - stats_h).round();
    let mut stats_right = gx0;
    let mut widths: Vec<Vec<f32>> = view
        .stats
        .iter()
        .map(|row| {
            row.iter()
                .map(|(label, value)| stat_width(&painter, label, value))
                .collect()
        })
        .collect();
    align_columns(&mut widths, gx1 + 1. - gx0);
    for (r, (row, widths)) in view.stats.iter().zip(&widths).enumerate() {
        let cap = stats_top + 5. + STAT_PITCH * r as f32;
        let mut x = gx0;
        for ((label, value), &w) in row.iter().zip(widths) {
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
            stats_right = stats_right.max(x + w);
            x += (w + STAT_COLUMN_GAP).round();
        }
    }
    let mut label_x = (left + INFO_X).max((stats_right + 24.).round());
    let bottom_limit = content.bottom() - 12.;
    let mut i = 0;
    let mut slots_rect = None;
    // Keep measured columns; overflow is accessible in the metadata popup below.
    while i < view.info.len() && label_x + 160. <= gx1 {
        let fit = (((bottom_limit - (stats_top + 5.)) / INFO_PITCH).floor() as usize).max(1);
        let column = &view.info[i..(i + if kind == DeviceType::Memory {
            4.min(fit)
        } else {
            fit
        })
        .min(view.info.len())];
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
            let full_value = value;
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
            paint::copy_value(
                ui,
                Rect::from_min_max(
                    pos2(value_x, cap - 3.),
                    pos2(
                        (value_x + paint::text_width(&painter, &value, Font::Body)).min(gx1 + 1.),
                        cap + 12.,
                    ),
                ),
                full_value,
            );
        }
        i += column.len();
        label_x = (value_x + value_w + 24.).round();
        if kind == DeviceType::Memory {
            let right_start = label_x;
            let slot_top = stats_top + 5. - m::MEMORY_SLOT_HEADER_H;
            let slot_bottom = content.bottom();
            slots_rect = Some(if gx1 - right_start >= m::MEMORY_SLOT_W {
                Rect::from_min_max(pos2(right_start, slot_top), pos2(gx1, slot_bottom))
            } else {
                Rect::from_min_max(
                    pos2(
                        (left + INFO_X).max(stats_right + 24.),
                        slot_top + 4. * INFO_PITCH + m::MEMORY_SLOT_INFO_GAP,
                    ),
                    pos2(gx1, slot_bottom),
                )
            });
            break;
        }
    }
    if i < view.info.len() {
        let at = Rect::from_min_size(
            pos2(
                gx1 - m::METADATA_MORE_W,
                content.bottom() - m::METADATA_MORE_H - 1.,
            ),
            vec2(m::METADATA_MORE_W, m::METADATA_MORE_H),
        );
        let response = ui.interact(
            at,
            ui.id().with(("more-hardware-info", device)),
            Sense::click(),
        );
        paint::text(
            &painter,
            at.right(),
            at.top() + 3.,
            &i18n("More details…"),
            Font::Body,
            pal::LINK,
            Align::Max,
        );
        egui::Popup::menu(&response)
            .frame(menu::themed_frame(ui.ctx()))
            .show(|ui| {
                egui::ScrollArea::vertical()
                    .max_height(m::METADATA_POPUP_MAX_H)
                    .show(ui, |ui| {
                        for (label, value) in &view.info {
                            ui.horizontal(|ui| {
                                ui.label(label);
                                let response = ui.label(value);
                                paint::copy_value(ui, response.rect, value);
                            });
                        }
                    });
            });
    }

    // Graph panels share the space between the caption and the statistics
    let panels = view.panels.len().max(1) as f32;
    let first_top = top + GRAPH_TOP;
    let graphs_bottom = stats_top - PANEL_GAP - 1.;
    let composition_h = if view.composition.is_some() {
        PANEL_GAP + 13. + m::MEMORY_COMPOSITION_H
    } else {
        0.
    } + swap_block_height(&view);
    let available =
        graphs_bottom - first_top + 1. - (panels - 1.) * (PANEL_GAP + 13.) - composition_h;
    let panel_h = (available / panels).floor().max(20.);
    let mut heights = vec![panel_h; view.panels.len()];
    if kind == DeviceType::Disk && heights.len() == 2 {
        heights[0] = available - m::DISK_TRANSFER_H;
        heights[1] = m::DISK_TRANSFER_H;
    } else if view.composition.is_some()
        && let Some(last) = heights.last_mut()
    {
        // Give the last panel the rounding remainder so the bar ends at the stats gap.
        *last += available - panel_h * panels;
    }
    let mut graph_top = first_top;
    for (index, (panel, height)) in view.panels.iter_mut().zip(heights).enumerate() {
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
            let y0 = graph_top + edge(r, rows, height);
            let y1 = graph_top + edge(r + 1, rows, height) - GRAPH_GAP;
            style_graph(graph, colors);
            let rect = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
            graph.paint(ui, rect);
            if rect.width() >= 3.
                && rect.height() >= 3.
                && ui.is_rect_visible(rect)
                && let Some(tooltip) = panel.tooltips.get(k)
            {
                ui.interact(
                    rect.intersect(ui.clip_rect()),
                    ui.id().with(("perf-hover", device, index, k)),
                    Sense::hover(),
                )
                .on_hover_text_at_pointer(tooltip);
            }
        }
        graph_top += height + PANEL_GAP + 13.;
    }
    if let Some(composition) = view.composition {
        paint::text(
            &painter,
            gx0,
            graph_top - 13.,
            &i18n("Physical memory composition"),
            Font::Caption,
            pal::LABEL,
            Align::Min,
        );
        composition.ui(
            ui,
            Rect::from_min_max(
                pos2(gx0, graph_top),
                pos2(gx1 + 1., graph_top + m::MEMORY_COMPOSITION_H),
            ),
            colors,
        );
        graph_top += m::MEMORY_COMPOSITION_H + PANEL_GAP + 13.;
    }
    if let Some(block) = view.swap_compression.filter(|block| block.height() > 0.) {
        paint::text(
            &painter,
            gx0,
            graph_top - 13.,
            &i18n("Swap usage"),
            Font::Caption,
            pal::LABEL,
            Align::Min,
        );
        paint::text(
            &painter,
            gx1 + 1.,
            graph_top - 13.,
            block.caption(),
            Font::Caption,
            pal::LABEL,
            Align::Max,
        );
        block.ui(
            ui,
            Rect::from_min_max(
                pos2(gx0, graph_top),
                pos2(gx1 + 1., graph_top + block.height()),
            ),
            colors,
        );
    }
    slots_rect
}

/// Caption plus swap/compression block; zero when the system has no swap.
fn swap_block_height(view: &PageView<'_>) -> f32 {
    match view.swap_compression.map(|block| block.height()) {
        Some(h) if h > 0. => PANEL_GAP + 13. + h,
        _ => 0.,
    }
}

fn stats_height(rows: usize) -> f32 {
    5. + STAT_PITCH * (rows.max(1) - 1) as f32 + STAT_VALUE_DY + 14.
}
