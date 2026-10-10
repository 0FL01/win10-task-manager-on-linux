/* crates/mc-ui/src/win10/style.rs
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

//! egui style for standard widgets (dialogs, settings): Win32 look, own values where the
//! references show nothing.

use egui::{Color32, CornerRadius, FontFamily, FontId, Shadow, Stroke, TextStyle, Visuals};

use super::{fonts, palette as pal};

/// Win32 dialog background
pub const DIALOG_BG: Color32 = Color32::from_rgb(0xf0, 0xf0, 0xf0);

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut visuals = Visuals::light();
    visuals.panel_fill = pal::WHITE;
    visuals.window_fill = DIALOG_BG;
    visuals.extreme_bg_color = pal::WHITE;
    visuals.faint_bg_color = Color32::from_rgb(0xf7, 0xf7, 0xf7);
    visuals.window_stroke = Stroke::new(1., pal::WINDOW_BORDER);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals.window_shadow = Shadow {
        offset: [0, 2],
        blur: 10,
        spread: 0,
        color: Color32::from_black_alpha(50),
    };
    visuals.popup_shadow = super::menu::frame().shadow;
    visuals.selection.bg_fill = pal::BUTTON_HOVER_BORDER;
    visuals.selection.stroke = Stroke::new(1., pal::WHITE);
    visuals.hyperlink_color = pal::LINK;
    let w = &mut visuals.widgets;
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = CornerRadius::ZERO;
        state.fg_stroke = Stroke::new(1., pal::TEXT);
        state.expansion = 0.;
    }
    w.noninteractive.bg_fill = DIALOG_BG;
    w.noninteractive.weak_bg_fill = DIALOG_BG;
    w.noninteractive.bg_stroke = Stroke::new(1., pal::TAB_BORDER);
    w.inactive.weak_bg_fill = pal::BUTTON_FILL;
    w.inactive.bg_fill = pal::WHITE;
    w.inactive.bg_stroke = Stroke::new(1., pal::BUTTON_BORDER);
    w.hovered.weak_bg_fill = pal::BUTTON_HOVER_FILL;
    w.hovered.bg_fill = pal::WHITE;
    w.hovered.bg_stroke = Stroke::new(1., pal::BUTTON_HOVER_BORDER);
    w.active.weak_bg_fill = pal::BUTTON_PRESSED_FILL;
    w.active.bg_fill = pal::BUTTON_PRESSED_FILL;
    w.active.bg_stroke = Stroke::new(1., pal::BUTTON_PRESSED_BORDER);
    w.open = w.hovered;
    ctx.set_visuals_of(egui::Theme::Light, visuals);

    ctx.all_styles_mut(|style| {
        let semibold = fonts::family(fonts::SEMIBOLD);
        style.text_styles = [
            (TextStyle::Small, FontId::new(11., FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(12., FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(12., FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(15., FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12., FontFamily::Monospace),
            ),
            (title1(), FontId::new(20., fonts::family(fonts::LIGHT))),
            (title3(), FontId::new(15., semibold.clone())),
            (title4(), FontId::new(13., semibold)),
            (caption(), FontId::new(11., FontFamily::Proportional)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(7., 7.);
        style.spacing.button_padding = egui::vec2(10., 3.);
        style.spacing.interact_size.y = 23.;
        style.spacing.window_margin = egui::Margin::same(11);
        style.interaction.tooltip_delay = 0.6;
    });
}

pub fn title1() -> TextStyle {
    TextStyle::Name("title-1".into())
}

pub fn title3() -> TextStyle {
    TextStyle::Name("title-3".into())
}

pub fn title4() -> TextStyle {
    TextStyle::Name("title-4".into())
}

pub fn caption() -> TextStyle {
    TextStyle::Name("caption".into())
}
