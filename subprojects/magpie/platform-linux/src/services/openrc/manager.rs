/* src/services/openrc/manager.rs
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

use std::process::Command;

use super::{rc_lib, OpenRCError};

pub struct ServiceManager {}

impl ServiceManager {
    pub fn new() -> Result<Self, OpenRCError> {
        if rc_lib().is_none() {
            return Err(OpenRCError::LibraryNotFound);
        }

        Ok(Self {})
    }

    pub fn start_service(&self, service: &str) -> Result<(), OpenRCError> {
        self.run_rc_command(&["rc-service", service, "start"])
    }

    pub fn stop_service(&self, service: &str) -> Result<(), OpenRCError> {
        self.run_rc_command(&["rc-service", service, "stop"])
    }

    pub fn restart_service(&self, service: &str) -> Result<(), OpenRCError> {
        self.run_rc_command(&["rc-service", service, "restart"])
    }

    pub fn enable_service(&self, service: &str) -> Result<(), OpenRCError> {
        self.run_rc_command(&["rc-update", "add", service])
    }

    pub fn disable_service(&self, service: &str) -> Result<(), OpenRCError> {
        self.run_rc_command(&["rc-update", "del", service])
    }

    fn run_rc_command(&self, args: &[&str]) -> Result<(), OpenRCError> {
        let cmd = Command::new("pkexec").args(args).output()?;
        if !cmd.status.success() {
            let error_message = String::from_utf8_lossy(&cmd.stderr)
                .as_ref()
                .trim()
                .to_owned();
            return Err(OpenRCError::CommandExecutionError(
                error_message,
                cmd.status.code().unwrap_or(-1),
            ));
        }

        Ok(())
    }
}
