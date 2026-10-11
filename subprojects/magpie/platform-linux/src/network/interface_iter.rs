/* src/network/interface_iter.rs
 *
 * Copyright 2025 Mission Center Developers
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

use std::borrow::Cow;

pub struct InterfaceIter {
    if_list: *mut libc::if_nameindex,
    current: u32,
}

impl Drop for InterfaceIter {
    fn drop(&mut self) {
        unsafe {
            libc::if_freenameindex(self.if_list);
        }
    }
}

impl InterfaceIter {
    pub fn new() -> Self {
        Self {
            if_list: unsafe { libc::if_nameindex() },
            current: 0,
        }
    }
}

impl Iterator for InterfaceIter {
    type Item = Cow<'static, str>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.if_list.is_null() {
            log::warn!("No network interfaces found");
            return None;
        }

        let entry = unsafe { self.if_list.add(self.current as usize) };
        if entry.is_null() {
            return None;
        }

        let entry = unsafe { &*entry };
        if entry.if_index == 0 || entry.if_name.is_null() {
            return None;
        }

        self.current += 1;

        let if_name = unsafe { std::ffi::CStr::from_ptr::<'static>(entry.if_name) };
        Some(if_name.to_string_lossy())
    }
}
