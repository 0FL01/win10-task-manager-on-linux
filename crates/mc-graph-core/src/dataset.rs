/* src/dataset.rs
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

use crate::scaling::{FillingSettings, RoundingSettings, ScalingSettings};
use crate::{MAX_DATA_POINTS, MIN_DATA_POINTS};

#[derive(Clone)]
pub struct DatasetSettings {
    pub dashed: bool,
    pub visible: bool,
    pub fill: FillingSettings,
    pub vertical_dropoff_lines: bool,

    pub scaling_settings: ScalingSettings,
    pub rounding_settings: RoundingSettings,
    pub low_watermark: f32,
    pub high_watermark: f32,

    // for example, when using pow2base10 scaling, we want to ensure we are using the correct reference point to calculate that nearest thousand.
    // we keep this as a float to not exclude other uses
    pub watermarking_multiplier: f32,

    pub following: Option<usize>,
    pub followed: Option<usize>,
}

#[derive(Clone)]
pub struct DatasetGroup {
    pub dataset_settings: DatasetSettings,

    pub datas: Vec<Dataset>,
}

impl Default for DatasetGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl DatasetGroup {
    pub fn new() -> Self {
        Self {
            dataset_settings: DatasetSettings {
                dashed: false,
                fill: Default::default(),
                visible: true,
                vertical_dropoff_lines: true,
                scaling_settings: Default::default(),
                rounding_settings: Default::default(),
                low_watermark: 0.0,
                high_watermark: 100.0,
                watermarking_multiplier: 1.,
                following: None,
                followed: None,
            },
            datas: vec![Dataset::default()],
        }
    }

    pub fn new_with_datas(d: Vec<Vec<(f32, f32)>>) -> Self {
        let mut datas = Vec::with_capacity(d.len());
        for v in d {
            datas.push(Dataset::new_with_data(v));
        }
        Self {
            dataset_settings: DatasetSettings {
                dashed: false,
                fill: Default::default(),
                visible: true,
                vertical_dropoff_lines: true,
                scaling_settings: Default::default(),
                rounding_settings: Default::default(),
                low_watermark: 0.0,
                high_watermark: 100.0,
                watermarking_multiplier: 1.,
                following: None,
                followed: None,
            },
            datas,
        }
    }

    pub fn new_with_fill(v: f32) -> Self {
        Self {
            dataset_settings: DatasetSettings {
                dashed: false,
                fill: Default::default(),
                visible: true,
                vertical_dropoff_lines: true,
                scaling_settings: Default::default(),
                rounding_settings: Default::default(),
                low_watermark: 0.0,
                high_watermark: 100.0,
                watermarking_multiplier: 1.,
                following: None,
                followed: None,
            },
            datas: vec![Dataset::new_with_fill(v)],
        }
    }
}

#[derive(Clone)]
pub struct Dataset {
    data: Vec<f32>,
    x_points: Vec<f32>,
    pub used_data: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct DatasetPoints {
    pub x: f32,
    pub y: f32,
}

impl DatasetGroup {
    pub fn set_datasets(&mut self, sets: usize) {
        for _ in self.datas.len()..sets {
            self.datas.push(Dataset::default());
        }
    }

    pub fn add_data(&mut self, points: &[f32]) {
        self.grow_to(points.len());

        for (idx, set) in points.iter().enumerate() {
            self.update_single_scaling(idx, *set);
        }

        self.update_expensive_scaling();
    }

    fn grow_to(&mut self, sets: usize) {
        while self.datas.len() < sets {
            let next = match self.datas.last() {
                Some(existing) => Dataset::empty_like(existing),
                None => Dataset::default(),
            };
            self.datas.push(next);
        }
    }

    pub fn reset_auto_scaling(&mut self) {
        // todo this better with down scaling support
        self.dataset_settings.high_watermark = 0.0;
        self.update_expensive_scaling()
    }

    fn update_expensive_scaling(&mut self) {
        match self.dataset_settings.scaling_settings {
            ScalingSettings::ScaleUp => {
                self.dataset_settings.high_watermark =
                    self.dataset_settings.rounding_settings.apply_up_rounding(
                        self.get_maximum() * self.dataset_settings.watermarking_multiplier,
                    ) / self.dataset_settings.watermarking_multiplier;
            }
            ScalingSettings::ScaleDown => {
                self.dataset_settings.low_watermark =
                    self.dataset_settings.rounding_settings.apply_down_rounding(
                        self.get_minimum() * self.dataset_settings.watermarking_multiplier,
                    ) / self.dataset_settings.watermarking_multiplier;
            }
            ScalingSettings::ScaleUpDown => {
                self.dataset_settings.high_watermark =
                    self.dataset_settings.rounding_settings.apply_up_rounding(
                        self.get_maximum() * self.dataset_settings.watermarking_multiplier,
                    ) / self.dataset_settings.watermarking_multiplier;
                self.dataset_settings.low_watermark =
                    self.dataset_settings.rounding_settings.apply_down_rounding(
                        self.get_minimum() * self.dataset_settings.watermarking_multiplier,
                    ) / self.dataset_settings.watermarking_multiplier;
            }
            ScalingSettings::StickyUp => {}
            ScalingSettings::StickyDown => {}
            ScalingSettings::StickyUpDown => {}
            ScalingSettings::StickyUpDownEqualMagnitude => {}
            ScalingSettings::Stacking => {}
            ScalingSettings::Fixed => {}
        }
    }

    pub fn apply_following_rules(&mut self, other: Option<&Self>) -> bool {
        let Some(other) = other else {
            return false;
        };

        let mut changed = false;
        if other.dataset_settings.high_watermark > self.dataset_settings.high_watermark {
            self.dataset_settings.high_watermark = other.dataset_settings.high_watermark;
            changed = true;
        }

        if other.dataset_settings.low_watermark < self.dataset_settings.low_watermark {
            self.dataset_settings.low_watermark = other.dataset_settings.low_watermark;
            changed = true;
        }

        changed
    }

    fn get_minimum(&self) -> f32 {
        self.datas
            .iter()
            .filter_map(|set| set.get_data_removed().iter().copied().reduce(f32::min))
            .reduce(f32::min)
            .unwrap_or(self.dataset_settings.low_watermark)
    }

    fn get_maximum(&self) -> f32 {
        self.datas
            .iter()
            .filter_map(|set| set.get_data_removed().iter().copied().reduce(f32::max))
            .reduce(f32::max)
            .unwrap_or(self.dataset_settings.high_watermark)
    }

    // do cheap updates whenever a new point is added
    fn update_single_scaling(&mut self, idx: usize, point: f32) {
        self.datas[idx].data.rotate_right(1);
        self.datas[idx].data[0] = point;

        // do scaling up
        match self.dataset_settings.scaling_settings {
            /* these require searching, wait for the expensive call */
            ScalingSettings::ScaleUp => {}
            ScalingSettings::ScaleDown => {}
            ScalingSettings::ScaleUpDown => {}
            ScalingSettings::StickyUp => {
                let max = self
                    .dataset_settings
                    .rounding_settings
                    .apply_up_rounding(point * self.dataset_settings.watermarking_multiplier)
                    / self.dataset_settings.watermarking_multiplier;

                if max > self.dataset_settings.high_watermark {
                    self.dataset_settings.high_watermark = max
                }
            }
            ScalingSettings::StickyDown => {
                let min = self
                    .dataset_settings
                    .rounding_settings
                    .apply_down_rounding(point * self.dataset_settings.watermarking_multiplier)
                    / self.dataset_settings.watermarking_multiplier;

                if min < self.dataset_settings.low_watermark {
                    self.dataset_settings.low_watermark = min
                }
            }
            ScalingSettings::StickyUpDown => {
                let max = self
                    .dataset_settings
                    .rounding_settings
                    .apply_up_rounding(point * self.dataset_settings.watermarking_multiplier)
                    / self.dataset_settings.watermarking_multiplier;

                if max > self.dataset_settings.high_watermark {
                    self.dataset_settings.high_watermark = max
                }

                let min = self
                    .dataset_settings
                    .rounding_settings
                    .apply_down_rounding(point * self.dataset_settings.watermarking_multiplier)
                    / self.dataset_settings.watermarking_multiplier;

                if min < self.dataset_settings.low_watermark {
                    self.dataset_settings.low_watermark = min
                }
            }
            ScalingSettings::StickyUpDownEqualMagnitude => {
                let max = self
                    .dataset_settings
                    .rounding_settings
                    .apply_up_rounding(point * self.dataset_settings.watermarking_multiplier)
                    / self.dataset_settings.watermarking_multiplier;

                if max > self.dataset_settings.high_watermark {
                    self.dataset_settings.low_watermark -=
                        max - self.dataset_settings.high_watermark;
                    self.dataset_settings.high_watermark = max;
                }

                let min = self
                    .dataset_settings
                    .rounding_settings
                    .apply_down_rounding(point * self.dataset_settings.watermarking_multiplier)
                    / self.dataset_settings.watermarking_multiplier;

                if min < self.dataset_settings.low_watermark {
                    self.dataset_settings.high_watermark +=
                        self.dataset_settings.low_watermark - min;
                    self.dataset_settings.low_watermark = min;
                }
            }
            ScalingSettings::Stacking => {}
            ScalingSettings::Fixed => {}
        }
    }

    pub fn update_data_points(&mut self, new_points: usize) {
        self.datas
            .iter_mut()
            .for_each(|set| set.update_data_points(new_points));
    }

    /// Project every series onto a `width` x `height` canvas.
    pub fn geometry(
        &self,
        width: f32,
        height: f32,
        point_spacing_factor: f32,
    ) -> Option<Vec<Vec<DatasetPoints>>> {
        if !self.dataset_settings.visible {
            return None;
        }

        let mut dataset_points: Vec<Vec<DatasetPoints>> = self
            .datas
            .iter()
            .map(|pts| pts.plot(width, height, &self.dataset_settings, point_spacing_factor))
            .collect();

        if dataset_points.is_empty() {
            return Some(dataset_points);
        }

        dataset_points = match self.dataset_settings.scaling_settings {
            ScalingSettings::Stacking => {
                // First, we pop the first dataset
                let mut stacked_points: Vec<Vec<DatasetPoints>> =
                    dataset_points.drain(0..1).collect();

                // for the remaining datasets, offset their y value off of the previous
                for (series_index, series) in dataset_points.iter().enumerate() {
                    let stacked_series = &stacked_points[series_index];
                    assert_eq!(series.len(), stacked_series.len());

                    let mut newset = Vec::new();

                    for (point_index, point) in series.iter().enumerate() {
                        newset.push(DatasetPoints {
                            x: point.x,
                            y: stacked_series[point_index].y + point.y - height,
                        })
                    }

                    stacked_points.push(newset);
                }

                stacked_points
            }
            _ => dataset_points,
        };

        Some(dataset_points)
    }
}

