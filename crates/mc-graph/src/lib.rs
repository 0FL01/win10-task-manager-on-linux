/* crates/mc-graph/src/lib.rs
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

//! egui port of the `mc-graph-widget` time-series graph.
//!
//! Data handling (ring buffers, scaling, geometry) is reused unchanged from `mc-graph-core`;
//! this crate only draws. Tessellated geometry is cached per (data generation, size, scale
//! factor, style) and reused across frames, so a frame that does not follow new data (hover)
//! only translates cached shapes.

use egui::epaint::{Mesh, PathStroke, Vertex, WHITE_UV};
use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, Vec2};

pub use mc_graph_core::{
    Dataset, DatasetGroup, DatasetPoints, DatasetSettings, FillingSettings, MAX_DATA_POINTS,
    MIN_DATA_POINTS, RoundingSettings, ScalingSettings,
};

/// Win10 style: the grid is the line colour mixed into white at this ratio
const WIN10_GRID_MIX: f32 = 0.16;
/// Win10 style: horizontal grid rows; vertical lines every 1/6 of the width (10 s of 60 s)
const WIN10_GRID_ROWS: u32 = 10;
const WIN10_GRID_COLUMNS: f32 = 6.;

fn mix_white(color: Color32, ratio: f32) -> Color32 {
    let c = |v: u8| (255. - (255. - v as f32) * ratio).round() as u8;
    Color32::from_rgb(c(color.r()), c(color.g()), c(color.b()))
}

/// Unfeathered, pixel-exact rectangle (egui feathers thin rectangles into line segments).
fn pixel_rect(out: &mut Vec<Shape>, rect: Rect, color: Color32) {
    let mut mesh = Mesh::default();
    mesh.add_colored_rect(rect, color);
    out.push(Shape::mesh(mesh));
}
/// History length of every graph: one minute at the normal update speed.
pub const DEFAULT_DATA_POINTS: u32 = 60;

/// Cached geometry: (key, fills under the grid, grid, data)
type GeometryCache = (CacheKey, Vec<Shape>, Vec<Shape>, Vec<Shape>);

#[derive(Clone, PartialEq)]
struct CacheKey {
    generation: u64,
    size: Vec2,
    pixels_per_point: f32,
}

/// A time-series graph with the same API surface as the GTK `GraphWidget`.
pub struct Graph {
    groups: Vec<DatasetGroup>,

    data_points: u32,
    /// Line, border and (mixed into white) grid colour
    base_color: Color32,
    /// Opaque area fill under the grid
    fill: Color32,
    grid_visible: bool,

    scroll_offset: u32,

    /// Bumped on every data or style change; invalidates the geometry cache.
    generation: u64,
    cache: Option<GeometryCache>,
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Graph {
    pub fn new() -> Self {
        Self {
            groups: vec![],
            data_points: DEFAULT_DATA_POINTS,
            base_color: Color32::BLACK,
            fill: Color32::WHITE,
            grid_visible: true,
            scroll_offset: 0,
            generation: 0,
            cache: None,
        }
    }

    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn groups(&self) -> &[DatasetGroup] {
        &self.groups
    }

    pub fn groups_mut(&mut self) -> &mut Vec<DatasetGroup> {
        self.invalidate();
        &mut self.groups
    }

    pub fn set_data_points(&mut self, count: u32) {
        if self.data_points != count {
            for group in self.groups.iter_mut() {
                group.update_data_points(count as usize);
            }
            self.data_points = count;
            self.invalidate();
        }
    }

    pub fn set_base_color(&mut self, color: Color32) {
        if self.base_color != color {
            self.base_color = color;
            self.invalidate();
        }
    }

    pub fn set_fill(&mut self, fill: Color32) {
        if self.fill != fill {
            self.fill = fill;
            self.invalidate();
        }
    }

    pub fn set_grid_visible(&mut self, visible: bool) {
        if self.grid_visible != visible {
            self.grid_visible = visible;
            self.invalidate();
        }
    }

    pub fn set_filled(&mut self, index: usize, filled: FillingSettings) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.fill = filled;
            self.invalidate();
        }
    }

    pub fn set_data_visible(&mut self, index: usize, visible: bool) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.visible = visible;
            self.invalidate();
        }
    }

    pub fn clear_datasets(&mut self) {
        self.groups.clear();
        self.invalidate();
    }

    pub fn add_dataset(&mut self, mut dataset: DatasetGroup) {
        dataset.update_data_points(self.data_points as usize);
        self.groups.push(dataset);
        self.invalidate();
    }

    /// Appends one sample to every dataset group (`datas[i]` holds one value per series).
    pub fn add_data_point(&mut self, datas: Vec<Vec<f32>>) {
        for (group, values) in self.groups.iter_mut().zip(datas.iter()) {
            group.add_data(values);
        }
        Self::apply_followings(&mut self.groups);
        self.invalidate();
    }

    pub fn add_single_data_point(&mut self, idx: usize, datas: Vec<f32>) {
        if let Some(group) = self.groups.get_mut(idx) {
            group.add_data(&datas);
            Self::apply_followings(&mut self.groups);
            self.invalidate();
        }
    }

    fn apply_followings(data: &mut [DatasetGroup]) {
        // bounded to avoid pathological cycles
        for _ in 0..data.len().max(1) * 2 {
            let mut changed = false;

            for i in 0..data.len() {
                let Some(other) = data[i]
                    .dataset_settings
                    .following
                    .and_then(|it| data.get(it).cloned())
                else {
                    continue;
                };

                changed |= data[i].apply_following_rules(Some(&other));
            }

            if !changed {
                break;
            }
        }
    }

    pub fn set_dataset_scaling(&mut self, index: usize, scaler: ScalingSettings) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.scaling_settings = scaler;
            self.invalidate();
        }
    }

    pub fn set_all_datasets_scaling(&mut self, scaler: ScalingSettings) {
        for g in self.groups.iter_mut() {
            g.dataset_settings.scaling_settings = scaler.clone();
        }
        self.invalidate();
    }

    pub fn set_all_datasets_rounding(&mut self, rounder: RoundingSettings) {
        for g in self.groups.iter_mut() {
            g.dataset_settings.rounding_settings = rounder.clone();
        }
        self.invalidate();
    }

    pub fn set_all_datasets_watermarking_multiplier(&mut self, multiplier: f32) {
        for g in self.groups.iter_mut() {
            g.dataset_settings.watermarking_multiplier = multiplier;
        }
        self.invalidate();
    }

    pub fn set_all_datasets_max_scale(&mut self, max: f32) {
        for g in self.groups.iter_mut() {
            g.dataset_settings.high_watermark = max;
        }
        self.invalidate();
    }

    pub fn set_dataset_max_scale(&mut self, index: usize, max: f32) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.high_watermark = max;
            self.invalidate();
        }
    }

    pub fn dataset_max_scale(&self, index: usize) -> f32 {
        self.groups
            .get(index)
            .map(|g| g.dataset_settings.high_watermark)
            .unwrap_or_default()
    }

    pub fn dataset_min_scale(&self, index: usize) -> f32 {
        self.groups
            .get(index)
            .map(|g| g.dataset_settings.low_watermark)
            .unwrap_or_default()
    }

    /// When used with sticky scaling, connected datasets share the largest high watermark and
    /// the smallest low watermark.
    pub fn connect_datasets(&mut self, a: usize, b: usize) {
        if a < self.groups.len() && b < self.groups.len() {
            self.groups[a].dataset_settings.following = Some(b);
            self.groups[b].dataset_settings.followed = Some(a);
        }
    }

    pub fn reset_auto_scaling(&mut self) {
        for g in self.groups.iter_mut() {
            g.reset_auto_scaling();
        }
        Self::apply_followings(&mut self.groups);
        self.invalidate();
    }

    pub fn point_spacing_factor(&self) -> f32 {
        1. / self.data_points.saturating_sub(1).max(1) as f32
    }

    /// Moves the vertical grid lines with the window-global sample counter, so the grid scrolls
    /// together with the data on every refresh. Returns `true` (the graph needs repainting).
    pub fn set_grid_offset(&mut self, offset: u32) -> bool {
        if self.scroll_offset != offset {
            self.scroll_offset = offset;
            self.invalidate();
        }
        true
    }

    pub fn paint(&mut self, ui: &Ui, rect: Rect) {
        // Square 1 px border, white background, opaque fill under a light grid, 1 px line; data
        // and grid live inside the border (`rect` includes it)
        let area = rect.shrink(1.);
        let size = area.size();
        if size.x < 1. || size.y < 1. {
            return;
        }

        let key = CacheKey {
            generation: self.generation,
            size,
            pixels_per_point: ui.ctx().pixels_per_point(),
        };

        let rebuild = !matches!(&self.cache, Some((k, ..)) if *k == key);
        if rebuild {
            let (under, grid, data) = self.build_shapes(size);
            self.cache = Some((key, under, grid, data));
        }

        let Some((_, under, grid, data)) = &self.cache else {
            return;
        };

        let origin = area.min.to_vec2();
        let add = |painter: &egui::Painter, shapes: &[Shape], at: Vec2| {
            for shape in shapes {
                let mut s = shape.clone();
                s.translate(at);
                painter.add(s);
            }
        };

        let outer = ui.painter_at(rect);
        let mut frame = vec![];
        pixel_rect(&mut frame, rect, self.base_color);
        pixel_rect(&mut frame, area, Color32::WHITE);
        outer.extend(frame);
        let painter = ui.painter_at(area);
        add(&painter, under, origin);
        add(&painter, grid, origin);
        add(&painter, data, origin);
    }

    fn build_shapes(&self, size: Vec2) -> (Vec<Shape>, Vec<Shape>, Vec<Shape>) {
        let (width, height) = (size.x, size.y);
        let mut grid = vec![];
        if self.grid_visible {
            self.build_grid(&mut grid, width, height);
        }

        let mut under = vec![];
        let mut data = vec![];
        for group in &self.groups {
            self.build_group(&mut under, &mut data, group, width, height);
        }

        (under, grid, data)
    }

    /// Grid inside the border: rows at `ceil(k · (h + 1) / 10)` from the top border
    /// (measured on the reference), columns every width/6 scrolling with the data.
    fn build_grid(&self, out: &mut Vec<Shape>, width: f32, height: f32) {
        let color = mix_white(self.base_color, WIN10_GRID_MIX);
        let span = height + 1.;
        for k in 1..WIN10_GRID_ROWS {
            let y = (k as f32 * span / WIN10_GRID_ROWS as f32).ceil() - 1.;
            if y >= 0. && y < height {
                pixel_rect(
                    out,
                    Rect::from_min_size(Pos2::new(0., y), Vec2::new(width, 1.)),
                    color,
                );
            }
        }
        let col_width = width / WIN10_GRID_COLUMNS;
        let point_spacing = width * self.point_spacing_factor();
        let offset = if col_width > 0. {
            (point_spacing as f64 * -(self.scroll_offset as f64)).rem_euclid(col_width as f64)
                as f32
        } else {
            0.
        };
        let mut x = offset;
        while x < width {
            let px = x.floor();
            if px >= 0. {
                pixel_rect(
                    out,
                    Rect::from_min_size(Pos2::new(px, 0.), Vec2::new(1., height)),
                    color,
                );
            }
            x += col_width;
        }
    }

    fn build_group(
        &self,
        under: &mut Vec<Shape>,
        out: &mut Vec<Shape>,
        group: &DatasetGroup,
        width: f32,
        height: f32,
    ) {
        let Some(series) = group.geometry(width, height, self.point_spacing_factor()) else {
            return;
        };

        let settings = &group.dataset_settings;
        let stroke_color = self.base_color;
        let stroke_width = 1.;

        let baseline = match settings.fill {
            FillingSettings::FillToBottom | FillingSettings::None => height,
            FillingSettings::FillToTop => 0.,
            FillingSettings::FillToZero => {
                if settings.low_watermark >= 0. || settings.low_watermark >= settings.high_watermark
                {
                    height
                } else {
                    height * settings.high_watermark
                        / (settings.high_watermark - settings.low_watermark)
                }
            }
        };

        let series_count = series.len();
        for (set_index, set) in series.iter().enumerate() {
            for run in set.split(|p| p.y.is_nan() || p.x.is_nan()) {
                let points = self.sample_run(run, width);
                if points.len() < 2 {
                    continue;
                }

                let fill = settings.fill != FillingSettings::None
                    && (settings.scaling_settings != ScalingSettings::Stacking
                        || set_index == series_count - 1);
                if fill {
                    under.push(Shape::mesh(area_mesh(&points, baseline, self.fill)));
                }

                if settings.dashed {
                    out.extend(Shape::dashed_line(
                        &points,
                        Stroke::new(stroke_width, stroke_color),
                        5.,
                        5.,
                    ));
                } else {
                    out.push(Shape::line(
                        points.clone(),
                        PathStroke::new(stroke_width, stroke_color),
                    ));
                }

                if settings.vertical_dropoff_lines {
                    let first = points[0];
                    let last = points[points.len() - 1];
                    let stroke = Stroke::new(stroke_width, stroke_color);
                    out.push(Shape::line_segment(
                        [first, Pos2::new(first.x, baseline)],
                        stroke,
                    ));
                    out.push(Shape::line_segment(
                        [last, Pos2::new(last.x, baseline)],
                        stroke,
                    ));
                }
            }
        }
    }

    /// Converts geometry into screen points: clips to the visible range and decimates runs denser
    /// than the pixel grid.
    fn sample_run(&self, run: &[DatasetPoints], width: f32) -> Vec<Pos2> {
        // Points are ordered newest (right) to oldest (left); keep one point past each edge so
        // lines reach the border.
        let mut visible = Vec::with_capacity(run.len());
        for (i, p) in run.iter().enumerate() {
            let next_inside = run.get(i + 1).is_some_and(|n| n.x >= 0.);
            let prev_inside = i > 0 && run[i - 1].x <= width;
            if (p.x >= 0. && p.x <= width) || next_inside || prev_inside {
                visible.push(Pos2::new(p.x, p.y));
            }
        }

        // Decimate: when several samples share one pixel column keep its min and max
        let spacing = if visible.len() > 1 {
            (visible[0].x - visible[visible.len() - 1].x).abs() / (visible.len() - 1) as f32
        } else {
            1.
        };
        if spacing < 0.5 && visible.len() > 4 {
            decimate(&visible)
        } else {
            visible
        }
    }
}

/// Keeps min/max per pixel column, preserving order.
fn decimate(points: &[Pos2]) -> Vec<Pos2> {
    let mut out = Vec::with_capacity(points.len() / 2 + 2);
    let mut i = 0;
    while i < points.len() {
        let column = points[i].x.floor();
        let mut j = i;
        let (mut min, mut max) = (points[i], points[i]);
        while j < points.len() && points[j].x.floor() == column {
            if points[j].y < min.y {
                min = points[j];
            }
            if points[j].y > max.y {
                max = points[j];
            }
            j += 1;
        }
        if j - i == 1 {
            out.push(points[i]);
        } else {
            out.push(Pos2::new(column + 0.5, min.y));
            out.push(Pos2::new(column + 0.5, max.y));
        }
        i = j;
    }
    out
}

/// Triangle strip between a polyline and a horizontal baseline (handles concave shapes, which
/// a convex polygon fill would not).
fn area_mesh(points: &[Pos2], baseline: f32, color: Color32) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(points.len() * 2);
    mesh.reserve_triangles((points.len() - 1) * 2);

    for p in points {
        mesh.vertices.push(Vertex {
            pos: *p,
            uv: WHITE_UV,
            color,
        });
        mesh.vertices.push(Vertex {
            pos: Pos2::new(p.x, baseline),
            uv: WHITE_UV,
            color,
        });
    }

    for i in 0..(points.len() as u32 - 1) {
        let top0 = i * 2;
        let bottom0 = top0 + 1;
        let top1 = top0 + 2;
        let bottom1 = top0 + 3;
        mesh.add_triangle(top0, bottom0, top1);
        mesh.add_triangle(top1, bottom0, bottom1);
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_with(points: &[f32]) -> Graph {
        let mut g = Graph::new();
        g.set_data_points(10);
        g.add_dataset(DatasetGroup::new());
        for p in points {
            g.add_data_point(vec![vec![*p]]);
        }
        g
    }

    #[test]
    fn data_changes_invalidate_cache() {
        let mut g = graph_with(&[1., 2.]);
        let before = g.generation();
        g.add_data_point(vec![vec![3.]]);
        assert_ne!(before, g.generation());
    }

    #[test]
    fn shapes_cover_fill_and_line() {
        let g = graph_with(&[10., 50., 90.]);
        let (under, grid, data) = g.build_shapes(Vec2::new(200., 100.));
        assert!(!grid.is_empty());
        assert!(under.iter().any(|s| matches!(s, Shape::Mesh(_))));
        assert!(data.iter().any(|s| matches!(s, Shape::Path(_))));
    }

    #[test]
    fn invisible_groups_draw_nothing() {
        let mut g = graph_with(&[10., 50.]);
        g.set_data_visible(0, false);
        g.set_grid_visible(false);
        let (under, grid, data) = g.build_shapes(Vec2::new(200., 100.));
        assert!(under.is_empty() && grid.is_empty() && data.is_empty());
    }

    #[test]
    fn area_mesh_has_two_triangles_per_segment() {
        let mesh = area_mesh(
            &[Pos2::new(0., 1.), Pos2::new(1., 5.), Pos2::new(2., 0.)],
            10.,
            Color32::RED,
        );
        assert_eq!(mesh.indices.len(), 4 * 3);
    }

    #[test]
    fn decimation_keeps_extremes() {
        let pts = [
            Pos2::new(0.1, 5.),
            Pos2::new(0.2, 1.),
            Pos2::new(0.3, 9.),
            Pos2::new(1.2, 3.),
        ];
        let d = decimate(&pts);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].y, 1.);
        assert_eq!(d[1].y, 9.);
    }

    #[test]
    fn win10_grid_rows_match_reference() {
        // Reference: graph border rows 151 and 305, grid rows at 167, 182, …, 290
        let g = graph_with(&[0.5; 10]);
        let mut grid = vec![];
        g.build_grid(&mut grid, 378., 153.);
        let rows: Vec<f32> = grid
            .iter()
            .filter_map(|s| match s {
                Shape::Mesh(m) => {
                    let r = m.calc_bounds();
                    (r.height() == 1.).then_some(r.top() + 152.)
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows, [167., 182., 198., 213., 228., 244., 259., 275., 290.]);
    }

    #[test]
    fn win10_fill_goes_under_the_grid() {
        let mut g = graph_with(&[0.2, 0.4, 0.6]);
        g.set_fill(Color32::LIGHT_BLUE);
        let (under, _, data) = g.build_shapes(Vec2::new(200., 100.));
        assert!(!under.is_empty());
        assert!(data.iter().all(|s| !matches!(s, Shape::Mesh(_))));
    }
}
