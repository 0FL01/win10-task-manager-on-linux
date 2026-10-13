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
use std::time::{Duration, Instant};

use crate::profile::{self, Domain, Phase, QueueTicket, Span};
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
    /// Bounded, noninteractive read used only by the automatic cache worker.
    fn automatic_smart_data(&self, id: &str) -> Option<DiskSmartData>;
}

const SMART_TTL: Duration = Duration::from_secs(5 * 60);
const SMART_RETRY: Duration = Duration::from_secs(30);

/// Only inventory-observable identity, not a disk-incarnation guarantee.
/// Unknown serial/WWN is never treated as proof of identity for cached SMART.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DiskIdentity {
    serial: Option<String>,
    wwn: Option<String>,
    model: Option<String>,
    kind: Option<i32>,
    interface: i32,
    capacity: u64,
    sector_size: u64,
}

impl DiskIdentity {
    fn observed(disk: &Disk) -> Option<Self> {
        let interface = disk.smart_interface?;
        DiskSmartInterfaceKind::try_from(interface).ok()?;
        let known = |value: &Option<String>| {
            value
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
        };
        let serial = known(&disk.serial_number);
        let wwn = known(&disk.world_wide_name);
        if serial.is_none() && wwn.is_none() {
            return None;
        }
        Some(Self {
            serial,
            wwn,
            model: known(&disk.model),
            kind: disk.kind,
            interface,
            capacity: disk.capacity_bytes,
            sector_size: disk.sector_size,
        })
    }
}

struct SmartEntry {
    identity: DiskIdentity,
    data: Option<DiskSmartData>,
    retry_at: Option<Instant>,
}

struct SmartJob {
    id: String,
    identity: DiskIdentity,
    generation: u64,
}

#[derive(Default)]
struct DiskSnapshotCache {
    disks: Vec<Disk>,
    smart: HashMap<String, SmartEntry>,
    generation: u64,
    running: bool,
    cursor: Option<String>,
}

impl DiskSnapshotCache {
    fn observe(&mut self, disks: Vec<Disk>) {
        let inventory: HashMap<_, _> = disks
            .iter()
            .filter_map(|disk| {
                DiskIdentity::observed(disk).map(|identity| (disk.id.clone(), identity))
            })
            .collect();
        if inventory.len() != self.smart.len()
            || inventory.iter().any(|(id, identity)| {
                self.smart
                    .get(id)
                    .is_none_or(|entry| &entry.identity != identity)
            })
        {
            // A disappearance/reappearance invalidates pending results even when
            // the same observed identity returns. No extra incarnation registry.
            self.generation += 1;
            self.smart
                .retain(|id, entry| inventory.get(id) == Some(&entry.identity));
            for (id, identity) in inventory {
                self.smart.entry(id).or_insert(SmartEntry {
                    identity,
                    data: None,
                    retry_at: None,
                });
            }
        }
        self.disks = disks;
    }

