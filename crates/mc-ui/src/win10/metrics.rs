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

/// Smallest full window at which the Performance page still lays out without overlaps
pub const MIN_WINDOW_SIZE: [f32; 2] = [760., 540.];

pub fn min_window_size_i32() -> [i32; 2] {
    [MIN_WINDOW_SIZE[0] as i32, MIN_WINDOW_SIZE[1] as i32]
}
/// own: "Fewer details" window
pub const COMPACT_WINDOW_SIZE: [f32; 2] = [400., 380.];
pub const COMPACT_MIN_SIZE: [f32; 2] = [300., 240.];
/// Own GPU table adaptation: keep default Processes columns usable at the minimum width.
pub const GPU_ENGINE_W: f32 = 140.;
pub const GPU_DROPDOWN_W: f32 = 7.;
pub const GPU_DROPDOWN_H: usize = 4;
pub const GPU_DROPDOWN_GAP: f32 = 4.;
pub const GPU_DROPDOWN_DY: f32 = 7.;
pub const GPU_MEMORY_W: f32 = 180.;
pub const TABLE_HORIZONTAL_STEP: f32 = 72.;
/// Width of the invisible resize zone along the window edges.
pub const RESIZE_GRIP: f32 = 4.;
/// Own: extend corner targets along the border without covering caption buttons/content.
pub const RESIZE_CORNER: f32 = 16.;

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
/// Space after the label (room for the submenu chevron) and between label and shortcut
pub const POPUP_TEXT_RIGHT_PAD: f32 = 40.;
pub const POPUP_SHORTCUT_GAP: f32 = 32.;

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

/// Inside our 1 px window border (the reference's 15 px include the frame column)
pub const SCROLLBAR_W: f32 = 14.;
pub const SCROLL_ARROW_H: f32 = 17.;
pub const SCROLL_THUMB_MIN: f32 = 17.;

/// GPU page own adaptation: engine tile captions, separate memory panels and vertically flowing
/// metadata. These extend the measured Performance typography/grid, rather than measured GPU pixels.
pub const GPU_TILE_CAPTION_H: f32 = 18.;
pub const GPU_ENGINE_MIN_H: f32 = 84.;
pub const GPU_MEMORY_MIN_H: f32 = 52.;
pub const GPU_AUX_MIN_H: f32 = 52.;
pub const GPU_PANEL_GAP: f32 = 24.;
pub const GPU_INFO_GAP: f32 = 24.;
/// Legend bar left of a statistic that names a graph series: 2 px wide, 40 px from the label's
/// cap top, text origin 8 px right of the column (ink at +9); dashed series as 3 px dashes with 3 px gaps
pub const STAT_LEGEND_W: f32 = 2.;
pub const STAT_LEGEND_H: f32 = 40.;
pub const STAT_LEGEND_TEXT_DX: f32 = 8.;
pub const STAT_LEGEND_DASH: f32 = 3.;
pub const GPU_COMPACT_INSET: f32 = 12.;

/// Own Unix Performance adaptation: thin disk transfer strip and memory composition.
pub const DISK_TRANSFER_H: f32 = 66.;
pub const MEMORY_COMPOSITION_H: f32 = 52.;
pub const MEMORY_GRAPH_MIN_H: f32 = 52.;

/// Own metadata adaptation, shared by System and the overflow popup.
pub const METADATA_VALUE_GAP: f32 = 12.;
pub const METADATA_MORE_W: f32 = 100.;
pub const METADATA_MORE_H: f32 = 17.;
pub const METADATA_POPUP_MAX_H: f32 = 360.;
pub const METADATA_COPY_MS: u64 = 650;

/// Own DIMM/SO-DIMM diagram alongside the Memory statistics.
pub const MEMORY_SLOT_W: f32 = 52.;
pub const MEMORY_SLOT_LABEL_H: f32 = 38.;
pub const MEMORY_SLOT_NAME_DY: f32 = 7.;
pub const MEMORY_SLOT_CAPACITY_DY: f32 = 23.;
pub const MEMORY_SLOT_FOOTER_RESERVE: f32 = 14.;
pub const MEMORY_SLOT_GAP: f32 = 10.;
pub const MEMORY_SLOT_HEADER_H: f32 = 18.;
pub const MEMORY_SLOT_DRAW_W: f32 = 28.;
pub const MEMORY_SLOT_INFO_GAP: f32 = 16.;
pub const MEMORY_MODULE_DIALOG_W: f32 = 500.;
pub const SPD_SCROLL_MAX_H: f32 = 400.;
/// Own design: swap usage bar and the zram/zswap rows below it (key/value row pitch).
pub const MEMORY_SWAP_BAR_H: f32 = 20.;
pub const MEMORY_SWAP_ROW_H: f32 = 18.;
pub const MEMORY_SWAP_ROW_CAP: f32 = 6.;

