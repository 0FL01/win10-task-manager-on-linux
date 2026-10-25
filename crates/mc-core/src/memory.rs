/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Available-based RAM usage and a separate, disjoint physical composition.
use mc_magpie::Memory;

#[derive(Debug, PartialEq, Eq)]
pub struct MemoryUsage {
    pub total: u64,
    pub free: u64,
    /// Memory available without swapping, falling back to bounded MemFree if invalid.
    pub available: u64,
    /// Headline RAM usage: total minus available memory.
    pub in_use: u64,
    pub occupied: u64,
    /// Disjoint components of occupied RAM, in composition-bar order.
    pub other: u64,
    pub cache: u64,
    pub shared: u64,
    pub buffers: u64,
    pub adjusted: bool,
}

impl MemoryUsage {
    pub fn from_memory(mem: &Memory) -> Self {
        let free = mem.mem_free.min(mem.mem_total);
        let available = if mem.mem_available <= mem.mem_total {
            mem.mem_available
        } else {
            free
        };
        let occupied = mem.mem_total - free;
        let shared = mem.sh_mem.min(occupied);
        let buffers = mem.buffers.min(occupied - shared);
        let raw_cache = mem
            .cached
            .saturating_sub(mem.sh_mem)
            .saturating_add(mem.s_reclaimable);
        let cache = raw_cache.min(occupied - shared - buffers);
        Self {
            total: mem.mem_total,
            free,
            available,
            in_use: mem.mem_total - available,
            occupied,
            other: occupied - shared - buffers - cache,
            cache,
            shared,
            buffers,
            adjusted: mem.cached < mem.sh_mem
                || mem
                    .cached
                    .saturating_sub(mem.sh_mem)
                    .checked_add(mem.s_reclaimable)
                    .is_none()
                || free != mem.mem_free
                || shared != mem.sh_mem
                || buffers != mem.buffers
                || cache != raw_cache,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_use_tracks_available_while_occupied_includes_cache() {
        let mut mem = Memory {
            mem_total: 3200,
            mem_free: 1600,
            mem_available: 2600,
            cached: 950,
            sh_mem: 20,
            s_reclaimable: 50,
            buffers: 10,
            ..Default::default()
        };
        let usage = MemoryUsage::from_memory(&mem);
        assert_eq!(usage.available, 2600);
        assert_eq!(usage.in_use, 600);
        assert_eq!(usage.in_use + usage.available, usage.total);
        assert_eq!(usage.occupied, 1600);
        assert_eq!(usage.cache, 980);
        assert_eq!(
            usage.other + usage.cache + usage.shared + usage.buffers,
            usage.occupied
        );
        assert_eq!(usage.occupied + usage.free, usage.total);
        mem.mem_available = 3000;
        assert_eq!(
            MemoryUsage::from_memory(&mem),
            MemoryUsage {
                available: 3000,
                in_use: 200,
                ..usage
            }
        );
    }

    #[test]
    fn inconsistent_or_unknown_counters_remain_a_bounded_partition() {
        for (total, free, cached, available) in [
            (0, 42, 100, 1),
            (100, 150, 200, 101),
            (100, 20, u64::MAX, 101),
        ] {
            let mem = Memory {
                mem_total: total,
                mem_free: free,
                mem_available: available,
                cached,
                sh_mem: 30,
                buffers: 40,
                s_reclaimable: u64::MAX,
                ..Default::default()
            };
            let u = MemoryUsage::from_memory(&mem);
            assert_eq!(u.other + u.cache + u.shared + u.buffers + u.free, total);
            assert_eq!(u.occupied + u.free, total);
            assert_eq!(u.available, u.free);
            assert_eq!(u.in_use, u.occupied);
            assert_eq!(u.in_use + u.available, total);
            assert!(u.adjusted);
        }
    }

    #[test]
    fn full_range_counters_preserve_partition_without_overflow() {
        let counters = [0, 1, 100, u64::MAX];
        for total in counters {
            for free in counters {
                for cached in counters {
                    for shared in counters {
                        let mem = Memory {
                            mem_total: total,
                            mem_free: free,
                            cached,
                            sh_mem: shared,
                            buffers: 40,
                            s_reclaimable: u64::MAX,
                            ..Default::default()
                        };
                        let u = MemoryUsage::from_memory(&mem);
                        let sum = [u.other, u.cache, u.shared, u.buffers, u.free]
                            .into_iter()
                            .try_fold(0_u64, u64::checked_add);
                        assert_eq!(sum, Some(total));
                        assert_eq!(u.occupied.checked_add(u.free), Some(total));
                        assert_eq!(u.occupied, total - free.min(total));
                        assert_eq!(u.in_use.checked_add(u.available), Some(total));
                    }
                }
            }
        }
    }
}
