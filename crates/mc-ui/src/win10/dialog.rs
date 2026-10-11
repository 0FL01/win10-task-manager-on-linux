/* crates/mc-ui/src/win10/dialog.rs
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

//! Win32-style building blocks of dialogs (own design).

use egui::{Response, RichText, Sense, Stroke, Ui, Vec2, pos2};

use super::palette as pal;

/// Secondary (grey) text.
pub fn caption(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Label::new(RichText::new(text.into()).color(pal::LABEL)).selectable(false))
}

/// Win32 check box without a label; returns a response marked changed on toggle.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(13.), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let painter = ui.painter();
    let border = if response.hovered() {
        pal::BUTTON_HOVER_BORDER
    } else {
        BOX_BORDER
    };
    let r = egui::Rect::from_min_size(rect.min.round(), rect.size());
    super::paint::fill(
        painter,
        r.left(),
        r.top(),
        r.right() - 1.,
        r.bottom() - 1.,
        pal::WHITE,
    );
    super::paint::outline(
        painter,
        r.left(),
        r.top(),
        r.right() - 1.,
        r.bottom() - 1.,
        border,
    );
    if *on {
        painter.line(
            vec![
                pos2(r.left() + 3., r.center().y),
                pos2(r.left() + 5.5, r.bottom() - 3.5),
                pos2(r.right() - 3., r.top() + 3.5),
            ],
            Stroke::new(1.5, pal::TEXT),
        );
    }
    response
}

/// Check box border (own design)
const BOX_BORDER: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
