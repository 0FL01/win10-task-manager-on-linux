/* magpie_client/spawner.rs
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

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const SIGUSR1: i32 = 10;
pub const PR_SET_PDEATHSIG: i32 = 1;

extern "C" {
    pub fn prctl(option: i32, ...) -> i32;
    pub fn signal(signum: i32, handler: usize) -> usize;
}

static TERMINATE_CHILD: AtomicBool = AtomicBool::new(false);
extern "C" fn on_sigusr1(_: i32) {
    TERMINATE_CHILD.store(true, Ordering::Relaxed);
}

fn main() {
    #[cfg(target_os = "linux")]
    unsafe {
        signal(SIGUSR1, on_sigusr1 as *const extern "C" fn(i32) as usize);
        prctl(PR_SET_PDEATHSIG, SIGUSR1);
    }

    let mut child = std::process::Command::new("/usr/bin/flatpak-spawn")
        .arg("--watch-bus")
        .arg("--host")
        .args(std::env::args().skip(1))
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .unwrap();

    while !TERMINATE_CHILD.load(Ordering::Relaxed) {
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                std::process::exit(status.code().unwrap());
            }
            Err(e) => {
                eprintln!("Error waiting for child process: {}", e);
                std::process::exit(1);
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    child.kill().unwrap();
}