impl Dataset {
    pub fn new_with_data(d: Vec<(f32, f32)>) -> Self {
        let (x_points, data) = d.into_iter().unzip();
        Dataset {
            data,
            x_points,
            used_data: 0,
        }
    }

    fn empty_like(other: &Self) -> Self {
        let mut data = vec![f32::NAN; other.data.len()];
        if let Some(v) = data.first_mut() {
            // so there's no vertical drop on the series' first frame
            *v = 0.0
        }
        Self {
            data,
            x_points: other.x_points.clone(),
            used_data: other.used_data,
        }
    }

    pub fn new_with_fill(v: f32) -> Self {
        let data = vec![v; MAX_DATA_POINTS];
        Self {
            data,
            x_points: (0..MAX_DATA_POINTS).map(|x| x as f32).collect(),
            used_data: MIN_DATA_POINTS,
        }
    }

    pub fn update_data_points(&mut self, new_points: usize) {
        self.used_data = new_points;
    }

    pub fn get_data(&self) -> Vec<f32> {
        self.data.iter().take(self.used_data).copied().collect()
    }

    pub fn get_data_removed(&self) -> Vec<f32> {
        self.data
            .iter()
            .take(self.used_data)
            .filter(|v| v.is_normal())
            .copied()
            .collect()
    }

