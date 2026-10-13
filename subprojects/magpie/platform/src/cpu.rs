/* src/cpu.rs
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

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use rayon::ThreadPool;

pub type Cpu = types::cpu::Cpu;
pub type LogicalCpu = types::cpu::LogicalCpu;

pub type CpuResponseKind = types::cpu::cpu_response::Response;
pub type CpuRequest = types::cpu::CpuRequest;
pub type CpuResponse = types::cpu::CpuResponse;
pub type CpuResponseError = types::cpu::CpuResponseError;

pub trait CpuCache {
    fn new() -> Self
    where
        Self: Sized;

    fn refresh(&mut self);
    fn cached(&self) -> &Cpu;
}

pub struct CpuHandler<CC>
where
    CC: CpuCache,
{
    pub(crate) cpu: Mutex<CC>,
    pub(crate) local_cache: Mutex<Cpu>,
    refreshing: AtomicBool,
}

impl<CC> Default for CpuHandler<CC>
where
    CC: CpuCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<CC> CpuHandler<CC>
where
    CC: CpuCache + Send,
{
    pub fn new() -> Self {
        let cpu = CC::new();
        let local_cache = Mutex::new(cpu.cached().clone());

        Self {
            cpu: Mutex::new(cpu),
            local_cache,
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut cpu = self.cpu.lock();
        cpu.refresh();
        *self.local_cache.lock() = cpu.cached().clone();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed CPU information in {:?}", start.elapsed());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeCache(Cpu);

    impl CpuCache for FakeCache {
        fn new() -> Self {
            let mut cpu = Cpu::default();
            cpu.core_usage_percent.resize(8, 0.0);
            Self(cpu)
        }

        fn refresh(&mut self) {}

        fn cached(&self) -> &Cpu {
            &self.0
        }
    }

    /// mission-center#542
    #[test]
    fn a_fresh_handler_is_seeded_from_its_cache() {
        let handler = CpuHandler::<FakeCache>::new();

        assert_eq!(handler.local_cache.lock().core_usage_percent.len(), 8);
    }
}
