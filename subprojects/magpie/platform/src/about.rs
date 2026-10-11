/* src/about.rs
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

use parking_lot::Mutex;
use rayon::ThreadPool;

pub type About = types::about::About;

pub type AboutOsInfo = types::about::about::OsInfo;
pub type AboutDeInfo = types::about::about::DeInfo;
pub type AboutDeviceInfo = types::about::about::DeviceInfo;
pub type AboutRequest = types::about::AboutRequest;
pub type AboutResponse = types::about::AboutResponse;
pub type AboutResponseInfoResponse = types::about::about_response::Response;
pub type AboutResponseError = types::about::AboutResponseError;

pub trait AboutCache {
    fn new() -> Self
    where
        Self: Sized;
    fn refresh(&mut self);
    fn about(&self) -> &About;
}

pub struct AboutHandler<MC>
where
    MC: AboutCache,
{
    pub(crate) about: Mutex<MC>,
    pub(crate) local_cache: Mutex<About>,
    refreshing: AtomicBool,
}

impl<MC> Default for AboutHandler<MC>
where
    MC: AboutCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<MC> AboutHandler<MC>
where
    MC: AboutCache + Send,
{
    pub fn new() -> Self {
        Self {
            about: Mutex::new(MC::new()),
            local_cache: Mutex::new(About::default()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut about = self.about.lock();
        about.refresh();
        *self.local_cache.lock() = about.about().clone();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed about information in {:?}", start.elapsed());
        });
    }
}
