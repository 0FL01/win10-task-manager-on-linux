/* src/services/systemd/manager.rs
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

use std::num::NonZeroU32;
use std::slice::from_raw_parts;
use std::str::from_utf8_unchecked;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use libloading::{Library, Symbol};
use zbus::proxy::MethodFlags;
use zbus::zvariant::DynamicTuple;
use zbus::{Connection, Proxy};

use crate::{async_runtime, sync, system_bus, user_bus};

use super::{service, SystemDError, SystemDManagerProxy};

#[allow(dead_code)]
mod ffi {
    #[repr(C)]
    #[allow(non_camel_case_types)]
    pub struct sd_journal {
        _unused: [u8; 0],
    }

    pub type FnSdJournalOpen =
        unsafe extern "C" fn(ret: *mut *mut sd_journal, flags: i32) -> libc::c_int;
    pub type FnSdJournalClose = unsafe extern "C" fn(j: *mut sd_journal);

    pub type FnSdJournalAddMatch = unsafe extern "C" fn(
        j: *mut sd_journal,
        match_: *const libc::c_void,
        size: libc::size_t,
    ) -> libc::c_int;
    pub type FnSdJournalAddDisjunction = unsafe extern "C" fn(j: *mut sd_journal) -> libc::c_int;
    pub type FnSdJournalAddConjunction = unsafe extern "C" fn(j: *mut sd_journal) -> libc::c_int;

    pub type FnSdJournalSeekTail = unsafe extern "C" fn(j: *mut sd_journal) -> libc::c_int;
    pub type FnSdJournalPrevious = unsafe extern "C" fn(j: *mut sd_journal) -> libc::c_int;

    pub type FnSdJournalGetData = unsafe extern "C" fn(
        j: *mut sd_journal,
        field: *const libc::c_char,
        data: *mut *const libc::c_void,
        length: *mut libc::size_t,
    ) -> libc::c_int;

    pub const SD_JOURNAL_LOCAL_ONLY: i32 = 1 << 0;
    pub const SD_JOURNAL_RUNTIME_ONLY: i32 = 1 << 1;
    pub const SD_JOURNAL_SYSTEM: i32 = 1 << 2;
    pub const SD_JOURNAL_CURRENT_USER: i32 = 1 << 3;
    pub const SD_JOURNAL_OS_ROOT: i32 = 1 << 4;
    pub const SD_JOURNAL_ALL_NAMESPACES: i32 = 1 << 5;
    pub const SD_JOURNAL_INCLUDE_DEFAULT_NAMESPACE: i32 = 1 << 6;
    pub const SD_JOURNAL_TAKE_DIRECTORY_FD: i32 = 1 << 7;
    pub const SD_JOURNAL_ASSUME_IMMUTABLE: i32 = 1 << 8;

    pub const SD_JOURNAL_NOP: i32 = 0;
    pub const SD_JOURNAL_APPEND: i32 = 1;
    pub const SD_JOURNAL_INVALIDATE: i32 = 2;
}

const MAX_LOG_MESSAGE_COUNT: usize = 5;

pub struct ServiceManager {
    systemd1: SystemDManagerProxy<'static>,

    fn_sd_journal_open: Symbol<'static, ffi::FnSdJournalOpen>,
    fn_sd_journal_close: Symbol<'static, ffi::FnSdJournalClose>,
    fn_sd_journal_seek_tail: Symbol<'static, ffi::FnSdJournalSeekTail>,
    fn_sd_journal_add_match: Symbol<'static, ffi::FnSdJournalAddMatch>,
    fn_sd_journal_add_disjunction: Symbol<'static, ffi::FnSdJournalAddDisjunction>,
    fn_sd_journal_add_conjunction: Symbol<'static, ffi::FnSdJournalAddConjunction>,
    fn_sd_journal_previous: Symbol<'static, ffi::FnSdJournalPrevious>,
    fn_sd_journal_get_data: Symbol<'static, ffi::FnSdJournalGetData>,

    boot_id: String,
}

fn systemd_lib() -> Option<&'static Library> {
    static RC_LIB: OnceLock<Option<Library>> = OnceLock::new();

    RC_LIB
        .get_or_init(|| unsafe { Library::new("libsystemd.so.0") }.ok())
        .as_ref()
}

impl ServiceManager {
    pub fn system_manager() -> Result<Self, SystemDError> {
        let bus = match system_bus() {
            Some(bus) => bus,
            None => {
                return Err(SystemDError::DBusConnectionError);
            }
        };

        Self::new(bus)
    }

    pub fn user_manager() -> Result<Self, SystemDError> {
        let bus = match user_bus() {
            Some(bus) => bus,
            None => {
                return Err(SystemDError::DBusConnectionError);
            }
        };

        Self::new(bus)
    }

    fn new(bus: &Connection) -> Result<Self, SystemDError> {
        let handle = match systemd_lib() {
            Some(l) => l,
            None => {
                return Err(SystemDError::LibSystemDNotFound);
            }
        };

        let rt = async_runtime();
        let systemd1 = sync!(rt, SystemDManagerProxy::new(bus))?;

        let fn_sd_journal_open =
            unsafe { handle.get::<ffi::FnSdJournalOpen>(b"sd_journal_open\0")? };
        let fn_sd_journal_close =
            unsafe { handle.get::<ffi::FnSdJournalClose>(b"sd_journal_close\0")? };
        let fn_sd_journal_seek_tail =
            unsafe { handle.get::<ffi::FnSdJournalSeekTail>(b"sd_journal_seek_tail\0")? };
        let fn_sd_journal_add_match =
            unsafe { handle.get::<ffi::FnSdJournalAddMatch>(b"sd_journal_add_match\0")? };
        let fn_sd_journal_add_disjunction = unsafe {
            handle.get::<ffi::FnSdJournalAddDisjunction>(b"sd_journal_add_disjunction\0")?
        };
        let fn_sd_journal_add_conjunction = unsafe {
            handle.get::<ffi::FnSdJournalAddConjunction>(b"sd_journal_add_conjunction\0")?
        };
        let fn_sd_journal_previous =
            unsafe { handle.get::<ffi::FnSdJournalPrevious>(b"sd_journal_previous\0")? };
        let fn_sd_journal_get_data =
            unsafe { handle.get::<ffi::FnSdJournalGetData>(b"sd_journal_get_data\0")? };

        let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .replace("-", "");

        Ok(Self {
            systemd1,

            fn_sd_journal_open,
            fn_sd_journal_close,
            fn_sd_journal_seek_tail,
            fn_sd_journal_add_match,
            fn_sd_journal_add_conjunction,
            fn_sd_journal_add_disjunction,
            fn_sd_journal_previous,
            fn_sd_journal_get_data,

            boot_id,
        })
    }

    pub fn service_logs(
        &self,
        name: &str,
        pid: Option<NonZeroU32>,
    ) -> Result<String, SystemDError> {
        fn error_string(mut errno: i32) -> String {
            if errno < 0 {
                errno = -errno;
            }

            unsafe {
                let mut buf = [0; 1024];
                let _ = libc::strerror_r(errno, buf.as_mut_ptr(), buf.len());
                let c_str = std::ffi::CStr::from_ptr(buf.as_ptr());

                c_str.to_string_lossy().into()
            }
        }

        struct JournalHandle {
            j: *mut ffi::sd_journal,
            close: Symbol<'static, ffi::FnSdJournalClose>,
        }

        impl Drop for JournalHandle {
            fn drop(&mut self) {
                unsafe { (self.close)(self.j) };
            }
        }

        let mut j: *mut ffi::sd_journal = std::ptr::null_mut();
        let ret = unsafe { (self.fn_sd_journal_open)(&mut j, ffi::SD_JOURNAL_SYSTEM) };
        if ret < 0 {
            let err_string = error_string(ret);
            log::error!("Failed to open journal: {}", err_string);

            return Err(SystemDError::JournalOpenError(err_string));
        }

        let raii_handle = JournalHandle {
            j,
            close: self.fn_sd_journal_close.clone(),
        };

        let ret = unsafe {
            (self.fn_sd_journal_add_match)(j, format!("UNIT={}\0", name).as_ptr() as _, 0)
        };
        if ret < 0 {
            let err_string = error_string(ret);
            log::error!("Failed to add match: {}", err_string);

            return Err(SystemDError::JournalAddMatchError(err_string));
        }

        let ret = unsafe { (self.fn_sd_journal_add_disjunction)(j) };
        if ret < 0 {
            let err_string = error_string(ret);
            log::error!("Failed to add disjunction: {}", err_string);

            return Err(SystemDError::JournalAddDisjunctionError(err_string));
        }

        if let Some(pid) = pid {
            let ret = unsafe {
                (self.fn_sd_journal_add_match)(j, format!("_PID={}\0", pid).as_ptr() as _, 0)
            };
            if ret < 0 {
                let err_string = error_string(ret);
                log::error!("Failed to add match: {}", err_string);

                return Err(SystemDError::JournalAddMatchError(err_string));
            }
        }

        let ret = unsafe { (self.fn_sd_journal_add_conjunction)(j) };
        if ret < 0 {
            let err_string = error_string(ret);
            log::error!("Failed to add disjunction: {}", err_string);

            return Err(SystemDError::JournalAddConjunctionError(err_string));
        }

        let ret = unsafe {
            (self.fn_sd_journal_add_match)(
                j,
                format!("_BOOT_ID={}\0", self.boot_id).as_ptr() as _,
                0,
            )
        };
        if ret < 0 {
            let err_string = error_string(ret);
            log::error!("Failed to add match: {}", err_string);

            return Err(SystemDError::JournalAddMatchError(err_string));
        }

        let ret = unsafe { (self.fn_sd_journal_seek_tail)(j) };
        if ret < 0 {
            let err_string = error_string(ret);
            log::error!("Failed to seek to tail: {}", err_string);

            return Err(SystemDError::JournalSeekError(err_string));
        }

        let mut messages = Vec::with_capacity(MAX_LOG_MESSAGE_COUNT);
        loop {
            let ret = unsafe { (self.fn_sd_journal_previous)(j) };
            if ret == 0 {
                break;
            }

            if ret < 0 {
                let err_string = error_string(ret);
                log::error!("Failed to iterate journal entries: {}", err_string);

                return Err(SystemDError::JournalIterateError(err_string));
            }

            let mut data: *const libc::c_void = std::ptr::null_mut();
            let mut length: libc::size_t = 0;

            let ret = unsafe {
                (self.fn_sd_journal_get_data)(j, c"MESSAGE".as_ptr() as _, &mut data, &mut length)
            };
            if ret == 0 {
                if messages.len() >= MAX_LOG_MESSAGE_COUNT {
                    break;
                }

                let message =
                    unsafe { from_utf8_unchecked(from_raw_parts(data as *const u8, length)) }[8..]
                        .to_owned();
                messages.push(message);
            }
        }

        drop(raii_handle);

        messages.reverse();
        Ok(messages.join("\n"))
    }

    pub fn start_service(&self, service: &str) -> Result<(), SystemDError> {
        let rt = async_runtime();

        let systemd1 = self.systemd1.as_ref().clone();
        let name = service.to_owned();

        rt.spawn(async move {
            if let Err(err) = service::start(&systemd1, &name, "fail").await {
                log::error!("Failed to start service: {err}");
            }
        });

        Ok(())
    }

    pub fn stop_service(&self, service: &str) -> Result<(), SystemDError> {
        let rt = async_runtime();

        let systemd1 = self.systemd1.as_ref().clone();
        let name = service.to_owned();

        rt.spawn(async move {
            if let Err(err) = service::stop(&systemd1, &name, "replace").await {
                log::error!("Failed to stop service: {err}");
            }
        });

        Ok(())
    }

    pub fn restart_service(&self, service: &str) -> Result<(), SystemDError> {
        let rt = async_runtime();

        let systemd1 = self.systemd1.as_ref().clone();
        let service = service.to_owned();

        rt.spawn(async move {
            if let Err(err) = service::restart(&systemd1, &service, "fail").await {
                log::error!("Failed to restart service: {err}");
            }
        });

        Ok(())
    }

    pub fn enable_service(
        &self,
        service: &str,
        systemd: &super::SystemD,
    ) -> Result<(), SystemDError> {
        let rt = async_runtime();

        let systemd1 = self.systemd1.as_ref().clone();
        let service = service.to_owned();
        let full_refresh_required = Arc::clone(systemd.full_refresh_required());

        rt.spawn(async move {
            if let Err(err) = service::enable(&systemd1, &service).await {
                log::error!("Failed to enable service: {err}");
                return;
            }

            if let Err(err) = daemon_reload(&systemd1).await {
                log::error!("Failed to reload systemd daemon: {err}");
                return;
            }

            full_refresh_required.store(true, Ordering::Release);
        });

        Ok(())
    }

    pub fn disable_service(
        &self,
        service: &str,
        systemd: &super::SystemD,
    ) -> Result<(), SystemDError> {
        let rt = async_runtime();

        let systemd1 = self.systemd1.as_ref().clone();
        let service = service.to_owned();
        let full_refresh_required = Arc::clone(systemd.full_refresh_required());

        rt.spawn(async move {
            if let Err(err) = service::disable(&systemd1, &service).await {
                log::error!("Failed to enable service: {err}");
                return;
            }

            if let Err(err) = daemon_reload(&systemd1).await {
                log::error!("Failed to reload systemd daemon: {err}");
                return;
            }

            full_refresh_required.store(true, Ordering::Release);
        });

        Ok(())
    }
}

async fn daemon_reload(proxy: &Proxy<'static>) -> zbus::Result<Option<()>> {
    let reply = proxy
        .call_with_flags(
            "Reload",
            MethodFlags::AllowInteractiveAuth.into(),
            &DynamicTuple(()),
        )
        .await?;
    Ok(reply)
}
