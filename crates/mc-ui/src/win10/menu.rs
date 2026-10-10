/* crates/mc-ui/src/win10/menu.rs
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

//! Win32-style drop-down menus (own design; the references show no open menu).

use egui::containers::menu::MenuConfig;
use egui::containers::menu::SubMenu;
use egui::{
    Align, Frame, Id, Margin, Popup, PopupAnchor, PopupCloseBehavior, PopupKind, Pos2, Response,
    Sense, Shadow, Stroke, Style, Ui, UiKind, UiStackInfo, pos2, vec2,
};

use super::paint::{self, Font};
use super::{metrics as m, palette as pal};

pub fn frame() -> Frame {
    Frame::new()
        .fill(pal::POPUP_BG)
        .stroke(Stroke::new(1., pal::POPUP_BORDER))
        .inner_margin(Margin::same(2))
        .corner_radius(0.)
        .shadow(Shadow {
            offset: [2, 2],
            blur: 4,
            spread: 0,
            color: egui::Color32::from_black_alpha(48),
        })
}

pub fn style(style: &mut Style) {
    style.spacing.item_spacing = vec2(0., 0.);
    style.spacing.menu_margin = Margin::same(2);
    style.visuals.menu_corner_radius = 0.into();
    style.visuals.window_fill = pal::POPUP_BG;
    style.visuals.window_stroke = Stroke::new(1., pal::POPUP_BORDER);
    style.visuals.popup_shadow = frame().shadow;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    Check,
    Radio,
}

fn row(
    ui: &mut Ui,
    label: &str,
    shortcut: &str,
    mark: Mark,
    enabled: bool,
    submenu: bool,
) -> Response {
    let width = ui.available_width().max(m::POPUP_MIN_W);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, m::POPUP_ITEM_H), sense);
    let painter = ui.painter();
    let highlighted = enabled && (response.hovered() || response.has_focus());
    if highlighted {
        painter.rect_filled(rect, 0., pal::POPUP_HOVER);
    }
    let ink = if enabled {
        pal::TEXT
    } else {
        pal::DISABLED_TEXT
    };
    let cap_top = (rect.top() + (m::POPUP_ITEM_H - 9.) / 2.).round();
    paint::text(
        painter,
        rect.left() + m::POPUP_TEXT_X,
        cap_top,
        label,
        Font::Body,
        ink,
        Align::Min,
    );
    if !shortcut.is_empty() {
        paint::text(
            painter,
            rect.right() - 24.,
            cap_top,
            shortcut,
            Font::Body,
            ink,
            Align::Max,
        );
    }
    let cx = rect.left() + 13.;
    let cy = rect.center().y;
    match mark {
        Mark::None => {}
        Mark::Check => {
            painter.line(
                vec![
                    pos2(cx - 4., cy),
                    pos2(cx - 1., cy + 3.),
                    pos2(cx + 5., cy - 4.),
                ],
                Stroke::new(1.3, ink),
            );
        }
        Mark::Radio => {
            painter.circle_filled(pos2(cx, cy), 3., ink);
        }
    }
    if submenu {
        let x = rect.right() - 12.;
        painter.line(
            vec![pos2(x, cy - 4.), pos2(x + 4., cy), pos2(x, cy + 4.)],
            Stroke::new(1., ink),
        );
    }
    response
}

/// Plain menu item; closes the menu when clicked.
pub fn item(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
    row(ui, label, shortcut, Mark::None, enabled, false).clicked()
}

/// Item with a check mark.
pub fn check(ui: &mut Ui, label: &str, checked: bool) -> bool {
    let mark = if checked { Mark::Check } else { Mark::None };
    row(ui, label, "", mark, true, false).clicked()
}

/// Item of a radio group.
pub fn radio(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let mark = if selected { Mark::Radio } else { Mark::None };
    row(ui, label, "", mark, true, false).clicked()
}

pub fn separator(ui: &mut Ui) {
    let width = ui.available_width().max(m::POPUP_MIN_W);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 7.), Sense::hover());
    let y = rect.top() + 3.;
    paint::hline(
        ui.painter(),
        rect.left() + 28.,
        rect.right() - 1.,
        y,
        pal::POPUP_SEPARATOR,
    );
}

/// Item opening a submenu on hover.
pub fn submenu(ui: &mut Ui, label: &str, content: impl FnOnce(&mut Ui)) {
    let response = row(ui, label, "", Mark::None, true, true);
    SubMenu::new().show(ui, &response, |ui| {
        ui.set_min_width(m::POPUP_MIN_W);
        content(ui)
    });
}

/// Context menu at `pos`; `open` turns false when it closes.
pub fn context(ui: &Ui, id: Id, pos: Pos2, open: &mut bool, content: impl FnOnce(&mut Ui)) {
    Popup::new(
        id,
        ui.ctx().clone(),
        PopupAnchor::Position(pos),
        ui.layer_id(),
    )
    .kind(PopupKind::Menu)
    .layout(egui::Layout::top_down_justified(Align::Min))
    .open_bool(open)
    .close_behavior(PopupCloseBehavior::CloseOnClick)
    .frame(frame())
    .style(style)
    .info(
        UiStackInfo::new(UiKind::Menu)
            .with_tag_value(MenuConfig::MENU_CONFIG_TAG, MenuConfig::new()),
    )
    .show(|ui| {
        ui.set_min_width(m::POPUP_MIN_W);
        content(ui)
    });
}
