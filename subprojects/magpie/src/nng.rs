/* src/nng.rs
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

pub use nng_c_sys::nng_errno_enum::*;
pub use nng_c_sys::nng_log_level::*;
pub use nng_c_sys::*;
use prost::bytes::buf::UninitSlice;
use prost::bytes::BufMut;
use std::mem::ManuallyDrop;

pub const NNG_OK: i32 = 0;

pub struct Buffer {
    ptr: *mut u8,
    capacity: usize,
    len: usize,
}

impl Buffer {
    pub fn new(capacity: usize) -> Self {
        let ptr = unsafe { nng_alloc(capacity) as *mut u8 };
        if ptr.is_null() {
            panic!("Failed to allocate buffer");
        }

        Self {
            ptr,
            capacity,
            len: 0,
        }
    }

    pub fn leak(self) -> (*mut u8, usize) {
        let this = ManuallyDrop::new(self);
        (this.ptr, this.capacity)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            nng_free(self.ptr as *mut std::ffi::c_void, self.capacity);
        }
    }
}

unsafe impl BufMut for Buffer {
    fn remaining_mut(&self) -> usize {
        self.capacity - self.len
    }

    unsafe fn advance_mut(&mut self, cnt: usize) {
        if cnt > self.remaining_mut() {
            panic!("Buffer overflow");
        }
        self.len += cnt;
    }

    fn chunk_mut(&mut self) -> &mut UninitSlice {
        let remaining = self.remaining_mut();

        if remaining == 0 {
            return UninitSlice::new(&mut []);
        }

        unsafe { UninitSlice::from_raw_parts_mut(self.ptr.add(self.len), remaining) }
    }
}

pub extern "C" fn nng_log_fn(
    level: nng_log_level::Type,
    _facility: nng_log_facility::Type,
    message_id: *const libc::c_char,
    message: *const libc::c_char,
) {
    let message_id = unsafe { std::ffi::CStr::from_ptr(message_id) };
    let message = unsafe { std::ffi::CStr::from_ptr(message) };

    match level {
        NNG_LOG_NONE => {}
        NNG_LOG_ERR => {
            log::error!("{:?}: {:?}", message_id, message);
        }
        NNG_LOG_WARN => {
            log::warn!("{:?}: {:?}", message_id, message);
        }
        NNG_LOG_NOTICE => {
            log::info!("{:?}: {:?}", message_id, message);
        }
        NNG_LOG_INFO => {
            log::info!("{:?}: {:?}", message_id, message);
        }
        NNG_LOG_DEBUG => {
            log::debug!("{:?}: {:?}", message_id, message);
        }
        _ => {}
    }
}
