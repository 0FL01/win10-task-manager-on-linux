/* crates/mc-magpie/src/lib.rs
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

//! Toolkit agnostic access to the Magpie statistics daemon.
//!
//! A single background thread owns the IPC connection. Snapshots are published into a
//! [`LatestSlot`] (newer readings overwrite older, unconsumed ones) and every other result is
//! delivered as an [`Event`] over a bounded channel. Nothing here blocks the caller.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

macro_rules! cmd_flatpak_host {
    ($cmd: expr) => {{
        use std::process::Command;

        const FLATPAK_SPAWN_CMD: &str = "/usr/bin/flatpak-spawn";

        let mut cmd = Command::new(FLATPAK_SPAWN_CMD);
        cmd.arg("--host").arg("sh").arg("-c");
        cmd.arg($cmd);

        cmd
    }};
}

// Verbatim copy of the GTK application's IPC client; kept close to upstream on purpose.
#[allow(clippy::all)]
mod client;
mod fixture;
mod gatherer;
mod hardware_access;
mod log_compat;
pub mod profile;
mod slot;
pub mod sockets;
mod spd;
mod synthetic;
pub mod users;

pub use client::{
    App, Battery, Connection, Cpu, Disk, DiskKind, ErrorEjectFailed, Fan, Gpu, Memory,
    MemoryDevice, Process, Service, SmartData,
};
pub use fixture::{load_fixture_dir, write_fixture_dir};
pub use gatherer::{Event, MagpieClient, Readings, Request, Waker};
pub use hardware_access::HardwareTelemetry;
pub use magpie_types;
pub use magpie_types::about::About;
pub use magpie_types::apps::icon::Icon;
pub use slot::LatestSlot;
pub use spd::SpdEeprom;
pub use synthetic::{synthetic_readings, synthetic_series};

pub type Pid = u32;

/// Length of one refresh tick; the update speed is expressed in multiples of this.
pub const INTERVAL_STEP: f64 = 0.05;
/// Default refresh interval in seconds.
pub const BASE_INTERVAL: f64 = 1.;

fn user_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();

    HOME.get_or_init(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/tmp".into())
    })
    .as_path()
}

fn flatpak_data_dir() -> &'static Path {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

    DATA_DIR
        .get_or_init(|| {
            let path = user_home().join(".var/app/io.github.taskmanager.TaskManager/data");
            std::fs::create_dir_all(&path).expect("Failed to create flatpak data directory");
            path
        })
        .as_path()
}

pub fn is_flatpak() -> bool {
    static IS_FLATPAK: OnceLock<bool> = OnceLock::new();
    *IS_FLATPAK.get_or_init(|| Path::new("/.flatpak-info").exists())
}

fn flatpak_app_path() -> &'static str {
    static FLATPAK_APP_PATH: OnceLock<String> = OnceLock::new();

    FLATPAK_APP_PATH
        .get_or_init(|| {
            let ini = match ini::Ini::load_from_file("/.flatpak-info") {
                Err(_) => return "".to_owned(),
                Ok(ini) => ini,
            };

            let section = match ini.section(Some("Instance")) {
                None => panic!("Unable to find `Instance` section in `/.flatpak-info`"),
                Some(section) => section,
            };

            match section.get("app-path") {
                None => {
                    panic!("Unable to find `app-path` key in section `Instance` missing from `/.flatpak-info`")
                }
                Some(app_path) => app_path.to_owned(),
            }
        })
        .as_str()
}

type FatalHook = Box<dyn Fn(String) + Send + Sync>;

static FATAL_HOOK: Mutex<Option<Arc<FatalHook>>> = Mutex::new(None);

pub(crate) fn set_fatal_hook(hook: FatalHook) {
    if let Ok(mut slot) = FATAL_HOOK.lock() {
        *slot = Some(Arc::new(hook));
    }
}

/// Reports an unrecoverable gatherer error to the UI and parks the calling thread forever.
///
/// The UI is expected to show the message and terminate the process.
pub(crate) fn fatal_error(message: &str) -> ! {
    log::error!(target: "TaskManager::Gatherer", "Fatal: {message}");

    let hook = FATAL_HOOK.lock().ok().and_then(|slot| slot.clone());
    match hook {
        Some(hook) => hook(message.to_owned()),
        None => {
            eprintln!("A fatal error has occurred: {message}");
            std::process::exit(-1);
        }
    }

    loop {
        std::thread::park();
    }
}
