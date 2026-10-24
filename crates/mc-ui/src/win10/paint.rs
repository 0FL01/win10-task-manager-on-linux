/* crates/mc-ui/src/win10/paint.rs
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

//! Pixel-exact drawing helpers. Coordinates are logical points with the window's top-left at
//! (0, 0); rectangles are given as *inclusive* pixel ranges like in docs/win10-spec/spec.md.

use egui::{Align, Color32, FontFamily, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2};

use super::fonts;

/// Fills the inclusive pixel rectangle `x0..=x1` × `y0..=y1`. Painted as a plain mesh: egui's
/// rect tessellation feathers thin rectangles into line segments, which loses single pixels.
pub fn fill(painter: &Painter, x0: f32, y0: f32, x1: f32, y1: f32, color: Color32) {
    let color = super::palette::resolve(painter.ctx(), color);
    if x1 < x0 || y1 < y0 {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.add_colored_rect(
        Rect::from_min_max(pos2(x0, y0), pos2(x1 + 1., y1 + 1.)),
        color,
    );
    painter.add(Shape::mesh(mesh));
}

/// Physical pixel cell of logical pixel `v`: it starts at the rounded position and is
/// `round(scale)` pixels long, so 1 px lines keep one thickness at fractional scales.
fn cell(v: f32, ppp: f32) -> (f32, f32) {
    let start = (v * ppp).round();
    (start, start + ppp.round().max(1.))
}

fn mesh_rect(painter: &Painter, left: f32, top: f32, right: f32, bottom: f32, color: Color32) {
    let ppp = painter.ctx().pixels_per_point();
    let mut mesh = Mesh::default();
    mesh.add_colored_rect(
        Rect::from_min_max(pos2(left / ppp, top / ppp), pos2(right / ppp, bottom / ppp)),
        color,
    );
    painter.add(Shape::mesh(mesh));
}

/// Snap the inclusive span's boundaries independently, keeping only its thickness fixed.
/// Adjacent spans then share an endpoint at fractional scales. Pixel glyphs use [`fill`].
fn rule(painter: &Painter, start: f32, end: f32, at: f32, vertical: bool, color: Color32) {
    let color = super::palette::resolve(painter.ctx(), color);
    if end < start {
        return;
    }
    let ppp = painter.ctx().pixels_per_point();
    let start = (start * ppp).round();
    let end = ((end + 1.) * ppp).round();
    let (a, b) = cell(at, ppp);
    if vertical {
        mesh_rect(painter, a, start, b, end, color);
    } else {
        mesh_rect(painter, start, a, end, b, color);
    }
}

pub fn hline(painter: &Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    rule(painter, x0, x1, y, false, color);
}

pub fn vline(painter: &Painter, x: f32, y0: f32, y1: f32, color: Color32) {
    rule(painter, y0, y1, x, true, color);
}

/// 1 px outline of the inclusive rectangle.
pub fn outline(painter: &Painter, x0: f32, y0: f32, x1: f32, y1: f32, color: Color32) {
    if x1 < x0 || y1 < y0 {
        return;
    }
    let color = super::palette::resolve(painter.ctx(), color);
    let ppp = painter.ctx().pixels_per_point();
    let (left, top) = ((x0 * ppp).round(), (y0 * ppp).round());
    let (right, bottom) = (((x1 + 1.) * ppp).round(), ((y1 + 1.) * ppp).round());
    let line = ppp.round().max(1.);
    if right - left <= 2. * line || bottom - top <= 2. * line {
        mesh_rect(painter, left, top, right, bottom, color);
        return;
    }
    for (l, t, r, b) in [
        (left, top, right, top + line),
        (left, bottom - line, right, bottom),
        (left, top + line, left + line, bottom - line),
        (right - line, top + line, right, bottom - line),
    ] {
        mesh_rect(painter, l, t, r, b, color);
    }
}

/// Vertical 1 px line whose grey level falls linearly from `top` to `bottom` (truncating, as
/// measured on the reference header separators).
pub fn vgradient(painter: &Painter, x: f32, y0: f32, y1: f32, top: Color32, bottom: Color32) {
    let rows = (y1 - y0).max(1.);
    let mut y = y0;
    while y <= y1 {
        let t = (y - y0) / rows;
        let c = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).floor() as u8;
        let color = Color32::from_rgb(
            c(top.r(), bottom.r()),
            c(top.g(), bottom.g()),
            c(top.b(), bottom.b()),
        );
        // Rows stay contiguous, the column keeps the line thickness
        let ppp = painter.ctx().pixels_per_point();
        let (left, right) = cell(x, ppp);
        let color = super::palette::resolve(painter.ctx(), color);
        mesh_rect(
            painter,
            left,
            (y * ppp).round(),
            right,
            ((y + 1.) * ppp).round(),
            color,
        );
        y += 1.;
    }
}

/// Anti-aliased 1 px line between pixel centres.
pub fn line(painter: &Painter, from: (f32, f32), to: (f32, f32), color: Color32) {
    let color = super::palette::resolve(painter.ctx(), color);
    line_ink(painter, from, to, color);
}

/// The same 1 px geometry, with ink already chosen for its surface (no palette resolution).
pub(super) fn line_ink(painter: &Painter, from: (f32, f32), to: (f32, f32), color: Color32) {
    painter.line_segment(
        [
            pos2(from.0 + 0.5, from.1 + 0.5),
            pos2(to.0 + 0.5, to.1 + 0.5),
        ],
        Stroke::new(1., color),
    );
}

/// Text roles of the interface (docs/win10-spec/spec.md, "Typography").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    /// 12 px Regular: body, menus, tabs, table cells
    Body,
    /// 12 px Semibold
    BodyStrong,
    /// 16 px Regular: Processes header summary
    Summary,
    /// 28 px Light: performance page title
    Title,
    /// 18 px Regular: device model on performance pages
    Model,
    /// 11 px Regular: graph captions
    Caption,
    /// 20 px Regular: statistic values
    StatValue,
    /// 15 px Regular: device names in the performance list
    DeviceName,
}

impl Font {
    pub fn id(self) -> FontId {
        let regular = FontFamily::Proportional;
        match self {
            Font::Body => FontId::new(12., regular),
            Font::BodyStrong => FontId::new(12., fonts::family(fonts::SEMIBOLD)),
            Font::Summary => FontId::new(16., regular),
            Font::Title => FontId::new(28., fonts::family(fonts::LIGHT)),
            Font::Model => FontId::new(18., regular),
            Font::Caption => FontId::new(11., regular),
            Font::StatValue => FontId::new(20., regular),
            Font::DeviceName => FontId::new(15., regular),
        }
    }

    /// Distance from the top of a laid-out row to the top of capital letters, measured on the
    /// bundled Open Sans so capitals land where the spec puts them.
    pub fn cap_offset(self) -> f32 {
        match self {
            Font::Body | Font::BodyStrong => 4.,
            Font::Summary => 5.,
            Font::Title => 10.,
            Font::Model => 7.,
            Font::Caption => 5.,
            Font::StatValue => 7.,
            Font::DeviceName => 4.,
        }
    }
}

/// Paints `text` so that its capitals start at `cap_top`; `x` is the left edge, right edge or
/// centre depending on `align`. Returns the painted rectangle.
pub fn text(
    painter: &Painter,
    x: f32,
    cap_top: f32,
    text: &str,
    font: Font,
    color: Color32,
    align: Align,
) -> Rect {
    let color = super::palette::resolve(painter.ctx(), color);
    let galley = painter.layout_no_wrap(text.to_owned(), font.id(), color);
    let width = galley.size().x;
    let left = match align {
        Align::Min => x,
        Align::Center => x - width / 2.,
        Align::Max => x - width,
    };
    let pos = Pos2::new(left.round(), (cap_top - font.cap_offset()).round());
    let rect = Rect::from_min_size(pos, galley.size());
    painter.galley(pos, galley, color);
    rect
}

/// Width of `text` in `font`.
pub fn text_width(painter: &Painter, text: &str, font: Font) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font.id(), Color32::BLACK)
        .size()
        .x
}

/// `text` shortened with "…" so that it fits `max_width` in `font`.
pub fn elide(painter: &Painter, text: &str, font: Font, max_width: f32) -> String {
    if text_width(painter, text, font) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if text_width(painter, &candidate, font) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        return String::new();
    }
    chars[..lo].iter().collect::<String>().trim_end().to_owned() + "…"
}

/// Shared metadata interaction: copy the full value, even when its painted form is elided.
/// Only the current highlight is retained; it expires with one scheduled repaint.
pub fn copy_value(ui: &mut egui::Ui, rect: Rect, value: &str) {
    copy_value_with_tooltip(ui, rect, value, None);
}

/// Copy the value, with optional explanatory text in the same hover interaction.
pub fn copy_value_with_tooltip(ui: &mut egui::Ui, rect: Rect, value: &str, tooltip: Option<&str>) {
    if value.is_empty() || !ui.is_rect_visible(rect) {
        return;
    }
    let id = ui
        .id()
        .with(("copy-value", rect.min.x.to_bits(), rect.min.y.to_bits()));
    let response = ui.interact(rect.intersect(ui.clip_rect()), id, egui::Sense::click());
    let state_id = egui::Id::new("metadata-copy-highlight");
    let now = ui.input(|input| input.time);
    if response.clicked() {
        ui.ctx().copy_text(value.to_owned());
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                state_id,
                (id, now + super::metrics::METADATA_COPY_MS as f64 / 1000.),
            )
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(
                super::metrics::METADATA_COPY_MS,
            ));
    }
    let highlighted = ui
        .ctx()
        .data(|data| data.get_temp::<(egui::Id, f64)>(state_id))
        .is_some_and(|(active, until)| active == id && now < until);
    if highlighted {
        outline(
            ui.painter(),
            rect.left(),
            rect.top(),
            rect.right() - 1.,
            rect.bottom() - 1.,
            super::palette::BUTTON_HOVER_BORDER,
        );
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if highlighted {
            mc_core::i18n::i18n("Copied")
        } else {
            format!(
                "{}\n{}",
                tooltip.unwrap_or(value),
                mc_core::i18n::i18n("Click to copy")
            )
        });
}
