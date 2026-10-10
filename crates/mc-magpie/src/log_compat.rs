/* crates/mc-magpie/src/log_compat.rs
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

//! Drop-in replacements for the GLib logging macros used by the original client code.

macro_rules! g_critical {
    ($domain: expr, $($arg: tt)+) => {
        log::error!(target: $domain, $($arg)+)
    };
}

macro_rules! g_warning {
    ($domain: expr, $($arg: tt)+) => {
        log::warn!(target: $domain, $($arg)+)
    };
}

macro_rules! g_debug {
    ($domain: expr, $($arg: tt)+) => {
        log::debug!(target: $domain, $($arg)+)
    };
}

#[allow(unused_imports)]
pub(crate) use {g_critical, g_debug, g_warning};
