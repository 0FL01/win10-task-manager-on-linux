/* core/src/lib.rs
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

//! The part of the graphing logic that has no opinion about drawing.
//!
//! This must be kept toolkit agnostic.

pub mod animation;
pub mod dataset;
pub mod scaling;

pub use animation::AnimationFrame;
pub use dataset::{Dataset, DatasetGroup, DatasetPoints, DatasetSettings};
pub use scaling::{FillingSettings, RoundingSettings, ScalingSettings};

/// Maximum number of data points a single dataset can buffer.
pub const MAX_DATA_POINTS: usize = 600;

/// Default initial number of visible data points in a fresh dataset.
pub const MIN_DATA_POINTS: usize = 10;
