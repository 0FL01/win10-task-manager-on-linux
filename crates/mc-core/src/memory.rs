/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Physical RAM accounting: everything except MemFree is occupied, including cache.
use mc_magpie::Memory;

#[derive(Debug, PartialEq, Eq)]
pub struct MemoryUsage {
    pub total: u64,
    pub free: u64,
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
    fn occupied_includes_cache_and_does_not_depend_on_available() {
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
        assert_eq!(usage.occupied, 1600);
        assert_eq!(usage.cache, 980);
        assert_eq!(
            usage.other + usage.cache + usage.shared + usage.buffers,
            usage.occupied
        );
        assert_eq!(usage.occupied + usage.free, usage.total);
        mem.mem_available = 3000;
        assert_eq!(MemoryUsage::from_memory(&mem), usage);
    }

    #[test]
    fn inconsistent_or_unknown_counters_remain_a_bounded_partition() {
        for (total, free, cached) in [(0, 42, 100), (100, 150, 200), (100, 20, u64::MAX)] {
            let mem = Memory {
                mem_total: total,
                mem_free: free,
                cached,
                sh_mem: 30,
                buffers: 40,
                s_reclaimable: u64::MAX,
                ..Default::default()
            };
            let u = MemoryUsage::from_memory(&mem);
            assert_eq!(u.other + u.cache + u.shared + u.buffers + u.free, total);
            assert_eq!(u.occupied + u.free, total);
            assert!(u.adjusted);
        }
    }
}
