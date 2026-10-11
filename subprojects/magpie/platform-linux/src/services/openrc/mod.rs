/* src/services/openrc/mod.rs
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

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::OpenOptions;
use std::hash::Hasher;
use std::io::Read;
use std::sync::OnceLock;

use ahash::AHasher;
use libloading::Library;
use thiserror::Error;

use magpie_platform::services::Service;

pub use manager::ServiceManager;
use service::State;
use string_list::RC_STRINGLIST;

mod manager;
mod service;
mod string_list;

type FnRcRunLevelList = unsafe extern "C" fn() -> *mut RC_STRINGLIST;
type FnRcServicesInRunlevel = unsafe extern "C" fn(*const libc::c_char) -> *mut RC_STRINGLIST;
type FnRcServiceState = unsafe extern "C" fn(*const libc::c_char) -> u32;
type FnRcServiceDescription =
    unsafe extern "C" fn(*const libc::c_char, *const libc::c_char) -> *mut libc::c_char;
type FnRcServiceValueGet =
    unsafe extern "C" fn(*const libc::c_char, *const libc::c_char) -> *mut libc::c_char;
type FnRCStringListFree = unsafe extern "C" fn(*mut RC_STRINGLIST);

#[derive(Debug, Error)]
pub enum OpenRCError {
    #[error("Library not found")]
    LibraryNotFound,
    #[error("LibLoading error: {0}")]
    LibLoadingError(#[from] libloading::Error),
    #[error("Missing runlevels")]
    MissingRunLevels,
    #[error("Missing services")]
    MissingServices,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Command execution error: {0}. Exited with status code {1}")]
    CommandExecutionError(String, i32),
}

pub struct OpenRC {
    fn_rc_runlevel_list: libloading::Symbol<'static, FnRcRunLevelList>,
    fn_rc_services_in_runlevel: libloading::Symbol<'static, FnRcServicesInRunlevel>,
    fn_rc_service_state: libloading::Symbol<'static, FnRcServiceState>,
    fn_rc_service_description: libloading::Symbol<'static, FnRcServiceDescription>,
    fn_rc_service_value_get: libloading::Symbol<'static, FnRcServiceValueGet>,

    fn_rc_string_list_free: libloading::Symbol<'static, FnRCStringListFree>,
}

fn rc_lib() -> Option<&'static Library> {
    static RC_LIB: OnceLock<Option<Library>> = OnceLock::new();

    RC_LIB
        .get_or_init(|| unsafe { Library::new("librc.so.1") }.ok())
        .as_ref()
}

impl OpenRC {
    pub fn new() -> Result<Self, OpenRCError> {
        let handle = match rc_lib() {
            Some(l) => l,
            None => return Err(OpenRCError::LibraryNotFound),
        };

        let fn_rc_runlevel_list = unsafe { handle.get::<FnRcRunLevelList>(b"rc_runlevel_list\0")? };

        let fn_rc_services_in_runlevel =
            unsafe { handle.get::<FnRcServicesInRunlevel>(b"rc_services_in_runlevel\0")? };

        let fn_rc_service_state = unsafe { handle.get::<FnRcServiceState>(b"rc_service_state\0")? };

        let fn_rc_service_description =
            unsafe { handle.get::<FnRcServiceDescription>(b"rc_service_description\0")? };

        let fn_rc_service_value_get =
            unsafe { handle.get::<FnRcServiceValueGet>(b"rc_service_value_get\0")? };

        let fn_rc_string_list_free =
            unsafe { handle.get::<FnRCStringListFree>(b"rc_stringlist_free\0")? };

        Ok(Self {
            fn_rc_runlevel_list,
            fn_rc_services_in_runlevel,
            fn_rc_service_state,
            fn_rc_service_description,
            fn_rc_service_value_get,

            fn_rc_string_list_free,
        })
    }

    pub fn list_services(&self) -> Result<HashMap<u64, Service>, OpenRCError> {
        let runlevels = unsafe { (self.fn_rc_runlevel_list)() };
        if runlevels.is_null() {
            log::warn!("Empty runlevel list returned from OpenRC");
            return Err(OpenRCError::MissingRunLevels);
        }

        let mut result = HashMap::new();

        let mut buffer = String::new();

        let runlevel_list = unsafe { &*runlevels };
        let mut current_rl = runlevel_list.tqh_first;
        while !current_rl.is_null() {
            let rl_rc_string = unsafe { &*current_rl };
            let rl_name = unsafe { CStr::from_ptr(rl_rc_string.value) }.to_string_lossy();

            let services_names = unsafe { (self.fn_rc_services_in_runlevel)(rl_rc_string.value) };
            if services_names.is_null() {
                log::warn!("Empty service list returned for runlevel '{}'.", rl_name);
                continue;
            }

            let rc_string_list = unsafe { &*services_names };
            let mut current = rc_string_list.tqh_first;
            while !current.is_null() {
                let rc_string = unsafe { &*current };
                let service_name = unsafe { CStr::from_ptr(rc_string.value) };

                let description_cstr =
                    unsafe { (self.fn_rc_service_description)(rc_string.value, std::ptr::null()) };
                let description = if !description_cstr.is_null() {
                    let description = unsafe { CStr::from_ptr(description_cstr) }
                        .to_string_lossy()
                        .to_string();
                    unsafe {
                        libc::free(description_cstr as *mut libc::c_void);
                    }
                    description
                } else {
                    String::new()
                };

                let state = unsafe { (self.fn_rc_service_state)(rc_string.value) };

                let pidfile = unsafe {
                    (self.fn_rc_service_value_get)(
                        rc_string.value,
                        c"pidfile".as_ptr() as *const libc::c_char,
                    )
                };
                let pid = if !pidfile.is_null() {
                    let pidfile_str = unsafe { CStr::from_ptr(pidfile) }.to_string_lossy();
                    let pid = if let Ok(mut pf) =
                        OpenOptions::new().read(true).open(pidfile_str.as_ref())
                    {
                        buffer.clear();
                        pf.read_to_string(&mut buffer)
                            .map(|_| buffer.trim().parse::<u32>().unwrap_or_default())
                            .unwrap_or_default()
                    } else {
                        0
                    };

                    unsafe {
                        libc::free(pidfile as *mut libc::c_void);
                    }

                    pid
                } else {
                    0
                };

                let service_name = service_name.to_string_lossy();
                let service_id = {
                    let mut hasher = AHasher::default();
                    hasher.write(service_name.as_bytes());
                    hasher.finish()
                };
                let state: State = unsafe { std::mem::transmute(state & 0xFF) };
                result.insert(
                    service_id,
                    Service {
                        id: service_id,
                        name: service_name.to_string(),
                        description: if description.is_empty() {
                            None
                        } else {
                            Some(description)
                        },
                        enabled: !rl_name.is_empty(),
                        running: state == State::Started,
                        failed: state == State::Failed,
                        pid: if pid == 0 { None } else { Some(pid) },
                        user: Some("root".to_owned()),
                        group: Some("root".to_owned()),
                        file_path: None,
                    },
                );

                current = rc_string.entries.tqe_next;
            }

            unsafe { (self.fn_rc_string_list_free)(services_names) };

            current_rl = rl_rc_string.entries.tqe_next;
        }

        unsafe { (self.fn_rc_string_list_free)(runlevels) };

        let services_names = unsafe { (self.fn_rc_services_in_runlevel)(std::ptr::null()) };
        if !services_names.is_null() {
            let rc_string_list = unsafe { &*services_names };
            let mut current = rc_string_list.tqh_first;
            while !current.is_null() {
                let rc_string = unsafe { &*current };
                let service_name = unsafe { CStr::from_ptr(rc_string.value) };

                let description_cstr =
                    unsafe { (self.fn_rc_service_description)(rc_string.value, std::ptr::null()) };
                let description = if !description_cstr.is_null() {
                    let description = unsafe { CStr::from_ptr(description_cstr) }
                        .to_string_lossy()
                        .to_string();
                    unsafe {
                        libc::free(description_cstr as *mut libc::c_void);
                    }
                    description
                } else {
                    String::new()
                };

                let state = unsafe { (self.fn_rc_service_state)(rc_string.value) };

                let pidfile = unsafe {
                    (self.fn_rc_service_value_get)(
                        rc_string.value,
                        c"pidfile".as_ptr() as *const libc::c_char,
                    )
                };
                let pid = if !pidfile.is_null() {
                    let pidfile_str = unsafe { CStr::from_ptr(pidfile) }.to_string_lossy();
                    let pid = if let Ok(mut pf) =
                        OpenOptions::new().read(true).open(pidfile_str.as_ref())
                    {
                        buffer.clear();
                        pf.read_to_string(&mut buffer)
                            .map(|_| buffer.trim().parse::<u32>().unwrap_or_default())
                            .unwrap_or_default()
                    } else {
                        0
                    };

                    unsafe {
                        libc::free(pidfile as *mut libc::c_void);
                    }

                    pid
                } else {
                    0
                };

                let service_name = service_name.to_string_lossy();
                let service_id = {
                    let mut hasher = AHasher::default();
                    hasher.write(service_name.as_bytes());
                    hasher.finish()
                };
                let state: State = unsafe { std::mem::transmute(state & 0xFF) };
                result.entry(service_id).or_insert_with(|| Service {
                    id: service_id,
                    name: service_name.to_string(),
                    description: if description.is_empty() {
                        None
                    } else {
                        Some(description.to_string())
                    },
                    enabled: false,
                    running: state == State::Started,
                    failed: state == State::Failed,
                    pid: if pid == 0 { None } else { Some(pid) },
                    user: Some("root".to_owned()),
                    group: Some("root".to_owned()),
                    file_path: None,
                });

                current = rc_string.entries.tqe_next;
            }

            unsafe { (self.fn_rc_string_list_free)(services_names) };
        }

        if result.is_empty() {
            return Err(OpenRCError::MissingServices);
        }

        Ok(result)
    }
}