/// Own consolidated-battery layout and original outline glyphs (not measured Win10 assets).
pub const BATTERY_CARD_MIN_W: f32 = 224.;
pub const BATTERY_CARD_H: f32 = 104.;
pub const BATTERY_CARD_GAP: f32 = 12.;
pub const BATTERY_CARD_PAD: f32 = 12.;
pub const BATTERY_CARD_NAME_GAP: f32 = 10.;
pub const BATTERY_CARD_NAME_DY: f32 = 15.;
pub const BATTERY_CARD_PERCENT_DY: f32 = 42.;
pub const BATTERY_CARD_STATE_DY: f32 = 48.;
pub const BATTERY_CARD_BAR_DY: f32 = 80.;
pub const BATTERY_CARD_BAR_H: f32 = 6.;
pub const BATTERY_SECTION_GAP: f32 = 24.;
pub const BATTERY_GRAPH_MIN_H: f32 = 160.;
pub const BATTERY_GRAPH_MAX_H: f32 = 320.;
pub const BATTERY_DETAIL_GRAPH_H: f32 = 120.;
pub const BATTERY_DETAIL_HEADING_H: f32 = 36.;
pub const BATTERY_GLYPH_SIZE: f32 = 16.;
pub const BATTERY_SIDEBAR_SIZE: [f32; 2] = [40., 22.];
pub const BATTERY_SIDEBAR_OUTLINE: &[([f32; 2], [f32; 2])] = &[
    ([0., 0.], [35., 0.]),
    ([0., 21.], [35., 21.]),
    ([0., 1.], [0., 20.]),
    ([35., 1.], [35., 6.]),
    ([35., 7.], [39., 7.]),
    ([39., 8.], [39., 13.]),
    ([35., 14.], [39., 14.]),
    ([35., 15.], [35., 20.]),
];
pub const BATTERY_GLYPH: &[([f32; 2], [f32; 2])] = &[
    ([1., 4.], [12., 4.]),
    ([1., 11.], [12., 11.]),
    ([1., 5.], [1., 10.]),
    ([12., 5.], [12., 6.]),
    ([12., 6.], [14., 6.]),
    ([14., 6.], [14., 9.]),
    ([12., 9.], [14., 9.]),
    ([12., 9.], [12., 10.]),
];
pub const BATTERY_MOUSE_GLYPH: &[([f32; 2], [f32; 2])] = &[
    ([6., 1.], [9., 1.]),
    ([9., 1.], [12., 4.]),
    ([12., 4.], [12., 11.]),
    ([12., 11.], [9., 14.]),
    ([9., 14.], [6., 14.]),
    ([6., 14.], [3., 11.]),
    ([3., 11.], [3., 4.]),
    ([3., 4.], [6., 1.]),
    ([3., 6.], [12., 6.]),
    ([8., 2.], [8., 5.]),
];
pub const BATTERY_HEADPHONES_GLYPH: &[([f32; 2], [f32; 2])] = &[
    ([2., 8.], [2., 6.]),
    ([2., 6.], [4., 2.]),
    ([4., 2.], [6., 1.]),
    ([6., 1.], [9., 1.]),
    ([9., 1.], [11., 2.]),
    ([11., 2.], [13., 6.]),
    ([13., 6.], [13., 8.]),
    ([1., 7.], [4., 7.]),
    ([4., 7.], [4., 13.]),
    ([4., 13.], [1., 13.]),
    ([1., 13.], [1., 7.]),
    ([11., 7.], [14., 7.]),
    ([14., 7.], [14., 13.]),
    ([14., 13.], [11., 13.]),
    ([11., 13.], [11., 7.]),
];
pub const BATTERY_GENERIC_GLYPH: &[([f32; 2], [f32; 2])] = &[
    ([3., 1.], [12., 1.]),
    ([12., 1.], [12., 14.]),
    ([12., 14.], [3., 14.]),
    ([3., 14.], [3., 1.]),
    ([6., 11.], [9., 11.]),
];
