/* crates/mc-ui/src/win10/chrome.rs
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

//! Window chrome: client-side title bar with caption buttons, resize edges, menu bar, tab strip
//! and footer (docs/win10-spec/spec.md, "Window chrome", "Tab strip", "Content frame").

use egui::containers::menu::MenuConfig;
use egui::{
    Align, CursorIcon, Id, Popup, PopupCloseBehavior, Rect, ResizeDirection, Response, Sense, Ui,
    UiKind, UiStackInfo, ViewportCommand, pos2,
};

use super::paint::{self, Font};
use super::{menu, metrics as m, palette as pal};

/// Paints the window background and its 1 px outline (no outline while maximized).
pub fn window_background(ui: &Ui, window: Rect, outlined: bool) {
    let painter = ui.painter();
    painter.rect_filled(window, 0., pal::resolve(ui.ctx(), pal::WHITE));
    if outlined {
        paint::outline(
            painter,
            window.left(),
            window.top(),
            window.right() - 1.,
            window.bottom() - 1.,
            pal::WINDOW_BORDER,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caption {
    Minimize,
    Maximize,
    Close,
}

/// Title bar with the window title, drag-to-move, double-click maximize and caption buttons.
pub fn title_bar(ui: &mut Ui, window: Rect, title: &str, maximized: bool) {
    let painter = ui.painter().clone();
    paint::text(
        &painter,
        window.left() + m::TITLE_TEXT_X,
        window.top() + m::TITLE_CAP_TOP,
        title,
        Font::Body,
        pal::TEXT,
        Align::Min,
    );

    let top = window.top() + m::TITLE_TOP;
    let bottom = window.top() + m::TITLE_BOTTOM;
    let mut x1 = window.right() - 1. - m::CAPTION_RIGHT_INSET;
    let mut buttons_left = x1;
    for caption in [Caption::Close, Caption::Maximize, Caption::Minimize] {
        let x0 = x1 - m::CAPTION_BUTTON_W + 1.;
        let rect = Rect::from_min_max(pos2(x0, top), pos2(x1 + 1., bottom + 1.));
        let response = ui.interact(
            rect,
            ui.id().with(("caption", caption as u8)),
            Sense::click(),
        );
        let pressed = response.is_pointer_button_down_on();
        let hovered = response.hovered();
        let close = caption == Caption::Close;
        let bg = match (close, pressed, hovered) {
            (false, true, _) => Some(pal::CAPTION_PRESSED),
            (false, false, true) => Some(pal::CAPTION_HOVER),
            (true, true, _) => Some(pal::CLOSE_PRESSED),
            (true, false, true) => Some(pal::CLOSE_HOVER),
            _ => None,
        };
        if let Some(bg) = bg {
            paint::fill(&painter, x0, top, x1, bottom, bg);
        }
        caption_glyph(
            &painter,
            caption,
            x0,
            window.top(),
            maximized,
            hovered || pressed,
        );
        if response.clicked() {
            ui.ctx().send_viewport_cmd(match caption {
                Caption::Minimize => ViewportCommand::Minimized(true),
                Caption::Maximize => ViewportCommand::Maximized(!maximized),
                Caption::Close => ViewportCommand::Close,
            });
        }
        buttons_left = x0;
        x1 = x0 - 1.;
    }

    let drag = Rect::from_min_max(
        pos2(window.left() + 1., top),
        pos2(buttons_left, bottom + 1.),
    );
    let response = ui.interact(drag, ui.id().with("title-drag"), Sense::click_and_drag());
    if response.double_clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if response.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

fn caption_glyph(
    painter: &egui::Painter,
    caption: Caption,
    x0: f32,
    top: f32,
    maximized: bool,
    highlighted: bool,
) {
    let gx = x0 + 20.;
    let gy = top + 11.;
    let ink = pal::TEXT;
    match caption {
        Caption::Minimize => paint::hline(painter, gx + 1., gx + 10., top + 16., ink),
        Caption::Maximize if maximized => {
            // own: restore glyph, two overlapping squares
            paint::outline(painter, gx, gy + 2., gx + 7., gy + 9., ink);
            paint::hline(painter, gx + 2., gx + 9., gy, ink);
            paint::vline(painter, gx + 9., gy, gy + 7., ink);
            paint::vline(painter, gx + 2., gy, gy + 1., ink);
            paint::hline(painter, gx + 8., gx + 8., gy + 7., ink);
        }
        Caption::Maximize => paint::outline(painter, gx, gy, gx + 9., gy + 9., ink),
        Caption::Close => {
            // Own ink role: white on the red hover/pressed surface in either theme. WHITE is
            // otherwise a surface token, so sending it through palette resolution darkens it.
            let ink = if highlighted {
                egui::Color32::WHITE
            } else {
                pal::resolve(painter.ctx(), pal::TEXT)
            };
            paint::line_ink(painter, (gx, gy), (gx + 9., gy + 9.), ink);
            paint::line_ink(painter, (gx + 9., gy), (gx, gy + 9.), ink);
        }
    }
}

/// Invisible resize zones along the window edges (only for a restored window).
pub fn resize_edges(ui: &mut Ui, window: Rect) {
    let g = m::RESIZE_GRIP;
    let c = m::RESIZE_CORNER;
    let (l, t, r, b) = (window.left(), window.top(), window.right(), window.bottom());
    let zones = [
        (
            Rect::from_min_max(pos2(l, t), pos2(l + c, t + g)),
            ResizeDirection::NorthWest,
        ),
        (
            Rect::from_min_max(pos2(r - c, t), pos2(r, t + g)),
            ResizeDirection::NorthEast,
        ),
        (
            Rect::from_min_max(pos2(l, b - g), pos2(l + c, b)),
            ResizeDirection::SouthWest,
        ),
        (
            Rect::from_min_max(pos2(r - c, b - g), pos2(r, b)),
            ResizeDirection::SouthEast,
        ),
        (
            Rect::from_min_max(pos2(l, t + g), pos2(l + g, t + c)),
            ResizeDirection::NorthWest,
        ),
        (
            Rect::from_min_max(pos2(r - g, t + g), pos2(r, t + c)),
            ResizeDirection::NorthEast,
        ),
        (
            Rect::from_min_max(pos2(l, b - c), pos2(l + g, b - g)),
            ResizeDirection::SouthWest,
        ),
        (
            Rect::from_min_max(pos2(r - g, b - c), pos2(r, b - g)),
            ResizeDirection::SouthEast,
        ),
        (
            Rect::from_min_max(pos2(l + c, t), pos2(r - c, t + g)),
            ResizeDirection::North,
        ),
        (
            Rect::from_min_max(pos2(l + c, b - g), pos2(r - c, b)),
            ResizeDirection::South,
        ),
        (
            Rect::from_min_max(pos2(l, t + c), pos2(l + g, b - c)),
            ResizeDirection::West,
        ),
        (
            Rect::from_min_max(pos2(r - g, t + c), pos2(r, b - c)),
            ResizeDirection::East,
        ),
    ];
    let pressed_direction = ui.input(|input| {
        // Hover may have moved or left since the press. Require an actual Primary press:
        // drag_started_by only checks which button is currently held, not which started it.
        let pos = input.events.iter().rev().find_map(|event| match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } => Some(*pos),
            _ => None,
        })?;
        // Shared bounds belong to the last registered zone, matching egui hit testing.
        zones
            .iter()
            .rev()
            .find_map(|(rect, direction)| rect.contains(pos).then_some(*direction))
    });
    let mut drag_started = false;
    for (i, (rect, direction)) in zones.into_iter().enumerate() {
        let response = ui.interact(rect, ui.id().with(("resize", i)), Sense::drag());
        let cursor = match direction {
            ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
            ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
        };
        // Egui's proximity hit testing must not advertise a resize outside the border.
        let on_border = ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|pos| rect.contains(pos))
        });
        if response.hovered() && on_border {
            ui.ctx().set_cursor_icon(cursor);
        }
        drag_started |= response.drag_started_by(egui::PointerButton::Primary);
    }
    if drag_started && let Some(direction) = pressed_direction {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::BeginResize(direction));
        // The native compositor owns this gesture now, including its button release.
        // Keeping an egui drag would lock the cursor and suppress the next resize when
        // Wayland sends PointerGone instead of forwarding that release to the client.
        ui.ctx().stop_dragging();
        // stop_dragging clears widget ownership, not the held button. Without clearing
        // that too, hover remains suppressed until some later client-side release.
        ui.ctx()
            .input_mut(|input| input.pointer = egui::PointerState::default());
    }
}

/// Menu bar; `content(ui, index)` fills the drop-down of item `index`.
pub fn menu_bar(
    ui: &mut Ui,
    window: Rect,
    labels: &[String],
    mut content: impl FnMut(&mut Ui, usize),
) {
    let painter = ui.painter().clone();
    let ids: Vec<Id> = (0..labels.len())
        .map(|i| ui.id().with(("menubar", i)))
        .collect();
    let any_open = ids
        .iter()
        .any(|id| Popup::is_id_open(ui.ctx(), id.with("popup")));

    let mut text_x = window.left() + m::MENU_FIRST_TEXT_X;
    for (i, label) in labels.iter().enumerate() {
        let width = paint::text_width(&painter, label, Font::Body).round();
        let rect = Rect::from_min_max(
            pos2(text_x - m::MENU_ITEM_PAD, window.top() + m::MENU_TOP),
            pos2(
                text_x + width + m::MENU_ITEM_PAD,
                window.top() + m::MENU_BOTTOM + 1.,
            ),
        );
        let response = ui.interact(rect, ids[i], Sense::click());
        let popup_id = Popup::default_response_id(&response);
        let mut open = Popup::is_id_open(ui.ctx(), popup_id);
        if any_open && !open && response.hovered() {
            Popup::open_id(ui.ctx(), popup_id);
            open = true;
        }
        if open || response.hovered() {
            paint::fill(
                &painter,
                rect.left(),
                rect.top(),
                rect.right() - 1.,
                rect.bottom() - 1.,
                pal::MENU_ITEM_HOVER,
            );
            paint::outline(
                &painter,
                rect.left(),
                rect.top(),
                rect.right() - 1.,
                rect.bottom() - 1.,
                pal::MENU_ITEM_HOVER_BORDER,
            );
        }
        paint::text(
            &painter,
            text_x,
            window.top() + m::MENU_CAP_TOP,
            label,
            Font::Body,
            pal::TEXT,
            Align::Min,
        );
        Popup::menu(&response)
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .frame(menu::themed_frame(ui.ctx()))
            .style(menu::style)
            .info(
                UiStackInfo::new(UiKind::Menu)
                    .with_tag_value(MenuConfig::MENU_CONFIG_TAG, MenuConfig::new()),
            )
            .show(|ui| {
                ui.set_min_width(m::POPUP_MIN_W);
                content(ui, i)
            });
        text_x = rect.right() + m::MENU_ITEM_PAD;
    }

    paint::hline(
        &painter,
        window.left() + 1.,
        window.right() - 2.,
        window.top() + m::MENU_LINE_1_Y,
        pal::MENU_LINE_1,
    );
    paint::hline(
        &painter,
        window.left() + 1.,
        window.right() - 2.,
        window.top() + m::MENU_LINE_2_Y,
        pal::MENU_LINE_2,
    );
}

/// Tab strip; returns the index of a newly pressed (or keyboard-activated) tab.
pub fn tabs(ui: &mut Ui, window: Rect, labels: &[String], selected: usize) -> Option<usize> {
    let painter = ui.painter().clone();
    let top = window.top();
    let line_y = top + m::TAB_BOTTOM_LINE_Y;
    paint::hline(
        &painter,
        window.left() + 1.,
        window.right() - 2.,
        line_y,
        pal::TAB_BORDER,
    );

    // Natural (unselected) boundaries; adjacent tabs share their border column
    let mut bounds = vec![window.left() + m::TAB_FIRST_LEFT];
    for label in labels {
        let width = paint::text_width(&painter, label, Font::Body).round() + m::TAB_EXTRA_W;
        bounds.push(bounds.last().copied().unwrap_or_default() + width);
    }

    let tab_rect = |i: usize| {
        Rect::from_min_max(
            pos2(bounds[i], top + m::TAB_TOP),
            pos2(bounds[i + 1] + 1., line_y),
        )
    };
    // own: like the Win32 tab control, a tab activates on the primary press, and the strip is
    // painted with the new selection in the same pass, so rapid switching follows the mouse
    let pressed = if ui.input(|i| i.pointer.primary_pressed()) {
        (0..labels.len()).find(|&i| i != selected && ui.rect_contains_pointer(tab_rect(i)))
    } else {
        None
    };
    let selected = pressed.unwrap_or(selected);

    let mut clicked = pressed;
    for (i, label) in labels.iter().enumerate() {
        if i == selected {
            continue;
        }
        let (x0, x1) = (bounds[i], bounds[i + 1]);
        let response = ui.interact(tab_rect(i), ui.id().with(("tab", i)), Sense::click());
        // own: hover tint
        let fill = if response.hovered() {
            pal::HOVER
        } else {
            pal::TAB_INACTIVE
        };
        paint::fill(
            &painter,
            x0 + 1.,
            top + m::TAB_TOP + 1.,
            x1 - 1.,
            line_y - 1.,
            fill,
        );
        paint::hline(&painter, x0, x1, top + m::TAB_TOP, pal::TAB_BORDER);
        paint::vline(&painter, x0, top + m::TAB_TOP, line_y, pal::TAB_BORDER);
        paint::vline(&painter, x1, top + m::TAB_TOP, line_y, pal::TAB_BORDER);
        paint::text(
            &painter,
            x0 + m::TAB_LABEL_X,
            top + m::TAB_CAP_TOP,
            label,
            Font::Body,
            pal::TEXT,
            Align::Min,
        );
        if response.clicked() {
            clicked = Some(i);
        }
    }

    if let Some(label) = labels.get(selected) {
        let x0 = (bounds[selected] - m::TAB_SELECTED_GROW).max(window.left() + 1.);
        let x1 = bounds[selected + 1] + m::TAB_SELECTED_GROW;
        let y0 = top + m::TAB_SELECTED_TOP;
        paint::fill(&painter, x0 + 1., y0 + 1., x1 - 1., line_y, pal::WHITE);
        paint::hline(&painter, x0, x1, y0, pal::TAB_BORDER);
        paint::vline(&painter, x0, y0, line_y, pal::TAB_BORDER);
        paint::vline(&painter, x1, y0, line_y, pal::TAB_BORDER);
        paint::text(
            &painter,
            bounds[selected] + m::TAB_LABEL_X,
            top + m::TAB_SELECTED_CAP_TOP,
            label,
            Font::Body,
            pal::TEXT,
            Align::Min,
        );
    }
    clicked
}

/// Footer boundary; optionally draws the separator and always returns its y.
pub fn footer_line(ui: &Ui, window: Rect, draw_separator: bool) -> f32 {
    let y = window.bottom() - m::FOOTER_H;
    if draw_separator {
        paint::hline(
            ui.painter(),
            window.left() + 1.,
            window.right() - 2.,
            y,
            pal::LIST_FRAME,
        );
    }
    y
}

/// "Fewer details" / "More details" toggle at the left of the footer.
pub fn details_toggle(
    ui: &mut Ui,
    window: Rect,
    line_y: f32,
    label: &str,
    expanded: bool,
) -> Response {
    let painter = ui.painter().clone();
    let x = window.left() + m::FOOTER_ICON_X;
    let y = line_y + m::FOOTER_ICON_DY;
    let width = paint::text_width(&painter, label, Font::Body).round();
    let rect = Rect::from_min_max(
        pos2(x, y),
        pos2(
            window.left() + m::FOOTER_TEXT_X + width + 1.,
            y + m::FOOTER_ICON_SIZE,
        ),
    );
    let response = ui.interact(rect, ui.id().with("details-toggle"), Sense::click());
    let ink = if response.hovered() {
        pal::BUTTON_HOVER_BORDER
    } else {
        pal::FOOTER_ICON
    };
    // own glyph: circle with a chevron (up = fewer details)
    let ink = pal::resolve(ui.ctx(), ink);
    let s = m::FOOTER_ICON_SIZE;
    let c = pos2(x + s / 2., y + s / 2.);
    painter.circle_stroke(c, s / 2. - 0.5, egui::Stroke::new(1., ink));
    let (dy0, dy1) = if expanded { (2., -2.) } else { (-2., 2.) };
    painter.line(
        vec![
            pos2(c.x - 4., c.y + dy0),
            pos2(c.x, c.y + dy1),
            pos2(c.x + 4., c.y + dy0),
        ],
        egui::Stroke::new(1.2, ink),
    );
    paint::text(
        &painter,
        window.left() + m::FOOTER_TEXT_X,
        line_y + m::FOOTER_CAP_DY,
        label,
        Font::Body,
        pal::TEXT,
        Align::Min,
    );
    response
}

/// Win32 push button at the right of the footer ("End task").
pub fn footer_button(
    ui: &mut Ui,
    window: Rect,
    line_y: f32,
    label: &str,
    enabled: bool,
) -> Response {
    let x1 = window.right() - m::FOOTER_BUTTON_RIGHT - 1.;
    let x0 = x1 - m::FOOTER_BUTTON_W + 1.;
    let y0 = line_y + m::FOOTER_BUTTON_DY;
    let rect = Rect::from_min_max(pos2(x0, y0), pos2(x1 + 1., y0 + m::FOOTER_BUTTON_H));
    button(ui, rect, ui.id().with("footer-button"), label, enabled)
}

/// Win32 push button in `rect`.
pub fn button(ui: &mut Ui, rect: Rect, id: Id, label: &str, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let (border, fill, ink) = if !enabled {
        (
            pal::BUTTON_DISABLED_BORDER,
            pal::BUTTON_DISABLED_FILL,
            pal::DISABLED_TEXT,
        )
    } else if response.is_pointer_button_down_on() {
        (
            pal::BUTTON_PRESSED_BORDER,
            pal::BUTTON_PRESSED_FILL,
            pal::TEXT,
        )
    } else if response.hovered() || response.has_focus() {
        (pal::BUTTON_HOVER_BORDER, pal::BUTTON_HOVER_FILL, pal::TEXT)
    } else {
        (pal::BUTTON_BORDER, pal::BUTTON_FILL, pal::TEXT)
    };
    let painter = ui.painter();
    let (x0, y0, x1, y1) = (
        rect.left(),
        rect.top(),
        rect.right() - 1.,
        rect.bottom() - 1.,
    );
    paint::fill(painter, x0 + 1., y0 + 1., x1 - 1., y1 - 1., fill);
    paint::outline(painter, x0, y0, x1, y1, border);
    paint::text(
        painter,
        rect.center().x,
        (rect.center().y - 4.5).round(),
        label,
        Font::Body,
        ink,
        Align::Center,
    );
    response
}
