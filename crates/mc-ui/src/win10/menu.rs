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

/// Width of a menu row. The popup's first (sizing) pass measures the rows' natural widths, so the
/// menu is as wide as its widest item; later passes fill that width. Using the available width in
/// the sizing pass would make every menu as wide as egui's default area (600 px), and submenus
/// that wide no longer fit beside their parent.
fn menu_width(ui: &Ui, natural: f32) -> f32 {
    let natural = natural.max(m::POPUP_MIN_W);
    if ui.is_sizing_pass() {
        natural
    } else {
        ui.available_width().max(natural)
    }
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
    let painter = ui.painter();
    let right = if shortcut.is_empty() {
        m::POPUP_TEXT_RIGHT_PAD
    } else {
        m::POPUP_SHORTCUT_GAP + paint::text_width(painter, shortcut, Font::Body) + 24.
    };
    let natural = m::POPUP_TEXT_X + paint::text_width(painter, label, Font::Body) + right;
    let width = menu_width(ui, natural.ceil());
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
    check_enabled(ui, label, checked, true, None)
}

/// Check item with an optional hover reason, also shown while the row is disabled.
pub fn check_enabled(
    ui: &mut Ui,
    label: &str,
    checked: bool,
    enabled: bool,
    reason: Option<&str>,
) -> bool {
    let mark = if checked { Mark::Check } else { Mark::None };
    let response = row(ui, label, "", mark, enabled, false);
    let response = if let Some(reason) = reason {
        response.on_hover_text(reason)
    } else {
        response
    };
    enabled && response.clicked()
}

/// Item of a radio group.
pub fn radio(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let mark = if selected { Mark::Radio } else { Mark::None };
    row(ui, label, "", mark, true, false).clicked()
}

pub fn separator(ui: &mut Ui) {
    let width = menu_width(ui, m::POPUP_MIN_W);
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

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Order, RawInput, Rect};

    use super::*;

    /// Opens a context menu at `at`, hovers its submenu row and returns the parent menu rect,
    /// the submenu rect and the hovered row's top.
    fn open_submenu(at: Pos2) -> (Rect, Option<Rect>, f32) {
        let ctx = Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1080., 700.));
        let menus = |ctx: &Context| -> Vec<Rect> {
            ctx.memory(|mem| {
                let mut rects: Vec<Rect> = mem
                    .areas()
                    .visible_layer_ids()
                    .into_iter()
                    .filter(|layer| layer.order == Order::Foreground)
                    .filter_map(|layer| mem.area_rect(layer.id))
                    .collect();
                rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
                rects
            })
        };
        let mut pointer = pos2(1., 1.);
        let mut parent = Rect::NOTHING;
        let mut row_top = 0.;
        for frame in 0..16 {
            let input = RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 * 0.1),
                events: vec![Event::PointerMoved(pointer)],
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let mut open = true;
                context(ui, Id::new("test-menu"), at, &mut open, |ui| {
                    item(ui, "Copy", "Ctrl+C", true);
                    submenu(ui, "Change graph to", |ui| {
                        radio(ui, "Overall utilization", true);
                        radio(ui, "Logical processors", false);
                    });
                });
            });
            output.textures_delta.clear();
            if frame == 4 {
                parent = menus(&ctx)
                    .into_iter()
                    .find(|r| r.contains(at + vec2(1., 1.)))
                    .expect("context menu is shown");
                // Second row: frame border + margin, then one item
                row_top = parent.top() + 3. + m::POPUP_ITEM_H;
                pointer = pos2(parent.left() + 60., row_top + m::POPUP_ITEM_H / 2.);
            }
        }
        let submenu = menus(&ctx).into_iter().find(|r| *r != parent);
        (parent, submenu, row_top)
    }

    #[test]
    fn menus_fit_their_content_and_submenus_open_beside_them() {
        let (parent, submenu, row_top) = open_submenu(pos2(300., 200.));
        let submenu = submenu.expect("submenu opens on hover");
        assert!(parent.width() < 300., "content-sized menu: {parent:?}");
        assert!(submenu.width() < 300., "content-sized submenu: {submenu:?}");
        assert!(
            submenu.left() >= parent.right() - 1.,
            "{parent:?} {submenu:?}"
        );
        assert!(
            (submenu.top() - (row_top - 3.)).abs() <= 3.,
            "{submenu:?} row {row_top}"
        );

        // Near the right edge the submenu flips to the left of its parent
        let (parent, submenu, _) = open_submenu(pos2(1080. - 230., 200.));
        let submenu = submenu.expect("submenu opens on hover");
        assert!(
            submenu.right() <= parent.left() + 1.,
            "{parent:?} {submenu:?}"
        );
    }
}
