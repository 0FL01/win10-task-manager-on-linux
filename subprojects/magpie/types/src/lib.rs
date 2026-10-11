/* src/apps.proto
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

#![allow(clippy::module_inception)]
#![allow(clippy::large_enum_variant)]

pub use prost;
pub use strum::IntoEnumIterator;

pub mod about;
pub mod apps;
pub mod battery;
pub mod common;
pub mod cpu;
pub mod disks;
pub mod fan;
pub mod gpus;
pub mod ipc;
pub mod memory;
pub mod network;
pub mod processes;
pub mod services;
pub mod setup_script;
