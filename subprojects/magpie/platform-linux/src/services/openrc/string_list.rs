/* src/services/openrc/string_list.rs
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

#[repr(C)]
pub struct RC_STRING_QUEUE {
    pub tqe_next: *mut RC_STRING,
    pub tqe_prev: *mut *mut RC_STRING,
}

#[repr(C)]
pub struct RC_STRING {
    pub value: *mut libc::c_char,
    pub entries: RC_STRING_QUEUE,
}

#[repr(C)]
pub struct RC_STRINGLIST {
    pub tqh_first: *mut RC_STRING,
    pub tqh_last: *mut *mut RC_STRING,
}
