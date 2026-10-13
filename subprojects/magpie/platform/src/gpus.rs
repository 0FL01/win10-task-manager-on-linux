/* src/gpus.rs
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
use std::time::Instant;

use crate::profile::{self, Domain, Phase, QueueTicket, Span};
use parking_lot::Mutex;
use rayon::ThreadPool;

pub type ApiVersion = types::gpus::ApiVersion;
pub type OpenGLVariant = types::gpus::OpenGlVariant;
pub type ProcessKind = types::gpus::process::Kind;
pub type Process = types::gpus::Process;

pub type Gpu = types::gpus::Gpu;
pub use types::gpus::{
    ClientTelemetry, DriverInfo, EngineKind, EngineTelemetry, GpuTelemetry, MemoryTelemetry,
    ProcessIdentity, ProcessObservation, TelemetryState,
};

pub type GpuMap = types::gpus::gpus_response::GpuMap;
pub type GpusResponseKind = types::gpus::gpus_response::Response;
pub type GpusRequest = types::gpus::GpusRequest;
pub type GpusResponse = types::gpus::GpusResponse;
pub type GpusResponseError = types::gpus::GpusResponseError;

pub trait GpuCache {
    fn new() -> Self
    where
        Self: Sized;

    fn refresh(&mut self);
    fn cached_entries(&self) -> &HashMap<String, Gpu>;
}

pub struct GpusHandler<GC>
where
    GC: GpuCache,
{
    pub(crate) gpus: Mutex<GC>,
    pub(crate) local_cache: Mutex<HashMap<String, Gpu>>,
    last_cache_update: Mutex<Option<Instant>>,
    refreshing: AtomicBool,
}

impl<GC> Default for GpusHandler<GC>
where
    GC: GpuCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<GC> GpusHandler<GC>
where
    GC: GpuCache + Send,
{
    pub fn new() -> Self {
        let _profile = Span::new(Phase::Init(Domain::Gpus));
        Self {
            gpus: Mutex::new(GC::new()),
            local_cache: Mutex::new(HashMap::new()),
            last_cache_update: Mutex::new(None),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let refresh = Span::new(Phase::Refresh(Domain::Gpus));
        let mut gpus = self.gpus.lock();
        gpus.refresh();
        drop(refresh);
        let mut publish = Span::new(Phase::Publish(Domain::Gpus));
        publish.items(gpus.cached_entries().len());
        let mut cache = self.local_cache.lock();
        *cache = gpus.cached_entries().clone();
        *self.last_cache_update.lock() = Some(Instant::now());
    }

    /// A response snapshot with elapsed cache age added to each device's collection age.
    /// Reading this snapshot never advances the collector's epoch or sequence.
    pub fn cached_entries_with_age(&self) -> HashMap<String, Gpu> {
        let cache = self.local_cache.lock();
        let age = self
            .last_cache_update
            .lock()
            .map(|time| time.elapsed().as_millis().min(u64::MAX as u128) as u64)
            .unwrap_or(0);
        let mut result = cache.clone();
        for gpu in result.values_mut() {
            if let Some(telemetry) = gpu.telemetry.as_mut() {
                telemetry.sample_age_ms = telemetry.sample_age_ms.saturating_add(age);
            }
        }
        result
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            profile::coalesced(Phase::Refresh(Domain::Gpus));
            return;
        }

        let queued = QueueTicket::new(Domain::Gpus);
        thread_pool.spawn(move || {
            queued.arrived();
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed GPU information in {:?}", start.elapsed());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct FakeCache(HashMap<String, Gpu>);
    impl GpuCache for FakeCache {
        fn new() -> Self {
            Self(HashMap::from([(
                "gpu".into(),
                Gpu {
                    telemetry: Some(GpuTelemetry {
                        epoch: 7,
                        sequence: 0,
                        sample_age_ms: 5,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )]))
        }
        fn refresh(&mut self) {
            self.0
                .get_mut("gpu")
                .unwrap()
                .telemetry
                .as_mut()
                .unwrap()
                .sequence += 1;
        }
        fn cached_entries(&self) -> &HashMap<String, Gpu> {
            &self.0
        }
    }

    #[test]
    fn response_age_increases_without_advancing_or_mutating_cached_sample() {
        let handler = GpusHandler::<FakeCache>::new();
        handler.refresh();
        *handler.last_cache_update.lock() = Some(Instant::now() - Duration::from_secs(2));
        let first = handler.cached_entries_with_age()["gpu"]
            .telemetry
            .clone()
            .unwrap();
        let second = handler.cached_entries_with_age()["gpu"]
            .telemetry
            .clone()
            .unwrap();
        assert_eq!((first.epoch, first.sequence), (7, 1));
        assert_eq!((second.epoch, second.sequence), (7, 1));
        assert!(first.sample_age_ms >= 2005);
        assert!(second.sample_age_ms >= first.sample_age_ms);
        assert_eq!(
            handler.local_cache.lock()["gpu"]
                .telemetry
                .as_ref()
                .unwrap()
                .sample_age_ms,
            5
        );
        handler.refresh();
        assert_eq!(
            handler.cached_entries_with_age()["gpu"]
                .telemetry
                .as_ref()
                .unwrap()
                .sequence,
            2
        );
    }
}
