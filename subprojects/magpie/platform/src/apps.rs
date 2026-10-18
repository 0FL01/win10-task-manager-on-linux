/* src/apps.rs
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
use std::sync::atomic::{AtomicBool, Ordering};

use crate::profile::{self, Domain, Phase, QueueTicket, Span};
use parking_lot::Mutex;
use rayon::ThreadPool;

use crate::processes::Process;

pub type Icon = types::apps::Icon;
pub type IconKind = types::apps::icon::Icon;
pub type App = types::apps::App;

pub type AppList = types::apps::apps_response::AppList;
pub type AppResponseKind = types::apps::apps_response::Response;
pub type AppsRequest = types::apps::AppsRequest;
pub type AppsResponse = types::apps::AppsResponse;
pub type AppsResponseError = types::apps::AppsResponseError;

pub type AppsIconResponseValue = types::apps::apps_icon_response::AppsIconResponseValue;
pub type AppIconResponseKind = types::apps::apps_icon_response::Response;
pub type AppsIconRequest = types::apps::AppsIconRequest;
pub type AppsIconResponse = types::apps::AppsIconResponse;
pub type AppsIconResponseError = types::apps::AppsIconResponseError;

pub trait AppCache {
    fn new() -> Self
    where
        Self: Sized;
    fn refresh(&mut self, processes: &Mutex<HashMap<u32, Process>>);
    fn refresh_icons(&mut self);
    fn cached_entries(&self) -> &[App];
    fn cached_icons(&self) -> HashMap<String, Icon>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    struct FakeCache {
        entries: Vec<App>,
        started: SyncSender<()>,
        release: Receiver<()>,
    }
    impl AppCache for FakeCache {
        fn new() -> Self {
            panic!("test constructs gated cache explicitly")
        }
        fn refresh(&mut self, _: &Mutex<HashMap<u32, Process>>) {
            self.entries = vec![App {
                id: "ready-before-icons".into(),
                ..Default::default()
            }];
        }
        fn refresh_icons(&mut self) {
            self.started.send(()).unwrap();
            self.release
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
        }
        fn cached_entries(&self) -> &[App] {
            &self.entries
        }
        fn cached_icons(&self) -> HashMap<String, Icon> {
            HashMap::new()
        }
    }
    #[test]
    fn slow_icons_do_not_hide_a_completed_application_map() {
        let (started_tx, started) = sync_channel(1);
        let (release, released) = sync_channel(1);
        let handler = AppsHandler {
            apps: Mutex::new(FakeCache {
                entries: vec![],
                started: started_tx,
                release: released,
            }),
            local_cache: Mutex::new(vec![]),
            local_icons_cache: Mutex::new(HashMap::new()),
            refreshing: AtomicBool::new(false),
        };
        let processes = Box::leak(Box::new(Mutex::new(HashMap::new())));
        std::thread::scope(|scope| {
            scope.spawn(|| handler.refresh(processes));
            started
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            let published = handler
                .local_cache
                .try_lock()
                .expect("icon work must not lock the map");
            assert_eq!(published[0].id, "ready-before-icons");
            drop(published);
            release.send(()).unwrap();
        });
    }
}

pub struct AppsHandler<AC>
where
    AC: AppCache,
{
    pub(crate) apps: Mutex<AC>,
    pub(crate) local_cache: Mutex<Vec<App>>,
    pub(crate) local_icons_cache: Mutex<HashMap<String, Icon>>,
    refreshing: AtomicBool,
}

impl<AC> Default for AppsHandler<AC>
where
    AC: AppCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<AC> AppsHandler<AC>
where
    AC: AppCache + Send,
{
    pub fn new() -> Self {
        let _profile = Span::new(Phase::Init(Domain::Apps));
        Self {
            apps: Mutex::new(AC::new()),
            local_cache: Mutex::new(Vec::new()),
            local_icons_cache: Mutex::new(HashMap::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self, processes: &'static Mutex<HashMap<u32, Process>>) {
        let refresh = Span::new(Phase::Refresh(Domain::Apps));
        let mut apps = self.apps.lock();
        apps.refresh(processes);
        drop(refresh);
        let mut publish = Span::new(Phase::Publish(Domain::Apps));
        publish.items(apps.cached_entries().len());
        *self.local_cache.lock() = apps.cached_entries().to_vec();
        drop(publish);
        // Names/PIDs are usable before icon-theme discovery and file reads complete.
        apps.refresh_icons();
        *self.local_icons_cache.lock() = apps.cached_icons();
    }

    pub fn refresh_async(
        &'static self,
        thread_pool: &ThreadPool,
        processes: &'static Mutex<HashMap<u32, Process>>,
    ) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            profile::coalesced(Phase::Refresh(Domain::Apps));
            return;
        }

        let queued = QueueTicket::new(Domain::Apps);
        thread_pool.spawn(move || {
            queued.arrived();
            let start = std::time::Instant::now();

            self.refresh(processes);
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed apps in {:?}", start.elapsed());
        })
    }
}
