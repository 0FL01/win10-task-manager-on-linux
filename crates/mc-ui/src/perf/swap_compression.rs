/* crates/mc-ui/src/perf/swap_compression.rs
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

//! Logical swap usage plus one row per active compression layer (zram swap, zswap pool).
//! Everything absent is omitted, so a system without swap gets no block at all.

use egui::{Align, Pos2, Rect, Sense, Ui};
use mc_core::format::{DataType, to_human_readable_nice};
use mc_core::i18n::{i18n, i18n_f, n_};
use mc_magpie::Memory;

use crate::win10::paint::{self, Font};
use crate::win10::{metrics as m, palette};

/// Smaller compressed volumes are noise and would make rows flicker in and out.
const MIN_COMPRESSION_THRESHOLD: u64 = 1024 * 1024;
/// Reference gap between a key/value label and its value
const VALUE_DX: f32 = 109.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Zram,
    Zswap,
}

struct SwapBar {
    total: u64,
    used: u64,
    caption: String,
    tooltip: String,
}

struct Row {
    layer: Layer,
    label: String,
    ratio: String,
    detail: String,
    tooltip: String,
}

#[derive(Default)]
pub struct SwapCompression {
    swap: Option<SwapBar>,
    rows: Vec<Row>,
}

fn bytes(v: u64) -> String {
    to_human_readable_nice(v as f32, &DataType::MemoryBytes)
}

/// `stored / ram` as "3.6×"; "—" when nothing backs the data.
fn ratio(stored: u64, ram: u64) -> String {
    if ram == 0 {
        "—".into()
    } else {
        i18n_f(n_("{}×"), &[&format!("{:.1}", stored as f64 / ram as f64)])
    }
}

/// zram devices active as swap: `(stored, compressed, RAM used)`. Collectors without the swap
/// list (older fixtures) only report all zram devices together.
fn zram_swap(mem: &Memory) -> (u64, u64, u64) {
    if mem.swap_devices.is_empty() {
        return (mem.zram_orig_size, mem.zram_compr_size, mem.zram_mem_used);
    }
    mem.swap_devices
        .iter()
        .filter_map(|device| device.zram.as_ref())
        .fold((0, 0, 0), |(o, c, u), z| {
            (
                o.saturating_add(z.orig_data_size),
                c.saturating_add(z.compr_data_size),
                u.saturating_add(z.mem_used_total),
            )
        })
}

impl SwapCompression {
    pub fn update(&mut self, mem: &Memory) {
        self.rows.clear();
        if mem.swap_total == 0 {
            self.swap = None;
            return;
        }

        let used = mem.swap_total.saturating_sub(mem.swap_free);
        let percent = (used as f64 / mem.swap_total as f64 * 100.).round();
        let mut tooltip = i18n_f(
            n_(
                "Swap usage ({} of {})\n\nLogical swap slots in use, including pages held compressed in zram and zswap. This is not the amount written to disk.",
            ),
            &[&bytes(used), &bytes(mem.swap_total)],
        );
        if !mem.swap_devices.is_empty() {
            tooltip.push('\n');
            for device in &mem.swap_devices {
                tooltip.push('\n');
                tooltip.push_str(&i18n_f(
                    n_("{}: {} of {}, priority {}"),
                    &[
                        &device.name,
                        &bytes(device.used),
                        &bytes(device.size),
                        &device.priority.to_string(),
                    ],
                ));
            }
        }
        self.swap = Some(SwapBar {
            total: mem.swap_total,
            used: used.min(mem.swap_total),
            caption: format!("{} / {} ({}%)", bytes(used), bytes(mem.swap_total), percent),
            tooltip,
        });

        let (stored, compressed, ram) = zram_swap(mem);
        if stored >= MIN_COMPRESSION_THRESHOLD {
            let ratio = ratio(stored, ram);
            self.rows.push(Row {
                layer: Layer::Zram,
                label: i18n("zram · RAM compression"),
                detail: i18n_f(n_("{} in {} RAM"), &[&bytes(stored), &bytes(ram)]),
                tooltip: i18n_f(
                    n_(
                        "zram ({})\n\nCompressed RAM block devices used as swap.\nStored: {}\nCompressed: {}\nRAM used: {}\nSaved: {}\n\nThe ratio is stored data divided by RAM used (mem_used_total), which includes allocator overhead and metadata.",
                    ),
                    &[
                        &ratio,
                        &bytes(stored),
                        &bytes(compressed),
                        &bytes(ram),
                        &bytes(stored.saturating_sub(ram)),
                    ],
                ),
                ratio,
            });
        }

        if mem.zswapped >= MIN_COMPRESSION_THRESHOLD {
            let ratio = ratio(mem.zswapped, mem.zswap);
            let mut tooltip = i18n_f(
                n_(
                    "zswap ({})\n\nCompressed RAM cache for pages on their way to swap; a layer in front of swap, not an extra swap device.\nStored: {}\nRAM used: {}\nSaved: {}",
                ),
                &[
                    &ratio,
                    &bytes(mem.zswapped),
                    &bytes(mem.zswap),
                    &bytes(mem.zswapped.saturating_sub(mem.zswap)),
                ],
            );
            if mem.zswap_enabled == Some(false) {
                tooltip.push_str("\n\n");
                tooltip.push_str(&i18n(n_(
                    "zswap is disabled; the remaining pool drains as pages are read back or written to swap.",
                )));
            }
            self.rows.push(Row {
                layer: Layer::Zswap,
                label: i18n("zswap · Compressed cache"),
                detail: i18n_f(
                    n_("{} in {} RAM"),
                    &[&bytes(mem.zswapped), &bytes(mem.zswap)],
                ),
                tooltip,
                ratio,
            });
        }
    }

    /// Height below the caption; zero when there is no swap and the block is omitted.
    pub fn height(&self) -> f32 {
        if self.swap.is_some() {
            m::MEMORY_SWAP_BAR_H + self.rows.len() as f32 * m::MEMORY_SWAP_ROW_H
        } else {
            0.
        }
    }

    /// "used / total (percent%)" for the right side of the caption.
    pub fn caption(&self) -> &str {
        self.swap.as_ref().map_or("", |swap| &swap.caption)
    }

    /// "3.6× (6.6 GiB in 1.8 GiB RAM)" for the copied page summary.
    pub fn summary(&self, layer: Layer) -> Option<String> {
        self.rows
            .iter()
            .find(|row| row.layer == layer)
            .map(|row| format!("{} ({})", row.ratio, row.detail))
    }

    pub fn ui(&self, ui: &mut Ui, rect: Rect, colors: palette::DeviceColors) {
        let Some(swap) = &self.swap else {
            return;
        };
        if rect.width() <= 0. || !ui.is_rect_visible(rect) {
            return;
        }
        let clip = rect.intersect(ui.clip_rect());
        let painter = ui.painter_at(clip);

        let bar = Rect::from_min_size(rect.min, egui::vec2(rect.width(), m::MEMORY_SWAP_BAR_H));
        let (x0, y0, x1, y1) = (bar.left(), bar.top(), bar.right() - 1., bar.bottom() - 1.);
        paint::fill(&painter, x0, y0, x1, y1, palette::WHITE);
        let split = Self::split_x(bar, swap.used, swap.total);
        if split > bar.left() {
            paint::fill(&painter, x0, y0, split - 1., y1, colors.fill);
            if split < bar.right() {
                paint::vline(&painter, split, y0, y1, colors.line);
            }
        }
        paint::outline(&painter, x0, y0, x1, y1, colors.line);
        let response = ui.interact(
            bar.intersect(clip),
            ui.id().with("memory-swap-bar"),
            Sense::hover(),
        );
        let _ = response.on_hover_text_at_pointer(&swap.tooltip);

        let label_w = self
            .rows
            .iter()
            .map(|row| paint::text_width(&painter, &row.label, Font::Body))
            .fold(0., f32::max);
        let ratio_x = rect.left() + VALUE_DX.max((label_w + 12.).round());
        let ratio_w = self
            .rows
            .iter()
            .map(|row| paint::text_width(&painter, &row.ratio, Font::Body))
            .fold(0., f32::max);
        let detail_x = (ratio_x + ratio_w + 12.).round();
        for (index, row) in self.rows.iter().enumerate() {
            let top = bar.bottom() + index as f32 * m::MEMORY_SWAP_ROW_H;
            let cap = top + m::MEMORY_SWAP_ROW_CAP;
            paint::text(
                &painter,
                rect.left(),
                cap,
                &row.label,
                Font::Body,
                palette::LABEL,
                Align::Min,
            );
            paint::text(
                &painter,
                ratio_x,
                cap,
                &row.ratio,
                Font::Body,
                palette::TEXT,
                Align::Min,
            );
            let detail = paint::elide(&painter, &row.detail, Font::Body, rect.right() - detail_x);
            paint::text(
                &painter,
                detail_x,
                cap,
                &detail,
                Font::Body,
                palette::TEXT,
                Align::Min,
            );
            let band = Rect::from_min_max(
                Pos2::new(rect.left(), top),
                Pos2::new(rect.right(), top + m::MEMORY_SWAP_ROW_H),
            );
            let response = ui.interact(
                band.intersect(clip),
                ui.id().with(("memory-compression", index)),
                Sense::hover(),
            );
            let _ = response.on_hover_text_at_pointer(&row.tooltip);
        }
    }

    /// Right edge of the used part; zero usage has no artificial width.
    fn split_x(bar: Rect, used: u64, total: u64) -> f32 {
        if total == 0 || used == 0 {
            bar.left()
        } else if used >= total {
            bar.right()
        } else {
            (bar.left() as f64 + bar.width() as f64 * used as f64 / total as f64)
                .round()
                .clamp(bar.left() as f64, bar.right() as f64) as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_magpie::magpie_types::memory::{SwapDevice, ZramStats};

    const GIB: u64 = 1024 * 1024 * 1024;

    fn swapfile() -> SwapDevice {
        SwapDevice {
            name: "/swapfile".into(),
            size: 32 * GIB,
            used: GIB,
            priority: -2,
            zram: None,
        }
    }

    fn zram(orig: u64, used: u64) -> SwapDevice {
        SwapDevice {
            name: "/dev/zram0".into(),
            size: 16 * GIB,
            used: orig,
            priority: 100,
            zram: Some(ZramStats {
                orig_data_size: orig,
                compr_data_size: used / 2,
                mem_used_total: used,
            }),
        }
    }

    fn layers(block: &SwapCompression) -> Vec<Layer> {
        block.rows.iter().map(|row| row.layer).collect()
    }

    #[test]
    fn no_swap_omits_the_whole_block_even_with_stale_counters() {
        let mut block = SwapCompression::default();
        block.update(&Memory {
            zram_orig_size: 4 * GIB,
            zram_mem_used: GIB,
            zswapped: GIB,
            zswap: GIB / 2,
            ..Default::default()
        });
        assert!(block.swap.is_none());
        assert!(block.rows.is_empty());
        assert_eq!(block.height(), 0.);
        assert_eq!(block.caption(), "");
    }

    #[test]
    fn plain_swap_shows_only_the_bar() {
        let mut block = SwapCompression::default();
        block.update(&Memory {
            swap_total: 8 * GIB,
            swap_free: 6 * GIB,
            swap_devices: vec![swapfile()],
            zswap_enabled: Some(true),
            ..Default::default()
        });
        assert!(layers(&block).is_empty());
        assert_eq!(block.height(), m::MEMORY_SWAP_BAR_H);
        assert_eq!(block.caption(), "2.00 GiB / 8.00 GiB (25%)");
        let tooltip = &block.swap.as_ref().unwrap().tooltip;
        assert!(tooltip.contains("/swapfile: 1.00 GiB of 32.0 GiB, priority -2"));
    }

    #[test]
    fn zram_swap_without_zswap_shows_one_row_with_ram_based_ratio() {
        let mut block = SwapCompression::default();
        block.update(&Memory {
            swap_total: 48 * GIB,
            swap_free: 40 * GIB,
            swap_devices: vec![swapfile(), zram(7 * GIB, 2 * GIB)],
            zswap_enabled: Some(false),
            ..Default::default()
        });
        assert_eq!(layers(&block), [Layer::Zram]);
        assert_eq!(block.height(), m::MEMORY_SWAP_BAR_H + m::MEMORY_SWAP_ROW_H);
        let row = &block.rows[0];
        assert_eq!(row.ratio, "3.5×");
        assert_eq!(row.detail, "7.00 GiB in 2.00 GiB RAM");
        assert!(row.tooltip.contains("Saved: 5.00 GiB"));
        assert!(row.tooltip.contains("Compressed: 1.00 GiB"));
        assert_eq!(
            block.summary(Layer::Zram).as_deref(),
            Some("3.5× (7.00 GiB in 2.00 GiB RAM)")
        );
        assert_eq!(block.summary(Layer::Zswap), None);
    }

    #[test]
    fn zram_that_is_not_swap_is_not_shown() {
        let mut block = SwapCompression::default();
        // The aggregate includes a zram /tmp; the swap list has no zram device.
        block.update(&Memory {
            swap_total: 8 * GIB,
            swap_free: 8 * GIB,
            swap_devices: vec![swapfile()],
            zram_orig_size: 4 * GIB,
            zram_mem_used: GIB,
            ..Default::default()
        });
        assert!(layers(&block).is_empty());
    }

    #[test]
    fn collectors_without_a_swap_list_fall_back_to_all_zram() {
        let mut block = SwapCompression::default();
        block.update(&Memory {
            swap_total: 8 * GIB,
            swap_free: 4 * GIB,
            zram_orig_size: 4 * GIB,
            zram_compr_size: GIB,
            zram_mem_used: GIB,
            ..Default::default()
        });
        assert_eq!(layers(&block), [Layer::Zram]);
        assert_eq!(block.rows[0].ratio, "4.0×");
    }

    #[test]
    fn zswap_rows_follow_the_pool_not_the_flag() {
        let mut block = SwapCompression::default();
        let mut memory = Memory {
            swap_total: 8 * GIB,
            swap_free: 7 * GIB,
            swap_devices: vec![swapfile(), zram(3 * GIB, GIB)],
            zswap_enabled: Some(true),
            ..Default::default()
        };
        block.update(&memory);
        assert_eq!(layers(&block), [Layer::Zram], "enabled but empty pool");

        memory.zswapped = 3 * GIB;
        memory.zswap = GIB;
        block.update(&memory);
        assert_eq!(layers(&block), [Layer::Zram, Layer::Zswap]);
        assert_eq!(block.rows[1].ratio, "3.0×");
        assert!(!block.rows[1].tooltip.contains("disabled"));

        memory.zswap_enabled = Some(false);
        block.update(&memory);
        assert_eq!(layers(&block), [Layer::Zram, Layer::Zswap], "draining");
        assert!(block.rows[1].tooltip.contains("zswap is disabled"));

        memory.zswapped = 0;
        memory.zswap = 0;
        block.update(&memory);
        assert_eq!(layers(&block), [Layer::Zram], "disabled and drained");
    }

    #[test]
    fn tiny_volumes_are_hidden_and_zero_ram_has_no_ratio() {
        let mut block = SwapCompression::default();
        block.update(&Memory {
            swap_total: 8 * GIB,
            swap_free: 8 * GIB,
            swap_devices: vec![zram(MIN_COMPRESSION_THRESHOLD - 1, 1)],
            zswapped: MIN_COMPRESSION_THRESHOLD,
            zswap: 0,
            ..Default::default()
        });
        assert_eq!(layers(&block), [Layer::Zswap]);
        assert_eq!(block.rows[0].ratio, "—");
    }

    #[test]
    fn bar_split_is_proportional_without_artificial_width() {
        let bar = Rect::from_min_max(Pos2::new(10., 0.), Pos2::new(110., 20.));
        assert_eq!(SwapCompression::split_x(bar, 0, 100), 10.);
        assert_eq!(SwapCompression::split_x(bar, 25, 100), 35.);
        assert_eq!(SwapCompression::split_x(bar, 100, 100), 110.);
        assert_eq!(SwapCompression::split_x(bar, 200, 100), 110.);
        assert_eq!(SwapCompression::split_x(bar, 1, 0), 10.);
    }
}
