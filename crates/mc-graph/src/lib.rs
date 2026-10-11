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
use egui::{
    Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

pub use mc_graph_core::{
    Dataset, DatasetGroup, DatasetPoints, DatasetSettings, FillingSettings, MAX_DATA_POINTS,
    MIN_DATA_POINTS, RoundingSettings, ScalingSettings,
};

const GRAPH_RADIUS: f32 = 7.;
/// Win10 style: the grid is the line colour mixed into white at this ratio
const WIN10_GRID_MIX: f32 = 0.16;
/// Win10 style: horizontal grid rows; vertical lines every 1/6 of the width (10 s of 60 s)
const WIN10_GRID_ROWS: u32 = 10;
const WIN10_GRID_COLUMNS: f32 = 6.;

/// Visual style of a graph.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum GraphStyle {
    /// Rounded outline, translucent fill and grid (former GNOME look)
    #[default]
    Rounded,
    /// Win10 Task Manager look: square 1 px border, white background, opaque `fill` under a light
    /// grid, 1 px line. `rect` passed to [`Graph::paint`] includes the border.
    Win10 { fill: Color32 },
}

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
// 20% opacity for the gridline overlay
const GRID_LINE_ALPHA: f32 = 51. / 255.;
const GRID_VERTICAL_LINE_IDEAL_PIXEL_WIDTH: f32 = 60.;
const GRID_HORIZONTAL_LINE_IDEAL_PIXEL_WIDTH: f32 = 50.;
/// History length of every graph: one minute at the normal update speed.
pub const DEFAULT_DATA_POINTS: u32 = 60;

fn compute_line_count(pixel_size: f32, ideal_pix: f32, force_divisible: f32) -> u32 {
    let raw_num_lines = pixel_size / ideal_pix;
    let num_lines = if force_divisible > 1. {
        (raw_num_lines / force_divisible).round() * force_divisible - 1.0
    } else {
        raw_num_lines.round()
    };
    num_lines.max(force_divisible - 1.0) as u32
}

fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0., 1.) * 255.).round() as u8,
    )
}

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
    base_color: Color32,
    scroll: bool,
    grid_visible: bool,
    force_vertical_line_count_divisible_by: f32,
    force_horizontal_line_count_divisible_by: f32,

    scroll_offset: u32,
    style: GraphStyle,

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
            scroll: true,
            grid_visible: true,
            force_vertical_line_count_divisible_by: 1.,
            force_horizontal_line_count_divisible_by: 2.,
            scroll_offset: 0,
            style: GraphStyle::Rounded,
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

    pub fn data_points(&self) -> u32 {
        self.data_points
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

    pub fn base_color(&self) -> Color32 {
        self.base_color
    }

    pub fn set_base_color(&mut self, color: Color32) {
        if self.base_color != color {
            self.base_color = color;
            self.invalidate();
        }
    }

    pub fn style(&self) -> GraphStyle {
        self.style
    }

    pub fn set_style(&mut self, style: GraphStyle) {
        if self.style != style {
            self.style = style;
            self.invalidate();
        }
    }

    pub fn set_scroll(&mut self, scroll: bool) {
        if self.scroll != scroll {
            self.scroll = scroll;
            self.invalidate();
        }
    }

    pub fn set_grid_visible(&mut self, visible: bool) {
        if self.grid_visible != visible {
            self.grid_visible = visible;
            self.invalidate();
        }
    }

    pub fn set_force_horizontal_line_count_divisible_by(&mut self, v: f32) {
        self.force_horizontal_line_count_divisible_by = v;
        self.invalidate();
    }

    pub fn set_force_vertical_line_count_divisible_by(&mut self, v: f32) {
        self.force_vertical_line_count_divisible_by = v;
        self.invalidate();
    }

    pub fn set_dashed(&mut self, index: usize, dashed: bool) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.dashed = dashed;
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

    pub fn set_dataset_rounding(&mut self, index: usize, rounder: RoundingSettings) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.rounding_settings = rounder;
            self.invalidate();
        }
    }

    pub fn set_dataset_opacity(&mut self, index: usize, opacity: f32) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.opacity = opacity;
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

    pub fn set_dataset_min_scale(&mut self, index: usize, min: f32) {
        if let Some(g) = self.groups.get_mut(index) {
            g.dataset_settings.low_watermark = min;
            self.invalidate();
        }
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

    /// Draws the graph filling `size` (or the available space when `None`).
    pub fn ui(&mut self, ui: &mut Ui, size: Vec2) -> Response {
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        if ui.is_rect_visible(rect) {
            self.paint(ui, rect);
        }
        response
    }

    pub fn paint(&mut self, ui: &Ui, rect: Rect) {
        let win10 = matches!(self.style, GraphStyle::Win10 { .. });
        // Win10: data and grid live inside the 1 px border
        let area = if win10 { rect.shrink(1.) } else { rect };
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
            let (under, grid, data) = self.build_shapes(size, key.pixels_per_point);
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

        if win10 {
            let outer = ui.painter_at(rect);
            let mut frame = vec![];
            pixel_rect(&mut frame, rect, self.base_color);
            pixel_rect(&mut frame, area, Color32::WHITE);
            outer.extend(frame);
            let painter = ui.painter_at(area);
            add(&painter, under, origin);
            add(&painter, grid, origin);
            add(&painter, data, origin);
            return;
        }

        let painter = ui.painter_at(rect);
        add(&painter, grid, origin);
        add(&painter, data, origin);

        // Rounded "clip": mask the corners with the panel colour, then draw the outline
        let bg = ui.visuals().panel_fill;
        paint_corner_masks(&painter, rect, GRAPH_RADIUS, bg);
        painter.rect_stroke(
            rect.shrink(0.5),
            CornerRadius::same(GRAPH_RADIUS as u8),
            Stroke::new(1., self.base_color),
            StrokeKind::Middle,
        );
    }

    fn build_shapes(
        &self,
        size: Vec2,
        pixels_per_point: f32,
    ) -> (Vec<Shape>, Vec<Shape>, Vec<Shape>) {
        let (width, height) = (size.x, size.y);
        let mut grid = vec![];
        if self.grid_visible {
            match self.style {
                GraphStyle::Rounded => self.build_grid(&mut grid, width, height, pixels_per_point),
                GraphStyle::Win10 { .. } => self.build_win10_grid(&mut grid, width, height),
            }
        }

        let mut under = vec![];
        let mut data = vec![];
        for group in &self.groups {
            self.build_group(
                &mut under,
                &mut data,
                group,
                width,
                height,
                pixels_per_point,
            );
        }

        (under, grid, data)
    }

    /// Win10 grid inside the border: rows at `ceil(k · (h + 1) / 10)` from the top border
    /// (measured on the reference), columns every width/6 scrolling with the data.
    fn build_win10_grid(&self, out: &mut Vec<Shape>, width: f32, height: f32) {
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
        let offset = if self.scroll && col_width > 0. {
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

    fn build_grid(&self, out: &mut Vec<Shape>, width: f32, height: f32, pixels_per_point: f32) {
        let color = with_alpha(self.base_color, GRID_LINE_ALPHA);
        let stroke = Stroke::new(1. / pixels_per_point.max(1.), color);
        let half = 0.5 / pixels_per_point.max(1.);

        let horizontal_line_count = compute_line_count(
            height,
            GRID_HORIZONTAL_LINE_IDEAL_PIXEL_WIDTH,
            self.force_horizontal_line_count_divisible_by,
        ) + 1;
        let vertical_line_count = compute_line_count(
            width,
            GRID_VERTICAL_LINE_IDEAL_PIXEL_WIDTH,
            self.force_vertical_line_count_divisible_by,
        ) + 1;

        let point_spacing = width * self.point_spacing_factor();

        let frame_height = height / horizontal_line_count as f32;
        for i in 1..horizontal_line_count {
            let y = (frame_height * i as f32).round() + half;
            out.push(Shape::line_segment(
                [Pos2::new(0., y), Pos2::new(width, y)],
                stroke,
            ));
        }

        let col_width = (width + point_spacing) / vertical_line_count as f32;
        let scroll_offset = if self.scroll && col_width > 0. {
            let raw = point_spacing as f64 * -(self.scroll_offset as f64);
            raw.rem_euclid(col_width as f64) as f32
        } else {
            0.
        };

        for i in 0..=vertical_line_count {
            let x = (col_width * i as f32 + scroll_offset - point_spacing).round() + half;
            if x < 0. || x > width {
                continue;
            }
            out.push(Shape::line_segment(
                [Pos2::new(x, 0.), Pos2::new(x, height)],
                stroke,
            ));
        }
    }

    fn build_group(
        &self,
        under: &mut Vec<Shape>,
        out: &mut Vec<Shape>,
        group: &DatasetGroup,
        width: f32,
        height: f32,
        pixels_per_point: f32,
    ) {
        let Some(series) = group.geometry(width, height, self.point_spacing_factor()) else {
            return;
        };

        let settings = &group.dataset_settings;
        let stroke_color = self.base_color;
        let (fill_color, fill_out_of_line) = match self.style {
            GraphStyle::Rounded => (with_alpha(self.base_color, settings.opacity), false),
            // Opaque fill, scaled down for secondary datasets (e.g. kernel time)
            GraphStyle::Win10 { fill } => (fill, true),
        };
        let _ = pixels_per_point;
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
                    let mesh = Shape::mesh(area_mesh(&points, baseline, fill_color));
                    if fill_out_of_line {
                        under.push(mesh);
                    } else {
                        out.push(mesh);
                    }
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

/// Paints the area between the rectangle corners and the rounded outline, emulating a rounded
/// clip (egui only clips to axis aligned rectangles).
fn paint_corner_masks(painter: &egui::Painter, rect: Rect, radius: f32, color: Color32) {
    const STEPS: usize = 6;
    let corners = [
        (rect.left_top(), Vec2::new(1., 1.)),
        (rect.right_top(), Vec2::new(-1., 1.)),
        (rect.left_bottom(), Vec2::new(1., -1.)),
        (rect.right_bottom(), Vec2::new(-1., -1.)),
    ];

    for (corner, dir) in corners {
        let center = corner + Vec2::new(dir.x * radius, dir.y * radius);
        let mut mesh = Mesh::default();
        mesh.vertices.push(Vertex {
            pos: corner,
            uv: WHITE_UV,
            color,
        });
        for s in 0..=STEPS {
            let angle = std::f32::consts::FRAC_PI_2 * s as f32 / STEPS as f32;
            let p =
                center + Vec2::new(-dir.x * radius * angle.cos(), -dir.y * radius * angle.sin());
            mesh.vertices.push(Vertex {
                pos: p,
                uv: WHITE_UV,
                color,
            });
        }
        for s in 1..=STEPS as u32 {
            mesh.add_triangle(0, s, s + 1);
        }
        painter.add(Shape::mesh(mesh));
    }
}

/// Settings shared by every graph on the performance page.
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
    fn line_counts_match_gtk() {
        assert_eq!(compute_line_count(300., 60., 1.), 5);
        assert_eq!(compute_line_count(200., 50., 2.), 3);
        assert_eq!(compute_line_count(10., 50., 2.), 1);
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
        let (_, grid, data) = g.build_shapes(Vec2::new(200., 100.), 1.);
        assert!(!grid.is_empty());
        assert!(data.iter().any(|s| matches!(s, Shape::Mesh(_))));
        assert!(data.iter().any(|s| matches!(s, Shape::Path(_))));
    }

    #[test]
    fn invisible_groups_draw_nothing() {
        let mut g = graph_with(&[10., 50.]);
        g.set_data_visible(0, false);
        g.set_grid_visible(false);
        let (_, grid, data) = g.build_shapes(Vec2::new(200., 100.), 1.);
        assert!(grid.is_empty() && data.is_empty());
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
        let mut g = graph_with(&[0.5; 10]);
        g.set_style(GraphStyle::Win10 {
            fill: Color32::WHITE,
        });
        g.set_scroll(false);
        let mut grid = vec![];
        g.build_win10_grid(&mut grid, 378., 153.);
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
        g.set_style(GraphStyle::Win10 {
            fill: Color32::LIGHT_BLUE,
        });
        let (under, _, data) = g.build_shapes(Vec2::new(200., 100.), 1.);
        assert!(!under.is_empty());
        assert!(data.iter().all(|s| !matches!(s, Shape::Mesh(_))));
    }
}
