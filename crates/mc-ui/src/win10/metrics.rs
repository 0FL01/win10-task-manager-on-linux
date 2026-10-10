/* crates/mc-ui/src/win10/metrics.rs
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

//! Geometry of the Win10-style interface in logical points (docs/win10-spec/spec.md). All
//! `y` values are window coordinates; a 1 px border surrounds the window.

/// Reference window size.
pub const WINDOW_SIZE: [f32; 2] = [1030., 705.];
/// Smallest full window at which the Performance page still lays out without overlaps
pub const MIN_WINDOW_SIZE: [f32; 2] = [760., 540.];

pub fn min_window_size_i32() -> [i32; 2] {
    [MIN_WINDOW_SIZE[0] as i32, MIN_WINDOW_SIZE[1] as i32]
}
/// own: "Fewer details" window
pub const COMPACT_WINDOW_SIZE: [f32; 2] = [400., 380.];
pub const COMPACT_MIN_SIZE: [f32; 2] = [300., 240.];
/// Width of the invisible resize zone along the window edges.
pub const RESIZE_GRIP: f32 = 4.;

pub const TITLE_TOP: f32 = 1.;
/// Last row of the title bar (inclusive).
pub const TITLE_BOTTOM: f32 = 30.;
pub const TITLE_TEXT_X: f32 = 9.;
pub const TITLE_CAP_TOP: f32 = 11.;
pub const CAPTION_BUTTON_W: f32 = 46.;
/// Distance of the close button's right edge from the window's right edge.
pub const CAPTION_RIGHT_INSET: f32 = 1.;

pub const MENU_TOP: f32 = 31.;
pub const MENU_BOTTOM: f32 = 49.;
pub const MENU_FIRST_TEXT_X: f32 = 9.;
pub const MENU_CAP_TOP: f32 = 37.;
/// Horizontal padding of a menu bar item (text starts 7 px after the item's left edge)
pub const MENU_ITEM_PAD: f32 = 7.;
pub const MENU_LINE_1_Y: f32 = 50.;
pub const MENU_LINE_2_Y: f32 = 51.;

/// Drop-down menus (own design)
pub const POPUP_ITEM_H: f32 = 22.;
pub const POPUP_TEXT_X: f32 = 33.;
pub const POPUP_MIN_W: f32 = 200.;

pub const TAB_BOTTOM_LINE_Y: f32 = 76.;
pub const TAB_FIRST_LEFT: f32 = 3.;
pub const TAB_TOP: f32 = 56.;
pub const TAB_SELECTED_TOP: f32 = 54.;
pub const TAB_SELECTED_GROW: f32 = 2.;
pub const TAB_LABEL_X: f32 = 7.;
pub const TAB_EXTRA_W: f32 = 12.;
pub const TAB_CAP_TOP: f32 = 62.;
pub const TAB_SELECTED_CAP_TOP: f32 = 60.;

pub const CONTENT_TOP: f32 = 77.;

/// Footer: the line sits `FOOTER_H` points above the window bottom edge (y 655 at 705)
pub const FOOTER_H: f32 = 50.;
pub const FOOTER_ICON_X: f32 = 9.;
/// Offsets inside the footer, relative to the footer line
pub const FOOTER_ICON_DY: f32 = 15.;
pub const FOOTER_ICON_SIZE: f32 = 19.;
pub const FOOTER_TEXT_X: f32 = 36.;
pub const FOOTER_CAP_DY: f32 = 20.;
pub const FOOTER_BUTTON_W: f32 = 83.;
pub const FOOTER_BUTTON_H: f32 = 21.;
pub const FOOTER_BUTTON_DY: f32 = 14.;
/// Gap between the button's right edge and the window's right edge
pub const FOOTER_BUTTON_RIGHT: f32 = 8.;
pub const FOOTER_LINK_ICON_X: f32 = 123.;
pub const FOOTER_LINK_TEXT_X: f32 = 142.;

/// Inside our 1 px window border (the reference's 15 px include the frame column)
pub const SCROLLBAR_W: f32 = 14.;
pub const SCROLL_ARROW_H: f32 = 17.;
pub const SCROLL_THUMB_MIN: f32 = 17.;
