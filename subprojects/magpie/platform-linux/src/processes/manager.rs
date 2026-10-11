/* src/processes/manager.rs
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

use std::process::Command;

use libc::c_int;

use crate::util::ELEVATION_COMMAND;

pub struct ProcessManager;

macro_rules! send_signal {
    ($pids: expr, $signal: expr) => {
        if !Self::send_signals(&$pids, $signal) {
            log::warn!(
                "Failed to send signal {} to {:?}, trying elevated",
                $signal,
                $pids
            );
            Self::send_elevated_signals(&$pids, $signal);
        }
    };
}

impl magpie_platform::processes::ProcessManager for ProcessManager {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {}
    }

    fn terminate_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGTERM);
    }

    fn kill_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGKILL);
    }

    fn interrupt_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGINT);
    }

    fn signal_user_one_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGUSR1);
    }

    fn signal_user_two_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGUSR2);
    }

    fn hangup_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGHUP);
    }

    fn continue_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGCONT);
    }

    fn suspend_processes(&self, pids: Vec<u32>) {
        send_signal!(pids, libc::SIGSTOP);
    }
}

impl ProcessManager {
    fn send_signals(pids: &Vec<u32>, signal: c_int) -> bool {
        let mut success = true;

        for pid in pids {
            let sig = unsafe { libc::kill(*pid as i32, signal) };

            success = success && sig == 0;
        }

        success
    }

    fn send_elevated_signals(pids: &[u32], signal: c_int) {
        let mut command = Command::new(ELEVATION_COMMAND[0]);
        let command = command
            .arg(ELEVATION_COMMAND[1])
            .arg("kill")
            .arg(format!("-{}", signal))
            .args(pids.iter().map(|pid| pid.to_string()));

        let status = command.status();

        if let Err(error) = status {
            log::error!("Failed to send elevated signal command: {:?}", error);
        }
    }
}
