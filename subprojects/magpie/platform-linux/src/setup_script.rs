/* src/setup_script.rs
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

use std::fs::{set_permissions, OpenOptions};
use std::io;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use which::which;

use crate::util::ELEVATION_COMMAND;

pub const COMMAND: &str = "/tmp/missioncenter-magpie-setup";

pub const SCRIPT_CONTENT: &str = include_str!("../bin/missioncenter-magpie-setup-linux");

pub fn run(revert: bool) -> Result<(), String> {
    let Some(file_name) = get_file_name() else {
        return Err("Could not find file!".to_string());
    };
    let mut command = Command::new(ELEVATION_COMMAND[0]);
    let mut command = command
        .arg(ELEVATION_COMMAND[1])
        .arg(&file_name)
        .stdout(Stdio::inherit());
    if revert {
        command = command.arg("--revert");
    }
    match command.output() {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).to_string())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

pub fn open() -> Result<(), String> {
    let Some(file_name) = get_file_name() else {
        return Err("Could not find file!".to_string());
    };
    let mut command = Command::new("xdg-open");
    let command = command.arg(&file_name);
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub fn get_file_name() -> Option<String> {
    let path = COMMAND.to_string();
    match create_script(&path) {
        Ok(()) => Some(path),
        Err(e) => {
            if e.kind() == ErrorKind::AlreadyExists {
                return Some(path);
            }
            log::info!("Could not create temp script file!: `{e}`",);
            None
        }
    }
}

pub fn get_elevation_command() -> String {
    match which("sudo") {
        Ok(_) => "sudo",
        Err(_) => "su",
    }
    .into()
}

fn create_script(path: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;

    file.write_all(SCRIPT_CONTENT.as_bytes())?;
    file.sync_all()?;

    // change file as executable
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    set_permissions(path, perms)?;

    Ok(())
}
