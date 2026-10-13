/* src/util.rs
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

use std::ffi::CStr;
use std::io::Read;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use zbus::proxy::PropertyStream;
use zbus::zvariant;

use libc::{c_longlong, c_ulonglong};

// do not run with interactive command line
pub const ELEVATION_COMMAND: [&str; 2] = ["pkexec", "--disable-internal-agent"];

#[macro_export]
macro_rules! sync {
    ($rt: ident, $code: expr) => {
        $rt.block_on(async { $code.await })
    };
}

pub fn async_runtime() -> &'static tokio::runtime::Handle {
    static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();

    RUNTIME.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        let handle = runtime.handle().clone();

        std::thread::Builder::new()
            .name("tokio-runtime".to_owned())
            .spawn(move || {
                runtime.block_on(async {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;
                    }
                });
            })
            .expect("Failed to spawn tokio runtime thread");

        handle
    })
}

pub fn beginning_of_time() -> std::time::Instant {
    unsafe {
        std::mem::transmute(libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        })
    }
}

pub fn cpu_count() -> usize {
    static CPU_COUNT: OnceLock<usize> = OnceLock::new();

    *CPU_COUNT.get_or_init(|| {
        let proc_stat = std::fs::read_to_string("/proc/stat").unwrap_or_else(|e| {
            log::error!("Failed to read /proc/stat: {}", e);
            "".to_owned()
        });

        proc_stat
            .lines()
            .map(|l| l.trim())
            .skip_while(|l| !l.starts_with("cpu"))
            .filter(|l| l.starts_with("cpu"))
            .count()
            .max(2)
            - 1
    })
}

pub fn for_process_in_proc(mut cb: impl FnMut(&str, u32)) {
    let proc = match std::fs::read_dir("/proc") {
        Ok(proc) => proc,
        Err(e) => {
            log::warn!("Failed to read /proc directory: {}", e);
            return;
        }
    };

    let proc_entries = proc
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false));
    for entry in proc_entries {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        let pid = match file_name.parse::<u32>().ok() {
            Some(pid) => pid,
            _ => continue,
        };

        cb(file_name.as_ref(), pid);
    }
}

pub fn hardware_db() -> &'static Path {
    static MC_MAGPIE_HW_DB: OnceLock<PathBuf> = OnceLock::new();

    MC_MAGPIE_HW_DB
        .get_or_init(|| {
            if let Ok(path) = std::env::var("MC_MAGPIE_HW_DB") {
                PathBuf::from(path).canonicalize().unwrap_or_else(|e| {
                    log::error!("Failed to canonicalize hardware database location: {e}. Using default location");
                    PathBuf::from("/usr/share/missioncenter/hw.db")
                })
            } else {
                log::debug!("MC_MAGPIE_HW_DB not set, using default hardware database location");
                PathBuf::from("/usr/share/missioncenter/hw.db")
            }
        })
        .as_path()
}

pub fn is_snap() -> bool {
    static IS_SNAP: OnceLock<bool> = OnceLock::new();
    *IS_SNAP.get_or_init(|| std::env::var_os("SNAP_CONTEXT").is_some())
}

pub fn read_bytes_fixed<'a, const LEN: usize>(
    path: impl Into<&'a str>,
    what: &str,
    buffer: &mut [u8; LEN],
) -> Option<NonZeroUsize> {
    let path = path.into();

    let mut file = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            if !magpie_platform::profile::metric_error(magpie_platform::profile::MetricError::Read)
            {
                log::warn!("Failed to read {what}. Failed to open `{path}`: {e}");
            }
            return None;
        }
    };

    let bytes_read = match file.read(&mut buffer[..LEN - 1]) {
        Ok(b) => b,
        Err(e) => {
            if !magpie_platform::profile::metric_error(magpie_platform::profile::MetricError::Read)
            {
                log::warn!("Failed to read {what}. Failed to read from `{path}`: {e}");
            }
            return None;
        }
    };

    NonZeroUsize::new(bytes_read)
}

pub fn read_i64<'a>(path: impl Into<&'a str>, what: &str) -> Option<i64> {
    let mut buffer = [0; 64];
    read_bytes_fixed(path, what, &mut buffer)?;

    let mut end_ptr: *mut libc::c_char = std::ptr::null_mut();

    // Set errno to 0 before calling strtoull
    unsafe { libc::__errno_location().write(0) };
    let result = unsafe { libc::strtoll(buffer.as_ptr() as _, &mut end_ptr, 10) };
    let errno = unsafe { *libc::__errno_location() };

    if errno == libc::ERANGE && result == c_longlong::MAX {
        log::warn!(
            "Failed to read {what}. Value out of range: `{:?}`",
            CStr::from_bytes_until_nul(&buffer)
        );
        return None;
    }

    if result == 0 && errno != 0 {
        log::warn!(
            "Failed to read {what}. Invalid value: `{:?}`",
            CStr::from_bytes_until_nul(&buffer)
        );
        return None;
    }

    Some(result)
}

pub fn parse_u64(bytes: &[u8], base: u8, what: &str) -> Option<u64> {
    parse_u64_impl(bytes, base, what, false)
}

/// Same parsing/baselines as the general helper, but aggregate PID-local
/// failures in opt-in diagnostics instead of logging individual input values.
pub(crate) fn parse_proc_u64(bytes: &[u8], base: u8, what: &str) -> Option<u64> {
    parse_u64_impl(bytes, base, what, true)
}

fn parse_u64_impl(bytes: &[u8], base: u8, what: &str, proc_value: bool) -> Option<u64> {
    let aggregate = || {
        if proc_value {
            magpie_platform::profile::proc_error(magpie_platform::profile::ProcError::Invalid)
        } else {
            magpie_platform::profile::metric_error(magpie_platform::profile::MetricError::Value)
        }
    };
    let c_str = CStr::from_bytes_until_nul(bytes)
        .inspect_err(|e| {
            if !aggregate() {
                log::warn!(
                    "Failed to read {what}. No NULL byte present in buffer: `{:?} (trimmed)`: {}",
                    &bytes[..20.min(bytes.len())],
                    e,
                );
            }
        })
        .ok()?;

    // Check for negative values
    {
        if bytes.trim_ascii_start().starts_with(b"-") {
            if !aggregate() {
                log::warn!("Failed to read {what}. Negative value: `{c_str:?}`",);
            }
            return None;
        }
    }

    let mut end_ptr: *mut libc::c_char = std::ptr::null_mut();

    // Set errno to 0 before calling strtoull
    unsafe { libc::__errno_location().write(0) };
    let result = unsafe { libc::strtoull(bytes.as_ptr() as _, &mut end_ptr, base as _) };
    let errno = unsafe { *libc::__errno_location() };

    if errno == libc::ERANGE && result == c_ulonglong::MAX {
        if !aggregate() {
            log::warn!("Failed to read {what}. Value out of range: `{c_str:?}`",);
        }
        return None;
    }

    if result == 0 && errno != 0 {
        if !aggregate() {
            log::warn!("Failed to read {what}. Invalid value: `{c_str:?}`",);
        }
        return None;
    }

    Some(result)
}

pub fn read_u64<'a>(path: impl Into<&'a str>, what: &str) -> Option<u64> {
    let mut buffer = [0; 64];
    let _ = read_bytes_fixed(path, what, &mut buffer)?;
    parse_u64(&buffer, 10, what)
}

pub async fn stream_has_contents<T>(
    stream: &mut PropertyStream<'_, T>,
) -> Option<Result<T, zbus::Error>>
where
    T: TryFrom<zvariant::OwnedValue>,
    T::Error: Into<zbus::Error>,
    T: Unpin,
{
    use futures::stream::Stream;
    use std::task::{Context, Poll};

    // Create a dummy context for polling
    let mut stream = std::pin::pin!(stream);

    // Poll the stream for a next value without blocking
    match stream
        .as_mut()
        .poll_next(&mut Context::from_waker(&futures::task::noop_waker()))
    {
        Poll::Ready(Some(str)) => Some(str.get().await),
        Poll::Ready(None) => None, // Stream ended without values
        Poll::Pending => None,     // No immediate value available
    }
}

pub fn sys_hz() -> usize {
    static HZ: OnceLock<usize> = OnceLock::new();

    *HZ.get_or_init(
        || match nix::unistd::sysconf(nix::unistd::SysconfVar::CLK_TCK) {
            Ok(Some(hz)) => hz as usize,
            Ok(None) => {
                log::error!("Failed to get system hz: sysconf returned None");
                100
            }
            Err(e) => {
                log::error!("Failed to get system hz: {}", e);
                100
            }
        },
    )
}

pub fn sys_page_size() -> usize {
    static PAGE_SIZE: OnceLock<usize> = OnceLock::new();

    *PAGE_SIZE.get_or_init(
        || match nix::unistd::sysconf(nix::unistd::SysconfVar::PAGE_SIZE) {
            Ok(Some(size)) => size as usize,
            Ok(None) => {
                log::error!("Failed to get page size: sysconf returned None");
                4096
            }
            Err(e) => {
                log::error!("Failed to get page size: {}", e);
                4096
            }
        },
    )
}

pub fn xdg_data_dirs() -> &'static Vec<String> {
    static XDG_DATA_DIRS: OnceLock<Vec<String>> = OnceLock::new();

    XDG_DATA_DIRS.get_or_init(|| {
        let mut out = vec![];
        match std::env::var("XDG_DATA_DIRS") {
            Ok(dir) => out.append(&mut dir.split(':').map(String::from).collect()),
            Err(e) => {
                log::info!("Failed to get XDG_DATA_DIRS: {e}. Falling back to `/usr/share`");
                out.push("/usr/share".into())
            }
        };
        let xdg_data_home = match std::env::var("XDG_DATA_HOME") {
            Ok(dir) => dir.to_string(),
            Err(e) => {
                log::info!(
                    "Failed to get XDG_DATA_HOME: {e}. Falling back to `$HOME/.local/share`",
                );
                format!("{}/.local/share", home_dir())
            }
        };
        out.push(xdg_data_home);
        out
    })
}

pub fn home_dir() -> &'static str {
    static HOME_DIR: OnceLock<String> = OnceLock::new();

    HOME_DIR.get_or_init(|| {
        std::env::var("HOME").unwrap_or_else(|e| {
            log::error!("Failed to get HOME: {}. Falling back to `/`", e);
            "/".into()
        })
    })
}

pub fn system_bus() -> Option<&'static zbus::Connection> {
    static SYSTEM_BUS: OnceLock<Option<zbus::Connection>> = OnceLock::new();

    SYSTEM_BUS
        .get_or_init(|| {
            match async_runtime().block_on(async { zbus::Connection::system().await }) {
                Ok(c) => Some(c),
                Err(e) => {
                    log::error!("Failed to connect to session bus: {}", e);
                    None
                }
            }
        })
        .as_ref()
}

pub fn user_bus() -> Option<&'static zbus::Connection> {
    static USER_BUS: OnceLock<Option<zbus::Connection>> = OnceLock::new();

    USER_BUS
        .get_or_init(|| {
            match async_runtime().block_on(async { zbus::Connection::session().await }) {
                Ok(c) => Some(c),
                Err(e) => {
                    log::error!("Failed to connect to session bus: {}", e);
                    None
                }
            }
        })
        .as_ref()
}
