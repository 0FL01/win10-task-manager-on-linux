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

//! Win32-style building blocks of dialogs and the settings window (own design).

use egui::{Align, Layout, Response, RichText, Sense, Stroke, Ui, Vec2, pos2};

use super::{palette as pal, style};

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
        Color::BOX
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

struct Color;
impl Color {
    const BOX: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
}

/// Radio buttons in a row; returns `true` when the selection changed.
pub fn toggle_group(ui: &mut Ui, options: &[(&str, &str)], selected: &mut usize) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for (i, (label, tooltip)) in options.iter().enumerate() {
            if ui
                .radio(*selected == i, *label)
                .on_hover_text(*tooltip)
                .clicked()
                && *selected != i
            {
                *selected = i;
                changed = true;
            }
        }
    });
    changed
}

/// Settings row: title (and subtitle) on the left, control on the right.
pub fn row(ui: &mut Ui, title: &str, subtitle: Option<&str>, add_control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_width(ui.available_width());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.;
            ui.add(egui::Label::new(title).wrap());
            if let Some(subtitle) = subtitle {
                ui.add(
                    egui::Label::new(
                        RichText::new(subtitle)
                            .text_style(style::caption())
                            .color(pal::LABEL),
                    )
                    .wrap(),
                );
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), add_control);
    });
}

/// Win32 group box: a titled 1 px frame around `add_rows`.
pub fn group(
    ui: &mut Ui,
    title: Option<&str>,
    description: Option<&str>,
    add_rows: impl FnOnce(&mut Ui),
) {
    if let Some(title) = title {
        ui.add_space(4.);
        ui.label(RichText::new(title).color(pal::GROUP_TEXT));
    }
    if let Some(description) = description {
        caption(ui, description);
    }
    egui::Frame::new()
        .stroke(Stroke::new(1., pal::TAB_BORDER))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.;
            add_rows(ui);
        });
    ui.add_space(8.);
}
