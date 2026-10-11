/* src/disks.rs
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

pub type DiskKind = types::disks::DiskKind;
pub type DiskSmartInterfaceKind = types::disks::smart_interface::Kind;
pub type Disk = types::disks::Disk;
pub type PartitionInfo = types::disks::PartitionInfo;
pub type DiskSmartTestResult = types::disks::smart_data::TestResult;
pub type DiskSmartPrettyUnitKind = types::disks::smart_data::ata::PrettyUnit;
pub type DiskSmartAtaAttribute = types::disks::smart_data::ata::Attribute;
pub type DiskSmartDataAta = types::disks::smart_data::Ata;
pub type DiskSmartDataNvme = types::disks::smart_data::Nvme;
pub type DiskSmartDataVariant = types::disks::smart_data::Data;
pub type DiskOptionalSmartData = types::disks::disks_response::OptionalSmartData;
pub type DiskSmartData = types::disks::SmartData;

pub type DiskList = types::disks::disks_response::DiskList;
pub type DisksResponseKind = types::disks::disks_response::Response;
pub type DisksRequestKind = types::disks::disks_request::Request;
pub type DisksRequest = types::disks::DisksRequest;
pub type DisksResponse = types::disks::DisksResponse;
pub type DisksResponseErrorBlocker = types::disks::error_eject_failed::Blocker;
pub type DisksResponseErrorEjectFailed = types::disks::ErrorEjectFailed;
pub type DisksResponseErrorKind = types::disks::disks_response_error::Error;
pub type DisksResponseError = types::disks::DisksResponseError;

pub trait DisksCache {
    fn new() -> Self
    where
        Self: Sized;

    fn refresh(&mut self);
    fn cached_entries(&self) -> &[Disk];
}

pub trait DisksManager {
    fn new() -> Self
    where
        Self: Sized;

    fn eject(
        &self,
        id: &str,
        processes: &Mutex<HashMap<u32, Process>>,
    ) -> Result<(), DisksResponseErrorEjectFailed>;
    fn smart_data(&self, id: &str) -> Option<DiskSmartData>;
}

pub struct DisksHandler<DC, DM>
where
    DC: DisksCache,
    DM: DisksManager,
{
    pub(crate) disks: Mutex<DC>,
    pub(crate) disk_manager: DM,
    pub(crate) local_cache: Mutex<Vec<Disk>>,
    refreshing: AtomicBool,
}

impl<DC, DM> Default for DisksHandler<DC, DM>
where
    DC: DisksCache + Send,
    DM: DisksManager + Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<DC, DM> DisksHandler<DC, DM>
where
    DC: DisksCache + Send,
    DM: DisksManager + Send + Sync,
{
    pub fn new() -> Self {
        Self {
            disks: Mutex::new(DC::new()),
            disk_manager: DM::new(),
            local_cache: Mutex::new(Vec::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut disks = self.disks.lock();
        disks.refresh();
        *self.local_cache.lock() = disks.cached_entries().to_vec();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed disk information in {:?}", start.elapsed());
        });
    }
}