    fn snapshot(&self, now: Instant) -> DiskList {
        DiskList {
            disks: self.disks.clone(),
            smart_data: self
                .smart
                .iter()
                .filter_map(|(id, entry)| {
                    if entry.retry_at.is_some_and(|expires| now < expires) {
                        entry.data.clone().map(|data| (id.clone(), data))
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    fn next_job(&mut self, now: Instant) -> Option<SmartJob> {
        if self.running {
            return None;
        }
        let mut due: Vec<_> = self
            .smart
            .iter()
            .filter(|(_, entry)| entry.retry_at.is_none_or(|retry| now >= retry))
            .map(|(id, _)| id.clone())
            .collect();
        due.sort_unstable();
        let id = due
            .iter()
            .find(|id| self.cursor.as_ref().is_none_or(|cursor| *id > cursor))
            .or_else(|| due.first())?
            .clone();
        self.cursor = Some(id.clone());
        self.running = true; // Reserve before queuing, not when the worker starts.
        Some(SmartJob {
            identity: self.smart[&id].identity.clone(),
            id,
            generation: self.generation,
        })
    }

    fn complete(&mut self, job: SmartJob, data: Option<DiskSmartData>, now: Instant) {
        self.running = false;
        if self.generation != job.generation {
            return;
        }
        if let Some(entry) = self.smart.get_mut(&job.id) {
            if entry.identity == job.identity {
                entry.retry_at = Some(
                    now + if data.is_some() {
                        SMART_TTL
                    } else {
                        SMART_RETRY
                    },
                );
                entry.data = data;
            }
        }
    }
}

pub struct DisksHandler<DC, DM>
where
    DC: DisksCache,
    DM: DisksManager,
{
    pub(crate) disks: Mutex<DC>,
    pub(crate) disk_manager: DM,
    local_cache: Mutex<DiskSnapshotCache>,
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
        let _profile = Span::new(Phase::Init(Domain::Disks));
        Self {
            disks: Mutex::new(DC::new()),
            disk_manager: DM::new(),
            local_cache: Mutex::new(DiskSnapshotCache::default()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let refresh = Span::new(Phase::Refresh(Domain::Disks));
        let mut disks = self.disks.lock();
        disks.refresh();
        drop(refresh);
        let mut publish = Span::new(Phase::Publish(Domain::Disks));
        publish.items(disks.cached_entries().len());
        self.local_cache
            .lock()
            .observe(disks.cached_entries().to_vec());
    }

    pub fn snapshot(&self) -> DiskList {
        self.local_cache.lock().snapshot(Instant::now())
    }

    pub fn refresh_smart_async(&'static self, thread_pool: &ThreadPool) {
        let Some(job) = self.local_cache.lock().next_job(Instant::now()) else {
            return;
        };
        thread_pool.spawn(move || {
            // No inventory/cache locks across any UDisks work. Manual reads use
            // their existing separate path and may overlap this automatic job.
            let data = self.disk_manager.automatic_smart_data(&job.id);
            let completed = Instant::now();
            self.local_cache.lock().complete(job, data, completed);
        });
    }

    pub fn refresh_async(&'static self, thread_pool: &'static ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            profile::coalesced(Phase::Refresh(Domain::Disks));
            return;
        }

        let queued = QueueTicket::new(Domain::Disks);
        thread_pool.spawn(move || {
            queued.arrived();
            let start = std::time::Instant::now();

            self.refresh();
            self.refresh_smart_async(thread_pool);
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed disk information in {:?}", start.elapsed());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

    fn disk(id: &str) -> Disk {
        Disk {
            id: id.into(),
            serial_number: Some(format!("serial-{id}")),
            smart_interface: Some(DiskSmartInterfaceKind::Ata as i32),
            capacity_bytes: 1000,
            sector_size: 512,
            ..Default::default()
        }
    }

    #[test]
    fn smart_ttl_and_failure_backoff_start_at_completion() {
        let mut cache = DiskSnapshotCache::default();
        cache.observe(vec![disk("a")]);
        let start = Instant::now();
        let job = cache.next_job(start).unwrap();
        assert!(cache.next_job(start + SMART_TTL).is_none());
        let completed = start + SMART_TTL;
        cache.complete(job, Some(DiskSmartData::default()), completed);
        assert_eq!(
            cache
                .snapshot(completed + SMART_TTL - Duration::from_secs(1))
                .smart_data
                .len(),
            1
        );
        assert!(cache
            .next_job(completed + SMART_TTL - Duration::from_secs(1))
            .is_none());
        assert!(cache.snapshot(completed + SMART_TTL).smart_data.is_empty());
        let job = cache.next_job(completed + SMART_TTL).unwrap();
        let failed = completed + SMART_TTL + Duration::from_secs(8);
        cache.complete(job, None, failed);
        assert!(cache.snapshot(failed).smart_data.is_empty());
        assert!(cache
            .next_job(failed + SMART_RETRY - Duration::from_secs(1))
            .is_none());
        assert!(cache.next_job(failed + SMART_RETRY).is_some());
    }

    #[test]
    fn round_robin_does_not_starve_later_disks_when_first_keeps_failing() {
        let mut cache = DiskSnapshotCache::default();
        cache.observe(vec![disk("c"), disk("a"), disk("b")]);
        let mut now = Instant::now();
        for expected in ["a", "b", "c", "a", "b", "c"] {
            let job = cache.next_job(now).unwrap();
            assert_eq!(job.id, expected);
            assert!(cache.next_job(now).is_none());
            cache.complete(job, None, now);
            // All previous failures are eligible again, including disk a.
            now += SMART_RETRY;
        }
    }

    #[test]
    fn removed_and_observably_replaced_disks_reject_pending_results() {
        let mut cache = DiskSnapshotCache::default();
        let now = Instant::now();
        let original = disk("a");
        cache.observe(vec![original.clone()]);
        let removed_job = cache.next_job(now).unwrap();
        cache.observe(vec![]);
        cache.observe(vec![original.clone()]);
        cache.complete(removed_job, Some(DiskSmartData::default()), now);
        assert!(cache.snapshot(now).smart_data.is_empty());

        let replaced_job = cache.next_job(now).unwrap();
        let mut replacement = original.clone();
        replacement.serial_number = Some("replacement".into());
        cache.observe(vec![replacement]);
        cache.complete(replaced_job, Some(DiskSmartData::default()), now);
        assert!(cache.snapshot(now).smart_data.is_empty());

        let job = cache.next_job(now).unwrap();
        cache.complete(job, Some(DiskSmartData::default()), now);
        assert_eq!(cache.snapshot(now).smart_data.len(), 1);
        cache.observe(vec![original]);
        assert!(cache.snapshot(now).smart_data.is_empty());
        cache.observe(vec![]);
        assert!(
            cache.smart.is_empty(),
            "absent disks and their backoff must be pruned"
        );
    }

    #[test]
    fn unknown_identity_is_unavailable_and_dynamic_statistics_do_not_invalidate() {
        let mut cache = DiskSnapshotCache::default();
        let now = Instant::now();
        let mut unknown = disk("a");
        unknown.serial_number = Some("  ".into());
        cache.observe(vec![unknown.clone()]);
        assert!(cache.next_job(now).is_none());
        assert!(cache.snapshot(now).smart_data.is_empty());

        unknown.world_wide_name = Some("wwn".into());
        cache.observe(vec![unknown.clone()]);
        let job = cache.next_job(now).unwrap();
        unknown.busy_percent = 25.0;
        unknown.temperature_milli_k = Some(300_000);
        cache.observe(vec![unknown.clone()]);
        cache.complete(job, Some(DiskSmartData::default()), now);
        assert_eq!(cache.snapshot(now).smart_data.len(), 1);
        unknown.smart_interface = None;
        cache.observe(vec![unknown]);
        assert!(cache.next_job(now).is_none());
        assert!(cache.snapshot(now).smart_data.is_empty());
    }

    struct TestInventory(Vec<Disk>);

    impl DisksCache for TestInventory {
        fn new() -> Self {
            Self(vec![disk("a")])
        }
        fn refresh(&mut self) {}
        fn cached_entries(&self) -> &[Disk] {
            &self.0
        }
    }

    struct BlockingManager {
        started: SyncSender<String>,
        release: Mutex<Receiver<()>>,
        calls: AtomicUsize,
    }

    impl DisksManager for BlockingManager {
        fn new() -> Self {
            unreachable!()
        }
        fn eject(
            &self,
            _: &str,
            _: &Mutex<HashMap<u32, Process>>,
        ) -> Result<(), DisksResponseErrorEjectFailed> {
            Ok(())
        }
        fn smart_data(&self, _: &str) -> Option<DiskSmartData> {
            Some(DiskSmartData::default())
        }
        fn automatic_smart_data(&self, id: &str) -> Option<DiskSmartData> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.send(id.into()).unwrap();
            self.release
                .lock()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Some(DiskSmartData::default())
        }
    }

    #[test]
    fn slow_smart_does_not_lock_snapshot_inventory_or_manual_reads_and_only_one_is_queued() {
        let (started_tx, started) = sync_channel(1);
        let (release, release_rx) = sync_channel(1);
        let handler = Box::leak(Box::new(DisksHandler {
            disks: Mutex::new(TestInventory::new()),
            disk_manager: BlockingManager {
                started: started_tx,
                release: Mutex::new(release_rx),
                calls: AtomicUsize::new(0),
            },
            local_cache: Mutex::new(DiskSnapshotCache::default()),
            refreshing: AtomicBool::new(false),
        }));
        let handler: &'static DisksHandler<TestInventory, BlockingManager> = handler;
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let (occupied_tx, occupied) = sync_channel(1);
        let (unblock, blocked) = sync_channel(1);
        pool.spawn(move || {
            occupied_tx.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        occupied.recv_timeout(Duration::from_secs(5)).unwrap();
        handler.refresh();
        for _ in 0..10 {
            handler.refresh_smart_async(&pool);
        }
        assert!(handler.local_cache.lock().running);
        assert_eq!(handler.disk_manager.calls.load(Ordering::SeqCst), 0);
        unblock.send(()).unwrap();
        assert_eq!(started.recv_timeout(Duration::from_secs(5)).unwrap(), "a");
        for _ in 0..10 {
            handler.refresh_smart_async(&pool);
        }

        let (responsive_tx, responsive) = sync_channel(1);
        let reader = std::thread::spawn(move || {
            handler.refresh();
            let snapshot = handler.snapshot();
            let manual = handler.disk_manager.smart_data("a");
            responsive_tx.send((snapshot, manual)).unwrap();
        });
        let response = responsive.recv_timeout(Duration::from_secs(5));
        // Release even if the responsiveness assertion will fail.
        release.send(()).unwrap();
        let (snapshot, manual) = response.expect("disk intake must not wait for SMART");
        assert_eq!(snapshot.disks.len(), 1);
        assert!(snapshot.smart_data.is_empty());
        assert!(manual.is_some());
        reader.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while handler.local_cache.lock().running {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(handler.disk_manager.calls.load(Ordering::SeqCst), 1);
        assert_eq!(handler.snapshot().smart_data.len(), 1);
    }
}
