/* crates/mc-ui/src/lib.rs
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

//! egui user interface of Task Manager. Depends on egui only (no eframe), so it can be
//! driven by eframe in the application and by egui_kittest in tests.

pub mod app;
pub mod cx;
pub mod dialogs;
pub mod icons;
pub mod perf;
pub mod smart;
pub mod table;
pub mod win10;

pub use app::{App, AppOptions, DataSource};
pub use dialogs::WIKI_URL;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Name shown in the interface (title bar, window title, About); binary and package
/// `taskmanager`, app id `io.github.taskmanager.TaskManager`.
pub const APP_NAME: &str = "Task Manager";
