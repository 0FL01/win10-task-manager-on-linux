/* crates/mc-ui/src/cx.rs
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

//! Typed commands and the per-frame context handed to every view.

use mc_core::config::Config;
use mc_magpie::Request;

use crate::icons::IconCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainPage {
    Processes,
    Performance,
    Details,
    Services,
}

impl MainPage {
    /// Tab order.
    pub const ALL: [MainPage; 4] = [
        MainPage::Processes,
        MainPage::Performance,
        MainPage::Details,
        MainPage::Services,
    ];

    pub fn settings_name(&self) -> &'static str {
        match self {
            MainPage::Processes => "apps-page",
            MainPage::Performance => "performance-page",
            MainPage::Details => "details-page",
            MainPage::Services => "services-page",
        }
    }

    pub fn from_settings_name(name: &str) -> Self {
        match name {
            "apps-page" => MainPage::Processes,
            "details-page" => MainPage::Details,
            "services-page" => MainPage::Services,
            _ => MainPage::Performance,
        }
    }

    pub fn label(&self) -> String {
        use mc_core::i18n::i18n;
        match self {
            MainPage::Processes => i18n("Processes"),
            MainPage::Performance => i18n("Performance"),
            MainPage::Details => i18n("Details"),
            MainPage::Services => i18n("Services"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    KeyboardShortcuts,
    About,
    AboutSystem,
    FirstRun,
    ProcessDetails(String),
    ServiceDetails(String),
    SmartData(String),
}

/// Everything a view can ask the application to do. Views never perform I/O themselves.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Magpie(Request),
    CopyText(String),
    OpenUrl(String),
    OpenDialog(Dialog),
    SetPage(MainPage),
    ToggleSummaryMode,
    /// Select the first visible device of a category ("cpu", "memory", "disk", ...)
    SelectDeviceCategory(&'static str),
    SelectDevice(String),
    /// Open the desktop network settings (`true`: Wi-Fi panel)
    OpenNetworkSettings(bool),
    /// Start a program (File → Run new task)
    RunTask(String),
    /// View → Refresh now
    RefreshNow,
    Quit,
}

/// Per-frame context passed to views.
pub struct Cx<'a> {
    pub icons: &'a mut IconCache,
    pub config: &'a mut Config,
    pub commands: &'a mut Vec<Command>,
}

impl Cx<'_> {
    pub fn send(&mut self, command: Command) {
        self.commands.push(command);
    }
}