    pub fn get_data_sanitized(&self, low_watermark: f32) -> Vec<f32> {
        self.data
            .iter()
            .take(self.used_data)
            .map(|v| {
                if !(v.is_normal() || v.is_nan() || v == &0.0) {
                    low_watermark
                } else {
                    *v
                }
            })
            .collect()
    }

    pub fn plot(
        &self,
        width: f32,
        height: f32,
        settings: &DatasetSettings,
        point_spacing_factor: f32,
    ) -> Vec<DatasetPoints> {
        let val_min = settings.low_watermark;
        let val_max = settings.high_watermark.max(val_min + 1.);

        let spacing = width * point_spacing_factor;

        self.x_points
            .iter()
            .zip(
                self.get_data_sanitized(val_min)
                    .iter()
                    .map(|y| (*y - val_min) / (val_max - val_min)),
            )
            .map(|(x, y)| (width - x * spacing, (1. - y) * height))
            .map(|(x, y)| DatasetPoints { x, y })
            .collect()
    }
}

impl Default for Dataset {
    fn default() -> Self {
        let mut data = vec![f32::NAN; MAX_DATA_POINTS];
        if let Some(v) = data.first_mut() {
            // so there's no vertical drop on first refresh
            *v = 0.0
        }
        Self {
            data,
            x_points: (0..MAX_DATA_POINTS).map(|x| x as f32).collect(),
            used_data: MIN_DATA_POINTS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces mission-center#542: the CPU page builds one series from an incomplete first
    /// reading, then feeds it one point per core once the real reading arrives.
    #[test]
    fn add_data_grows_to_match_the_point_count() {
        let mut group = DatasetGroup::new();
        group.set_datasets(0);
        assert_eq!(group.datas.len(), 1);

        group.add_data(&[7.0; 12]);

        assert_eq!(group.datas.len(), 12);
        for set in &group.datas {
            assert_eq!(set.get_data()[0], 7.0);
        }
    }

    #[test]
    fn add_data_keeps_series_it_was_given_no_point_for() {
        let mut group = DatasetGroup::new();
        group.set_datasets(4);

        group.add_data(&[1.0, 2.0]);

        assert_eq!(group.datas.len(), 4);
        assert_eq!(group.datas[0].get_data()[0], 1.0);
        assert_eq!(group.datas[1].get_data()[0], 2.0);
        // untouched, still holding what `Dataset::default` seeded
        assert_eq!(group.datas[2].get_data()[0], 0.0);
        assert!(group.datas[2].get_data()[1..].iter().all(|v| v.is_nan()));
    }

    #[test]
    fn grown_series_share_the_time_axis_of_the_original() {
        let mut group = DatasetGroup::new();
        group.update_data_points(42);

        group.add_data(&[1.0; 3]);

        let first = &group.datas[0];
        for set in &group.datas[1..] {
            assert_eq!(set.used_data, first.used_data);
            assert_eq!(set.x_points, first.x_points);
            assert_eq!(set.data.len(), first.data.len());
        }
    }

    #[test]
    fn geometry_of_an_empty_stacking_group_does_not_panic() {
        let mut group = DatasetGroup::new();
        group.dataset_settings.scaling_settings = ScalingSettings::Stacking;
        group.datas.clear();

        assert!(group.geometry(100., 100., 1.).is_some_and(|p| p.is_empty()));
    }
}
