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
        Self {
            apps: Mutex::new(AC::new()),
            local_cache: Mutex::new(Vec::new()),
            local_icons_cache: Mutex::new(HashMap::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self, processes: &'static Mutex<HashMap<u32, Process>>) {
        let mut apps = self.apps.lock();
        apps.refresh(processes);
        *self.local_cache.lock() = apps.cached_entries().to_vec();
        *self.local_icons_cache.lock() = apps.cached_icons();
    }

    pub fn refresh_async(
        &'static self,
        thread_pool: &ThreadPool,
        processes: &'static Mutex<HashMap<u32, Process>>,
    ) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh(processes);
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed apps in {:?}", start.elapsed());
        })
    }
}
