/* crates/mc-ui/src/perf/mem_composition.rs
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

//! Disjoint physical composition: Other, Cache, Shared, Buff and Free.

use egui::{Pos2, Rect, Sense, Ui};
use mc_core::format::{DataType, to_human_readable_nice};
use mc_core::i18n::{i18n, i18n_f, n_};
use mc_magpie::Memory;

use crate::win10::{paint, palette};

#[derive(Default)]
pub struct MemComposition {
    total: u64,
    // Draw order; Free, Shared, Buff and Cache are clamped before Other is derived.
    bytes: [u64; 5],
    tooltips: [String; 5],
}

impl MemComposition {
    pub fn update(&mut self, mem: &Memory) {
        self.total = mem.mem_total;
        if self.total == 0 {
            self.bytes = [0; 5];
            self.tooltips = std::array::from_fn(|_| {
                i18n(n_(
                    "Memory composition unavailable\n\nTotal memory is unknown.",
                ))
            });
            return;
        }

        let accounting = mc_core::memory::MemoryUsage::from_memory(mem);
        self.bytes = [
            accounting.other,
            accounting.cache,
            accounting.shared,
            accounting.buffers,
            accounting.free,
        ];
        let adjusted = accounting.adjusted;
        let values = self.bytes.map(|v| {
            if v < 1024 {
                i18n_f(n_("{} B"), &[&v.to_string()])
            } else {
                to_human_readable_nice(v as f32, &DataType::MemoryBytes)
            }
        });
        let overview = i18n_f(
            n_("Other: {}\nCache: {}\nShared: {}\nBuff: {}\nFree: {}"),
            &[&values[0], &values[1], &values[2], &values[3], &values[4]],
        );
        self.tooltips = [
            i18n_f(
                n_(
                    "Other ({})\n\nPrograms, kernel and other memory outside Cache, Shared and Buff. All four non-free sections together equal physically occupied RAM (MemTotal minus MemFree), not the primary In use value. MemAvailable is an estimate, not a section of this bar.",
                ),
                &[&values[0]],
            ),
            i18n_f(
                n_(
                    "Cache ({})\n\nFile cache excluding Shared, plus reclaimable slab: max(Cached - Shmem, 0) + SReclaimable. This is not the raw Cached counter.",
                ),
                &[&values[1]],
            ),
            i18n_f(
                n_(
                    "Shared ({})\n\nShared memory (Shmem), including tmpfs. Counted separately from Cache.",
                ),
                &[&values[2]],
            ),
            i18n_f(
                n_("Buff ({})\n\nKernel buffers (Buffers), accounted for separately from Cache."),
                &[&values[3]],
            ),
            i18n_f(
                n_(
                    "Free ({})\n\nUnoccupied physical memory (MemFree). Cache, including ZFS ARC, is not free memory.",
                ),
                &[&values[4]],
            ),
        ];
        let adjustment = adjusted.then(|| {
            i18n(n_(
                "Adjusted counters: Cached below Shmem is treated as zero before adding SReclaimable; overflow is saturated; Free, Shared, Buff, then Cache are limited to remaining total memory. Values shown are the displayed partition.",
            ))
        });
        for tooltip in &mut self.tooltips {
            tooltip.push_str("\n\n");
            tooltip.push_str(&overview);
            if let Some(adjustment) = &adjustment {
                tooltip.push_str("\n\n");
                tooltip.push_str(adjustment);
            }
        }
    }

    pub fn ui(&self, ui: &mut Ui, rect: Rect, colors: palette::DeviceColors) {
        if rect.width() <= 0. || rect.height() <= 0. || !ui.is_rect_visible(rect) {
            return;
        }
        let clipped = rect.intersect(ui.clip_rect());
        let painter = ui.painter_at(clipped);
        paint::fill(
            &painter,
            rect.left(),
            rect.top(),
            rect.right() - 1.,
            rect.bottom() - 1.,
            palette::WHITE,
        );
        let segments = self.segment_rects(rect);
        for (index, segment) in segments.iter().enumerate() {
            if segment.width() < 1. {
                continue;
            }
            paint::fill(
                &painter,
                segment.left(),
                segment.top(),
                segment.right() - 1.,
                segment.bottom() - 1.,
                if index == 4 {
                    palette::WHITE
                } else {
                    colors.fill
                },
            );
            if segment.left() > rect.left() {
                paint::vline(
                    &painter,
                    segment.left(),
                    rect.top(),
                    rect.bottom() - 1.,
                    colors.line,
                );
            }
        }
        paint::outline(
            &painter,
            rect.left(),
            rect.top(),
            rect.right() - 1.,
            rect.bottom() - 1.,
            colors.line,
        );

        let response = ui.interact(clipped, ui.id().with("memory-composition"), Sense::hover());
        if self.total == 0 {
            let _ = response.on_hover_text_at_pointer(&self.tooltips[0]);
        } else if let Some(pos) = response.hover_pos()
            && let Some(index) = Self::segment_at(&segments, pos)
        {
            let _ = response.on_hover_text_at_pointer(&self.tooltips[index]);
        }
    }

    fn segment_rects(&self, rect: Rect) -> [Rect; 5] {
        let mut cumulative = 0u64;
        let mut left = rect.left();
        std::array::from_fn(|index| {
            cumulative += self.bytes[index];
            let right = if self.total == 0 || cumulative == 0 {
                rect.left()
            } else if cumulative == self.total {
                rect.right()
            } else {
                (rect.left() as f64 + rect.width() as f64 * cumulative as f64 / self.total as f64)
                    .round()
                    .clamp(rect.left() as f64, rect.right() as f64) as f32
            };
            let segment =
                Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(right, rect.bottom()));
            left = right;
            segment
        })
    }

    fn segment_at(segments: &[Rect; 5], pos: Pos2) -> Option<usize> {
        segments.iter().position(|segment| {
            segment.width() >= 1.
                && pos.x >= segment.left()
                && pos.x < segment.right()
                && pos.y >= segment.top()
                && pos.y < segment.bottom()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_partition_removes_shared_overlap_and_adds_reclaimable_slab() {
        let gib = 1024 * 1024 * 1024;
        let mut memory = Memory {
            mem_total: 16 * gib,
            mem_free: 2 * gib,
            mem_available: 9 * gib,
            sh_mem: 2 * gib,
            buffers: gib,
            cached: 6 * gib,
            s_reclaimable: gib,
            dirty: gib,
            ..Default::default()
        };
        let mut composition = MemComposition::default();
        composition.update(&memory);
        assert_eq!(composition.bytes, [6 * gib, 5 * gib, 2 * gib, gib, 2 * gib]);
        assert_eq!(composition.bytes.iter().sum::<u64>(), memory.mem_total);
        assert_ne!(
            composition.bytes[0],
            memory.mem_total - memory.mem_available
        );
        assert_ne!(composition.bytes[1], memory.cached);
        assert!(composition.tooltips[0].starts_with("Other (6.00 GiB)\n\n"));
        assert!(composition.tooltips[1].starts_with("Cache (5.00 GiB)\n\n"));
        assert!(composition.tooltips[4].contains("ZFS ARC"));
        assert!(composition.tooltips.iter().all(|tip| tip.contains(
            "Other: 6.00 GiB\nCache: 5.00 GiB\nShared: 2.00 GiB\nBuff: 1.00 GiB\nFree: 2.00 GiB"
        ) && !tip.contains("Adjusted counters")));

        memory.mem_available = 3 * gib;
        composition.update(&memory);
        assert_eq!(composition.bytes, [6 * gib, 5 * gib, 2 * gib, gib, 2 * gib]);
    }

    #[test]
    fn unknown_and_inconsistent_counters_do_not_invent_memory() {
        let mut composition = MemComposition::default();
        composition.update(&Memory {
            mem_free: 100,
            cached: 100,
            ..Default::default()
        });
        assert_eq!(composition.total, 0);
        assert_eq!(composition.bytes, [0; 5]);
        assert!(composition.tooltips[0].contains("Total memory is unknown."));

        composition.update(&Memory {
            mem_total: 100,
            mem_free: 20,
            sh_mem: 30,
            buffers: 10,
            cached: 5,
            s_reclaimable: 10,
            ..Default::default()
        });
        assert_eq!(composition.bytes, [30, 10, 30, 10, 20]);
        assert!(composition.tooltips[1].starts_with("Cache (10 B)\n\n"));
        assert!(composition.tooltips[1].contains("Cached below Shmem"));

        composition.update(&Memory {
            mem_total: 100,
            mem_free: 20,
            sh_mem: 30,
            buffers: 10,
            cached: u64::MAX,
            s_reclaimable: u64::MAX,
            ..Default::default()
        });
        assert_eq!(composition.bytes, [0, 40, 30, 10, 20]);
        assert!(composition.tooltips[1].starts_with("Cache (40 B)\n\n"));
        assert!(composition.tooltips[1].contains("overflow is saturated"));
        assert!(composition.tooltips[1].contains("Other: 0 B"));

        composition.update(&Memory {
            mem_total: 100,
            mem_free: 20,
            sh_mem: 30,
            buffers: 100,
            cached: 100,
            ..Default::default()
        });
        assert_eq!(composition.bytes, [0, 0, 30, 50, 20]);
        composition.update(&Memory {
            mem_total: 100,
            mem_free: 120,
            sh_mem: 130,
            buffers: 140,
            cached: 150,
            ..Default::default()
        });
        assert_eq!(composition.bytes, [0, 0, 0, 0, 100]);
        assert!(composition.tooltips[4].starts_with("Free (100 B)\n\n"));
        assert!(
            composition.tooltips[4]
                .contains("Other: 0 B\nCache: 0 B\nShared: 0 B\nBuff: 0 B\nFree: 100 B")
        );
    }

    #[test]
    fn cumulative_boundaries_match_hits_and_skip_invisible_segments() {
        let rect = Rect::from_min_max(Pos2::new(10., 20.), Pos2::new(110., 72.));
        let mut composition = MemComposition::default();
        composition.update(&Memory {
            mem_total: 100,
            mem_free: 20,
            sh_mem: 10,
            buffers: 10,
            cached: 40,
            ..Default::default()
        });
        let segments = composition.segment_rects(rect);
        assert_eq!(
            segments.map(|segment| segment.right()),
            [40., 70., 80., 90., 110.]
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(39.9, 30.)),
            Some(0)
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(40., 30.)),
            Some(1)
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(70., 30.)),
            Some(2)
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(80., 30.)),
            Some(3)
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(90., 30.)),
            Some(4)
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(110., 30.)),
            None
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(30., 72.)),
            None
        );

        composition.update(&Memory {
            mem_total: 1000,
            mem_free: 500,
            sh_mem: 1,
            cached: 1,
            ..Default::default()
        });
        let segments = composition.segment_rects(rect);
        assert_eq!(
            segments.map(|segment| segment.right()),
            [60., 60., 60., 60., 110.]
        );
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(60., 30.)),
            Some(4)
        );

        composition.update(&Memory::default());
        let segments = composition.segment_rects(rect);
        assert!(segments.iter().all(|segment| segment.width() == 0.));
        assert_eq!(
            MemComposition::segment_at(&segments, Pos2::new(10., 30.)),
            None
        );
    }
}
