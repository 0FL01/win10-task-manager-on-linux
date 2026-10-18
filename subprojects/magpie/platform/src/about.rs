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

use crate::profile::{self, Domain, Phase, QueueTicket, Span};
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

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeCache {
        data: About,
        reads: usize,
    }
    impl AboutCache for FakeCache {
        fn new() -> Self {
            Self {
                data: About::default(),
                reads: 0,
            }
        }
        fn refresh(&mut self) {
            self.reads += 1;
        }
        fn about(&self) -> &About {
            &self.data
        }
    }
    #[test]
    fn identity_is_loaded_once_even_with_concurrent_requests() {
        let handler = AboutHandler::<FakeCache>::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| handler.refresh());
            }
        });
        handler.refresh();
        assert_eq!(handler.about.lock().reads, 1);
    }
}

pub struct AboutHandler<MC>
where
    MC: AboutCache,
{
    pub(crate) about: Mutex<MC>,
    pub(crate) local_cache: Mutex<About>,
    refreshing: AtomicBool,
    initialized: std::sync::Once,
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
        let _profile = Span::new(Phase::Init(Domain::About));
        Self {
            about: Mutex::new(MC::new()),
            local_cache: Mutex::new(About::default()),
            refreshing: AtomicBool::new(false),
            initialized: std::sync::Once::new(),
        }
    }

    pub fn refresh(&self) {
        self.initialized.call_once(|| self.load());
    }

    // OS/firmware/session identity is static for the lifetime of this daemon. Repeated About
    // requests must not rerun desktop/package-manager subprocesses on the IPC serving thread.
    fn load(&self) {
        let refresh = Span::new(Phase::Refresh(Domain::About));
        let mut about = self.about.lock();
        about.refresh();
        drop(refresh);
        let mut publish = Span::new(Phase::Publish(Domain::About));
        publish.items(1);
        let mut data = about.about().clone();
        data.ready = Some(true);
        *self.local_cache.lock() = data;
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.initialized.is_completed() {
            return;
        }
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            profile::coalesced(Phase::Refresh(Domain::About));
            return;
        }

        let queued = QueueTicket::new(Domain::About);
        thread_pool.spawn(move || {
            queued.arrived();
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed about information in {:?}", start.elapsed());
        });
    }
}
