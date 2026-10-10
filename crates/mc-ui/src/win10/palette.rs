/* crates/mc-ui/src/win10/palette.rs
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

//! Colours of the Win10-style interface (docs/win10-spec/spec.md, "Palette"). Values marked
//! "own" are our design where the references show nothing.

use egui::Color32;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const WHITE: Color32 = rgb(0xffffff);
pub const TEXT: Color32 = rgb(0x000000);
/// own: window outline
pub const WINDOW_BORDER: Color32 = rgb(0x7a7a7a);

pub const MENU_LINE_1: Color32 = rgb(0xf2f2f2);
pub const MENU_LINE_2: Color32 = rgb(0xf0f0f0);
/// own: menu bar item hover / open
pub const MENU_ITEM_HOVER: Color32 = rgb(0xe5f3ff);
pub const MENU_ITEM_HOVER_BORDER: Color32 = rgb(0xcce8ff);
/// own: drop-down menus
pub const POPUP_BG: Color32 = rgb(0xf2f2f2);
pub const POPUP_BORDER: Color32 = rgb(0xcccccc);
pub const POPUP_HOVER: Color32 = rgb(0x91c9f7);
pub const POPUP_SEPARATOR: Color32 = rgb(0xd7d7d7);
pub const DISABLED_TEXT: Color32 = rgb(0x838383);

/// own: caption buttons
pub const CAPTION_HOVER: Color32 = rgb(0xe5e5e5);
pub const CAPTION_PRESSED: Color32 = rgb(0xcccccc);
pub const CLOSE_HOVER: Color32 = rgb(0xe81123);
pub const CLOSE_PRESSED: Color32 = rgb(0xf1707a);

pub const TAB_BORDER: Color32 = rgb(0xd9d9d9);
pub const TAB_INACTIVE: Color32 = rgb(0xf0f0f0);

pub const LIST_FRAME: Color32 = rgb(0xa0a0a0);
pub const HEADER_SEPARATOR_TOP: Color32 = rgb(0xe8e8e8);
pub const HEADER_SEPARATOR_BOTTOM: Color32 = rgb(0xbbbbbb);
pub const HEADER_SEPARATOR_FLAT: Color32 = rgb(0xe5e5e5);
pub const HEADER_NAME: Color32 = rgb(0x4c607a);
pub const SORT_CHEVRON: Color32 = rgb(0x6e6091);
pub const SORT_CHEVRON_FLAT: Color32 = rgb(0x595959);
pub const EXPANDER: Color32 = rgb(0xa6a6a6);
/// own: list selection / hover
pub const SELECTION: Color32 = rgb(0xcce8ff);
pub const HOVER: Color32 = rgb(0xe5f3ff);
/// own: group header text ("Apps (n)")
pub const GROUP_TEXT: Color32 = rgb(0x1e3287);
/// own: header summary at ≥ 90 %
pub const SUMMARY_ALERT: Color32 = rgb(0xc42b1c);

pub const HEAT_ZERO: Color32 = rgb(0xfff4c4);
pub const HEAT_LOW: Color32 = rgb(0xf9eca8);
/// own: higher heat levels (≥ 5, 20, 50, 80 % of total)
pub const HEAT_LEVELS: [Color32; 4] = [rgb(0xffe48a), rgb(0xffd25a), rgb(0xffb63e), rgb(0xff9a3c)];
pub const HEAT_SEPARATOR: Color32 = rgb(0xead5a0);

pub const SCROLL_TRACK: Color32 = rgb(0xf0f0f0);
pub const SCROLL_THUMB: Color32 = rgb(0xcdcdcd);
/// own
pub const SCROLL_THUMB_HOVER: Color32 = rgb(0xa6a6a6);
pub const SCROLL_ARROW: Color32 = rgb(0x606060);

pub const FOOTER_ICON: Color32 = rgb(0x999999);
pub const BUTTON_BORDER: Color32 = rgb(0xadadad);
pub const BUTTON_FILL: Color32 = rgb(0xe1e1e1);
pub const BUTTON_HOVER_BORDER: Color32 = rgb(0x0078d7);
pub const BUTTON_HOVER_FILL: Color32 = rgb(0xe5f1fb);
/// own: pressed button
pub const BUTTON_PRESSED_BORDER: Color32 = rgb(0x005499);
pub const BUTTON_PRESSED_FILL: Color32 = rgb(0xcce4f7);
pub const BUTTON_DISABLED_BORDER: Color32 = rgb(0xbfbfbf);
pub const BUTTON_DISABLED_FILL: Color32 = rgb(0xcccccc);
pub const LINK: Color32 = rgb(0x0066cc);
pub const SERVICES_ICON: Color32 = rgb(0x7c8c96);

pub const LABEL: Color32 = rgb(0x707070);
pub const DEVICE_SELECTED: Color32 = rgb(0xd9d9d9);
pub const GRID: Color32 = rgb(0xd9eaf4);

/// Border/line and fill colour of a performance device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceColors {
    pub line: Color32,
    pub fill: Color32,
}

pub const CPU: DeviceColors = DeviceColors {
    line: rgb(0x117dbb),
    fill: rgb(0xf1f6fa),
};
pub const MEMORY: DeviceColors = DeviceColors {
    line: rgb(0x8b12ae),
    fill: rgb(0xf4f2f4),
};
pub const DISK: DeviceColors = DeviceColors {
    line: rgb(0x4da60c),
    fill: rgb(0xeff7e9),
};
pub const NETWORK: DeviceColors = DeviceColors {
    line: rgb(0xa54d05),
    fill: rgb(0xfcf3eb),
};
pub const GPU: DeviceColors = CPU;
pub const FAN: DeviceColors = DeviceColors {
    line: rgb(0x813d9c),
    fill: rgb(0xf6f1f8),
};
pub const BATTERY: DeviceColors = DeviceColors {
    line: rgb(0x3b8f4e),
    fill: rgb(0xf0f7f1),
};
