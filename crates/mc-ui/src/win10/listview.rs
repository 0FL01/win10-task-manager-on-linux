/* crates/mc-ui/src/win10/listview.rs
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

//! Win32-style list view: column header (Processes style with a summary row, or flat),
//! virtualised rows and a 15 px scroll bar (docs/win10-spec/spec.md, "Scroll bar",
//! "Processes tab", "Details tab").

use std::ops::Range;

use egui::{Align, CursorIcon, Id, Pos2, Rect, Sense, Ui, pos2};
use mc_core::config::SortOrder;

use super::paint::{self, Font};
use super::{metrics as m, palette as pal};

/// Column header layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderKind {
    /// Processes tab: 56 px, summary value above the name, gradient separators
    Summary,
    /// Details/Services tabs: 24 px single line, flat separators
    Flat,
}

impl HeaderKind {
    pub fn height(self) -> f32 {
        match self {
            HeaderKind::Summary => 56.,
            HeaderKind::Flat => 24.,
        }
    }
}

pub struct ColumnHeader {
    pub title: String,
    pub summary: Option<String>,
    pub align: Align,
    pub sort: SortOrder,
    /// Summary drawn in the alert colour (column total ≥ 90 %)
    pub alert: bool,
}

/// Scroll position and column widths of one list.
#[derive(Debug, Clone, Default)]
pub struct ListState {
    pub offset: f32,
    pub x_offset: f32,
    pub widths: Vec<f32>,
    resize: Option<(usize, f32, f32)>,
    thumb_grab: Option<f32>,
    x_thumb_grab: Option<f32>,
    /// Column being dragged to a new position (index, pointer x at the start)
    moving: Option<(usize, f32)>,
}

impl ListState {
    pub fn new(widths: &[f32]) -> Self {
        Self {
            widths: widths.to_vec(),
            ..Default::default()
        }
    }

    /// Left edge of each column and the x of the separator after it.
    pub fn columns(&self, left: f32) -> Vec<(f32, f32)> {
        let mut x = left - self.x_offset;
        self.widths
            .iter()
            .map(|w| {
                let col = (x, x + w);
                x += w + 1.;
                col
            })
            .collect()
    }

    /// A column resize is in progress (widths are being edited in `widths`).
    pub fn is_resizing(&self) -> bool {
        self.resize.is_some()
    }

    pub fn scroll_to_row(&mut self, row: usize, row_h: f32, view_h: f32) {
        let top = row as f32 * row_h;
        if top < self.offset {
            self.offset = top;
        } else if top + row_h > self.offset + view_h {
            self.offset = top + row_h - view_h;
        }
    }
}

#[derive(Debug, Default)]
pub struct HeaderResponse {
    pub clicked: Option<usize>,
    pub context_menu: Option<Pos2>,
    /// A column was dropped at a new position: (from, to) display indices
    pub moved: Option<(usize, usize)>,
    /// A column resize ended (widths in `ListState` changed)
    pub resized: bool,
}

/// Paints the header in `rect` (its height is `kind.height()`).
pub fn header(
    ui: &mut Ui,
    rect: Rect,
    kind: HeaderKind,
    columns: &[ColumnHeader],
    state: &mut ListState,
) -> HeaderResponse {
    let painter = ui.painter().with_clip_rect(rect);
    let mut out = HeaderResponse::default();
    let spans = state.columns(rect.left());
    let bottom = rect.bottom() - 1.;

    for (i, (column, &(x0, sep))) in columns.iter().zip(&spans).enumerate() {
        let cell = Rect::from_min_max(pos2(x0, rect.top()), pos2(sep, rect.bottom()));
        // The first column (Name) stays in place; the others can be dragged to a new position
        let sense = if i == 0 {
            Sense::click()
        } else {
            Sense::click_and_drag()
        };
        let response = ui.interact(
            cell.intersect(rect),
            ui.id().with(("list-header", i)),
            sense,
        );
        if response.drag_started()
            && let Some(pos) = response.interact_pointer_pos()
        {
            state.moving = Some((i, pos.x));
        }
        if response.drag_stopped()
            && let Some((from, _)) = state.moving.take()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let to = drop_index(&spans, pos.x);
            if to != from {
                out.moved = Some((from, to));
            }
        }
        let highlight = state
            .moving
            .map(|(m, _)| m == i)
            .unwrap_or(response.hovered());
        if highlight && state.resize.is_none() {
            paint::fill(&painter, x0, rect.top(), sep - 1., bottom, pal::HOVER);
        }
        if response.clicked() {
            out.clicked = Some(i);
        }
        if response.secondary_clicked() {
            out.context_menu = response.interact_pointer_pos();
        }

        let text_x = match column.align {
            Align::Max => sep - 5.,
            _ => x0 + 8.,
        };
        match kind {
            HeaderKind::Summary => {
                paint::vgradient(
                    &painter,
                    sep,
                    rect.top() + 15.,
                    bottom,
                    pal::HEADER_SEPARATOR_TOP,
                    pal::HEADER_SEPARATOR_BOTTOM,
                );
                if let Some(summary) = &column.summary {
                    let ink = if column.alert {
                        pal::SUMMARY_ALERT
                    } else {
                        pal::TEXT
                    };
                    // Values too wide for the column (e.g. rates) fall back to the body font
                    let room = sep - x0 - 8.;
                    let (font, cap) = if paint::text_width(&painter, summary, Font::Summary) <= room
                    {
                        (Font::Summary, rect.top() + 19.)
                    } else {
                        (Font::Body, rect.top() + 21.)
                    };
                    let summary = paint::elide(&painter, summary, font, room);
                    paint::text(&painter, text_x, cap, &summary, font, ink, column.align);
                }
                paint::text(
                    &painter,
                    text_x,
                    rect.top() + 40.,
                    &column.title,
                    Font::Body,
                    pal::HEADER_NAME,
                    column.align,
                );
                if column.sort != SortOrder::None {
                    chevron(
                        &painter,
                        x0 + 9.,
                        rect.top() + 20.,
                        8.,
                        column.sort,
                        pal::SORT_CHEVRON,
                    );
                }
            }
            HeaderKind::Flat => {
                paint::vline(
                    &painter,
                    sep,
                    rect.top(),
                    bottom,
                    pal::HEADER_SEPARATOR_FLAT,
                );
                let text_x = match column.align {
                    Align::Max => sep - 6.,
                    _ => x0 + 7.,
                };
                paint::text(
                    &painter,
                    text_x,
                    rect.top() + 7.,
                    &column.title,
                    Font::Body,
                    pal::TEXT,
                    column.align,
                );
                if column.sort != SortOrder::None {
                    let cx = ((x0 + sep) / 2.).floor();
                    chevron(
                        &painter,
                        cx - 3.,
                        rect.top(),
                        7.,
                        column.sort,
                        pal::SORT_CHEVRON_FLAT,
                    );
                }
            }
        }
    }

    // Column resizing by dragging a separator
    for (i, &(_, sep)) in spans.iter().enumerate() {
        let grip = Rect::from_min_max(pos2(sep - 3., rect.top()), pos2(sep + 4., rect.bottom()));
        let response = ui.interact(grip, ui.id().with(("list-resize", i)), Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
        }
        if response.drag_started()
            && let Some(pos) = response.interact_pointer_pos()
        {
            state.resize = Some((i, state.widths[i], pos.x));
        }
        if response.dragged()
            && let (Some((index, width, start)), Some(pos)) =
                (state.resize, response.interact_pointer_pos())
            && index == i
        {
            state.widths[i] = (width + pos.x - start).round().max(24.);
        }
        if response.drag_stopped() && state.resize.take().is_some() {
            out.resized = true;
        }
    }

    // Drop marker while a column is being moved
    if let Some((from, _)) = state.moving
        && let Some(pos) = ui.ctx().pointer_interact_pos()
    {
        let to = drop_index(&spans, pos.x);
        // Marker on the side the column will be inserted
        let x = if to > from {
            spans[to].1
        } else {
            spans[to].0 - 1.
        };
        paint::fill(
            &painter,
            x - 1.,
            rect.top(),
            x + 1.,
            bottom,
            pal::BUTTON_HOVER_BORDER,
        );
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    out
}

/// Display index a moved column takes: that of the column under the pointer (the first column
/// is fixed). Apply with `remove(from)` + `insert(to)`.
fn drop_index(spans: &[(f32, f32)], x: f32) -> usize {
    let last = spans.len().saturating_sub(1);
    spans
        .iter()
        .position(|&(_, sep)| x <= sep)
        .unwrap_or(last)
        .clamp(1.min(last), last)
}

/// Moves `items[from]` to `to` like a header drop.
pub fn apply_move<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from < items.len() && to < items.len() {
        let item = items.remove(from);
        items.insert(to, item);
    }
}

/// Sort indicator: a 1 px chevron `width` wide and `(width + 1) / 2` tall, pointing down for
/// descending order.
pub fn chevron(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    width: f32,
    order: SortOrder,
    color: egui::Color32,
) {
    let rows = ((width + 1.) / 2.).floor() as i32;
    for r in 0..rows {
        let r = r as f32;
        let row_y = if order == SortOrder::Descending {
            y + r
        } else {
            y + rows as f32 - 1. - r
        };
        paint::fill(painter, x + r, row_y, x + r, row_y, color);
        let right = x + width - 1. - r;
        paint::fill(painter, right, row_y, right, row_y, color);
    }
}

#[derive(Debug, Default)]
pub struct RowsResponse {
    pub hovered: Option<usize>,
    pub clicked: Option<usize>,
    pub double_clicked: Option<usize>,
    pub context_menu: Option<(usize, Pos2)>,
}

/// Handles wheel scrolling and row interaction in `body`; returns the visible row range.
pub fn rows(
    ui: &mut Ui,
    id: Id,
    body: Rect,
    state: &mut ListState,
    row_h: f32,
    count: usize,
) -> (Range<usize>, RowsResponse) {
    let content_h = count as f32 * row_h;
    let max_offset = (content_h - body.height()).max(0.);

    let response = ui.interact(body, id, Sense::click());
    if ui.rect_contains_pointer(body) {
        // Raw wheel events (no smooth-scroll animation frames); a notch scrolls 3 rows
        let delta: f32 = ui.input(|i| {
            i.raw
                .events
                .iter()
                .map(|e| match e {
                    egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } if !modifiers.shift => match unit {
                        egui::MouseWheelUnit::Point => delta.y,
                        egui::MouseWheelUnit::Line => delta.y * 3. * row_h,
                        egui::MouseWheelUnit::Page => delta.y * body.height(),
                    },
                    _ => 0.,
                })
                .sum()
        });
        state.offset -= delta;
    }
    state.offset = state.offset.clamp(0., max_offset).round();

    let row_at = |pos: Pos2| {
        let index = ((pos.y - body.top() + state.offset) / row_h).floor();
        (index >= 0. && (index as usize) < count && body.contains(pos)).then_some(index as usize)
    };
    let mut out = RowsResponse {
        hovered: response.hover_pos().and_then(row_at),
        ..Default::default()
    };
    if let Some(pos) = response.interact_pointer_pos() {
        // egui reports a "triple" click when an unrelated click happened shortly before (it
        // measures the distance to the previous click only); treat it as a double click
        if response.double_clicked() || response.triple_clicked() {
            out.double_clicked = row_at(pos);
        } else if response.clicked() {
            out.clicked = row_at(pos);
        }
        if response.secondary_clicked()
            && let Some(row) = row_at(pos)
        {
            out.context_menu = Some((row, pos));
        }
    }

    let first = (state.offset / row_h).floor() as usize;
    let last = (((state.offset + body.height()) / row_h).ceil() as usize).min(count);
    (first.min(last)..last, out)
}

/// Top of row `index` on screen.
pub fn row_top(body: Rect, state: &ListState, row_h: f32, index: usize) -> f32 {
    body.top() + index as f32 * row_h - state.offset
}

/// Returns the bottom space occupied by horizontal overflow. Both header and rows
/// use `columns`, so dragging/resizing and scrolling retain the same hit-test coordinates.
pub fn horizontal_scrollbar(ui: &mut Ui, content: Rect, state: &mut ListState) -> f32 {
    let view = (content.width() - m::SCROLLBAR_W).max(1.);
    let width: f32 = state.widths.iter().map(|w| w + 1.).sum();
    let maximum = (width - view).max(0.);
    if maximum == 0. {
        state.x_offset = 0.;
        return 0.;
    }
    if ui.rect_contains_pointer(content) {
        let delta: f32 = ui.input(|i| {
            i.raw
                .events
                .iter()
                .map(|event| {
                    if let egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } = event
                    {
                        let delta = if modifiers.shift { delta.y } else { delta.x };
                        delta
                            * match unit {
                                egui::MouseWheelUnit::Point => 1.,
                                egui::MouseWheelUnit::Line => m::TABLE_HORIZONTAL_STEP,
                                egui::MouseWheelUnit::Page => view,
                            }
                    } else {
                        0.
                    }
                })
                .sum()
        });
        state.x_offset -= delta;
    }
    state.x_offset = state.x_offset.clamp(0., maximum).round();
    let track = Rect::from_min_max(
        pos2(content.left(), content.bottom() - m::SCROLLBAR_W),
        pos2(content.left() + view, content.bottom()),
    );
    let painter = ui.painter().with_clip_rect(track);
    paint::fill(
        &painter,
        track.left(),
        track.top(),
        track.right() - 1.,
        track.bottom() - 1.,
        pal::SCROLL_TRACK,
    );
    let left = Rect::from_min_max(
        track.min,
        pos2(track.left() + m::SCROLL_ARROW_H, track.bottom()),
    );
    let right = Rect::from_min_max(
        pos2(track.right() - m::SCROLL_ARROW_H, track.top()),
        track.max,
    );
    for (rect, forward) in [(left, false), (right, true)] {
        let response = ui.interact(
            rect,
            ui.id().with(("list-x-arrow", forward)),
            Sense::click(),
        );
        if response.clicked() {
            state.x_offset += if forward {
                m::TABLE_HORIZONTAL_STEP
            } else {
                -m::TABLE_HORIZONTAL_STEP
            };
        }
        if response.hovered() {
            paint::fill(
                &painter,
                rect.left(),
                rect.top(),
                rect.right() - 1.,
                rect.bottom() - 1.,
                pal::POPUP_SEPARATOR,
            );
        }
        for d in 0..4 {
            let x = rect.center().x.round()
                + if forward {
                    2. - d as f32
                } else {
                    d as f32 - 2.
                };
            paint::vline(
                &painter,
                x,
                rect.center().y.round() - d as f32,
                rect.center().y.round() + d as f32,
                pal::TEXT,
            );
        }
    }
    let lane = Rect::from_min_max(
        pos2(left.right(), track.top()),
        pos2(right.left(), track.bottom()),
    );
    let thumb_width = (lane.width() * view / width)
        .max(m::SCROLL_THUMB_MIN)
        .min(lane.width())
        .round();
    let travel = (lane.width() - thumb_width).max(0.);
    let thumb_left = (lane.left() + travel * state.x_offset.clamp(0., maximum) / maximum).round();
    let thumb = Rect::from_min_max(
        pos2(thumb_left, track.top()),
        pos2(thumb_left + thumb_width, track.bottom()),
    );
    let response = ui.interact(lane, ui.id().with("list-x-thumb"), Sense::click_and_drag());
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.x_thumb_grab = thumb.contains(pos).then_some(pos.x - thumb_left);
    }
    if response.dragged()
        && travel > 0.
        && let (Some(grab), Some(pos)) = (state.x_thumb_grab, response.interact_pointer_pos())
    {
        state.x_offset = (pos.x - grab - lane.left()) / travel * maximum;
    }
    if response.drag_stopped() {
        state.x_thumb_grab = None;
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && !thumb.contains(pos)
    {
        state.x_offset += if pos.x < thumb_left { -view } else { view };
    }
    state.x_offset = state.x_offset.clamp(0., maximum).round();
    paint::fill(
        &painter,
        thumb.left(),
        thumb.top(),
        thumb.right() - 1.,
        thumb.bottom() - 1.,
        if response.hovered() || state.x_thumb_grab.is_some() {
            pal::SCROLL_THUMB_HOVER
        } else {
            pal::SCROLL_THUMB
        },
    );
    m::SCROLLBAR_W
}

/// Vertical scroll bar in `track` (full height including the arrow buttons). Returns `false`
/// and paints nothing when everything fits.
pub fn scrollbar(
    ui: &mut Ui,
    track: Rect,
    state: &mut ListState,
    row_h: f32,
    content_h: f32,
    view_h: f32,
) -> bool {
    if content_h <= view_h {
        state.offset = 0.;
        return false;
    }
    let painter = ui.painter().clone();
    let (x0, x1) = (track.left(), track.right() - 1.);
    let (y0, y1) = (track.top(), track.bottom() - 1.);
    paint::fill(&painter, x0, y0, x1, y1, pal::SCROLL_TRACK);

    let max_offset = content_h - view_h;
    let up = Rect::from_min_max(pos2(x0, y0), pos2(x1 + 1., y0 + m::SCROLL_ARROW_H));
    let down = Rect::from_min_max(
        pos2(x0, y1 + 1. - m::SCROLL_ARROW_H),
        pos2(x1 + 1., y1 + 1.),
    );
    let cx = ((x0 + x1) / 2.).round();
    for (rect, is_up) in [(up, true), (down, false)] {
        let response = ui.interact(
            rect,
            ui.id().with(("list-scroll-arrow", is_up)),
            Sense::click(),
        );
        if response.hovered() {
            paint::fill(
                &painter,
                rect.left(),
                rect.top(),
                rect.right() - 1.,
                rect.bottom() - 1.,
                pal::POPUP_SEPARATOR,
            );
        }
        if response.clicked() {
            state.offset += if is_up { -row_h } else { row_h };
        }
        arrow(
            &painter,
            cx,
            if is_up {
                rect.top() + 6.
            } else {
                rect.top() + 7.
            },
            is_up,
        );
    }

    let lane_top = up.bottom();
    let lane_h = down.top() - lane_top;
    let thumb_h = (lane_h * view_h / content_h)
        .max(m::SCROLL_THUMB_MIN)
        .round();
    let travel = lane_h - thumb_h;
    let thumb_top = (lane_top + travel * state.offset.clamp(0., max_offset) / max_offset).round();
    let thumb = Rect::from_min_max(pos2(x0, thumb_top), pos2(x1 + 1., thumb_top + thumb_h));

    let lane = Rect::from_min_max(pos2(x0, lane_top), pos2(x1 + 1., down.top()));
    let lane_response = ui.interact(
        lane,
        ui.id().with("list-scroll-lane"),
        Sense::click_and_drag(),
    );
    if lane_response.drag_started()
        && let Some(pos) = lane_response.interact_pointer_pos()
    {
        state.thumb_grab = thumb.contains(pos).then_some(pos.y - thumb_top);
    }
    if lane_response.dragged()
        && let (Some(grab), Some(pos)) = (state.thumb_grab, lane_response.interact_pointer_pos())
        && travel > 0.
    {
        state.offset = (pos.y - grab - lane_top) / travel * max_offset;
    }
    if lane_response.drag_stopped() {
        state.thumb_grab = None;
    }
    if lane_response.clicked()
        && let Some(pos) = lane_response.interact_pointer_pos()
        && !thumb.contains(pos)
    {
        state.offset += if pos.y < thumb_top { -view_h } else { view_h };
    }
    state.offset = state.offset.clamp(0., max_offset);

    let hot = lane_response.hovered()
        && lane_response.hover_pos().is_some_and(|p| thumb.contains(p))
        || state.thumb_grab.is_some();
    let thumb_color = if hot {
        pal::SCROLL_THUMB_HOVER
    } else {
        pal::SCROLL_THUMB
    };
    paint::fill(
        &painter,
        x0,
        thumb.top(),
        x1,
        thumb.bottom() - 1.,
        thumb_color,
    );
    true
}

/// 7×5 chevron of the scroll arrows (2 px stroke), apex at (`cx`, `top`) for up.
fn arrow(painter: &egui::Painter, cx: f32, top: f32, up: bool) {
    let rows: [&[(f32, f32)]; 5] = [
        &[(0., 0.)],
        &[(-1., 1.)],
        &[(-2., 2.)],
        &[(-3., -1.), (1., 3.)],
        &[(-3., -2.), (2., 3.)],
    ];
    for (r, spans) in rows.iter().enumerate() {
        let y = if up {
            top + r as f32
        } else {
            top + 4. - r as f32
        };
        for &(a, b) in spans.iter() {
            paint::fill(painter, cx + a, y, cx + b, y, pal::SCROLL_ARROW);
        }
    }
}
