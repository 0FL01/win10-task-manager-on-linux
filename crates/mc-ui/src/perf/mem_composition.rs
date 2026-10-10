/* crates/mc-ui/src/perf/mem_composition.rs
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

//! Horizontal "memory composition" bar: in use, modified, standby, free.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use mc_core::format::{DataType, UnitSettings, to_human_readable_nice};
use mc_core::i18n::i18n_f;
use mc_core::i18n::n_;
use mc_magpie::Memory;

const GRAPH_RADIUS: u8 = 7;
const FILL_ALPHA: u8 = 50;

#[derive(Default)]
pub struct MemComposition {
    total: u64,
    used: u64,
    modified: u64,
    standby: u64,
    free: u64,
    tooltips: [String; 4],
}

impl MemComposition {
    pub fn update(&mut self, mem: &Memory, units: &UnitSettings) {
        let total = if mem.mem_total == 0 { 1 } else { mem.mem_total };
        let mem_avail = if mem.mem_available > mem.mem_total {
            mem.mem_free
        } else {
            mem.mem_available
        };
        self.total = total;
        self.used = total.saturating_sub(mem_avail);
        self.modified = mem.dirty;
        self.standby = total.saturating_sub(self.used + mem.mem_free);
        self.free = mem.mem_free;

        let h = |v: u64| to_human_readable_nice(v as f32, &DataType::MemoryBytes, units);
        self.tooltips = [
            i18n_f(
                n_("In use ({})\n\nMemory used by the operating system and running applications"),
                &[&h(self.used)],
            ),
            i18n_f(
                n_(
                    "Modified ({})\n\nMemory whose contents must be written to disk before it can be used by another process",
                ),
                &[&h(self.modified)],
            ),
            i18n_f(
                n_(
                    "Standby ({})\n\nMemory that contains cached data and code that is not actively in use",
                ),
                &[&h(self.standby)],
            ),
            i18n_f(
                n_(
                    "Free ({})\n\nMemory that is not currently in use, and that will be repurposed first when the operating system, drivers, or applications need more memory",
                ),
                &[&h(self.free)],
            ),
        ];
    }

    pub fn ui(&self, ui: &mut Ui, size: Vec2, color: Color32) {
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), FILL_ALPHA);
        let stroke = Stroke::new(1., color);
        let total = self.total.max(1) as f32;
        let width = rect.width();

        let mut x = rect.left();
        let mut anchors = [0f32; 4];

        // used: filled
        let w_used = (width * self.used as f32 / total).trunc();
        let used = Rect::from_min_size(rect.min, Vec2::new(w_used, rect.height()));
        painter.rect_filled(used, 0., fill);
        painter.line_segment([used.right_top(), used.right_bottom()], stroke);
        x += w_used;
        anchors[0] = x;

        // modified: filled + hatched
        let w_mod = (width * self.modified as f32 / total).trunc();
        let modified =
            Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w_mod, rect.height()));
        painter.rect_filled(modified, 0., fill);
        hatch(&painter, modified, fill);
        painter.line_segment([modified.right_top(), modified.right_bottom()], stroke);
        x += w_mod;
        anchors[1] = x;

        // standby: hatched only
        let w_standby = (width * self.standby as f32 / total).trunc();
        let standby = Rect::from_min_size(
            Pos2::new(x, rect.top()),
            Vec2::new(w_standby, rect.height()),
        );
        hatch(&painter, standby, fill);
        painter.line_segment([standby.right_top(), standby.right_bottom()], stroke);
        x += w_standby;
        anchors[2] = x;
        anchors[3] = rect.right() + 1.;

        painter.rect_stroke(
            rect.shrink(0.5),
            CornerRadius::same(GRAPH_RADIUS),
            stroke,
            StrokeKind::Middle,
        );

        let response = if let Some(pos) = response.hover_pos() {
            let index = anchors.iter().position(|a| *a >= pos.x).unwrap_or(0);
            response.on_hover_text(&self.tooltips[index])
        } else {
            response
        };
        let _ = response;
    }
}

fn hatch(painter: &egui::Painter, rect: Rect, color: Color32) {
    if rect.width() < 1. {
        return;
    }
    let step = 4.;
    let mut shapes = vec![];
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        let a = Pos2::new(x, rect.bottom());
        let b = Pos2::new(x + rect.height(), rect.top());
        shapes.push(Shape::line_segment([a, b], Stroke::new(1., color)));
        x += step;
    }
    painter.with_clip_rect(rect).extend(shapes);
}
