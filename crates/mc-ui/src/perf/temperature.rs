/* crates/mc-ui/src/perf/temperature.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! One hwmon temperature channel of the Cooling page: its history, thresholds and details.

use mc_core::i18n::{i18n, i18n_f, n_};
use mc_graph::{DatasetGroup, Graph, ScalingSettings};
use mc_magpie::TemperatureSensor;

use super::cpu::{TEMPERATURE_HIGH_WATERMARK, TEMPERATURE_LOW_WATERMARK};
use super::{GraphPanel, PageView, over};

const COLOR: egui::Color32 = crate::win10::palette::FAN.line;
/// Driver limits outside (0, 200] °C are placeholders (NVMe reports 65262 °C), not limits.
const PLAUSIBLE_LIMIT: std::ops::RangeInclusive<f32> = 0.001..=200.;

pub fn key(sensor: &TemperatureSensor) -> String {
    let chip = sensor.chip.as_ref();
    format!(
        "temp:{}-{}-{}",
        chip.map_or(0, |c| c.hwmon_index),
        chip.and_then(|c| c.name.as_deref()).unwrap_or_default(),
        sensor.temp_index
    )
}

pub fn celsius(millicelsius: Option<i32>) -> Option<f32> {
    millicelsius.map(|m| m as f32 / 1000.)
}

/// A driver limit worth showing.
pub fn limit(millicelsius: Option<i32>) -> Option<f32> {
    celsius(millicelsius).filter(|c| PLAUSIBLE_LIMIT.contains(c))
}

/// The driver's label, otherwise the channel ("temp1"), unchanged.
pub fn label(sensor: &TemperatureSensor) -> String {
    sensor
        .label
        .clone()
        .unwrap_or_else(|| format!("temp{}", sensor.temp_index))
}

pub fn temperature_label(celsius: Option<f32>) -> String {
    celsius
        .map(|c| i18n_f("{} °C", &[&format!("{c:.1}")]))
        .unwrap_or_else(|| "—".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Normal,
    /// At or above the driver's `max`
    High,
    /// At or above the driver's `crit`
    Critical,
}

pub fn level(current: Option<f32>, max: Option<f32>, crit: Option<f32>) -> Level {
    match current {
        Some(t) if crit.is_some_and(|crit| t >= crit) => Level::Critical,
        Some(t) if max.is_some_and(|max| t >= max) => Level::High,
        _ => Level::Normal,
    }
}

/// Fraction of the critical (otherwise high) limit; nothing without a plausible limit.
pub fn bar(current: Option<f32>, max: Option<f32>, crit: Option<f32>) -> Option<f32> {
    let limit = crit.or(max)?;
    current
        .filter(|t| *t >= 0.)
        .map(|t| (t / limit).clamp(0., 1.))
}

pub struct SensorEntry {
    key: String,
    title: String,
    chip: String,
    channel: String,
    graph: Graph,
    current: String,
    highest: Option<f32>,
    max: Option<f32>,
    crit: Option<f32>,
    max_y: String,
}

impl SensorEntry {
    pub fn new(sensor: &TemperatureSensor) -> Self {
        let mut graph = Graph::new();
        graph.set_base_color(COLOR);
        let mut t = DatasetGroup::new_with_fill(f32::NAN);
        t.dataset_settings.scaling_settings = ScalingSettings::StickyUpDown;
        t.dataset_settings.high_watermark = TEMPERATURE_HIGH_WATERMARK;
        t.dataset_settings.low_watermark = TEMPERATURE_LOW_WATERMARK;
        graph.add_dataset(t);
        Self {
            key: key(sensor),
            title: String::new(),
            chip: String::new(),
            channel: format!("temp{}", sensor.temp_index),
            graph,
            current: String::new(),
            highest: None,
            max: None,
            crit: None,
            max_y: String::new(),
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn update(&mut self, sensor: &TemperatureSensor, chip_title: &str) {
        self.title = label(sensor);
        self.chip = chip_title.to_owned();
        let current = celsius(sensor.input_millicelsius);
        self.current = temperature_label(current);
        if let Some(t) = current {
            self.highest = Some(self.highest.map_or(t, |h| h.max(t)));
        }
        self.max = limit(sensor.max_millicelsius);
        self.crit = limit(sensor.crit_millicelsius);
        self.graph
            .add_data_point(vec![vec![current.unwrap_or(f32::NAN)]]);
        self.max_y = format!("{} °C", self.graph.dataset_max_scale(0).round());
    }

    pub fn set_grid_offset(&mut self, offset: u32) -> bool {
        self.graph.set_grid_offset(offset)
    }

    pub fn detail_view(&mut self, max_duration: &str) -> PageView<'_> {
        let mut stats = vec![(i18n("Current Temperature"), self.current.clone())];
        stats.push((i18n("Highest"), temperature_label(self.highest)));
        let mut info = vec![(i18n("Sensor:"), format!("{} · {}", self.chip, self.channel))];
        if let Some(max) = self.max {
            info.push((i18n("High limit:"), temperature_label(Some(max))));
        }
        if let Some(crit) = self.crit {
            info.push((i18n("Critical limit:"), temperature_label(Some(crit))));
        }
        PageView {
            model_tooltip: None,
            title: self.title.clone(),
            model: self.title.clone(),
            panels: vec![GraphPanel::single(
                over(n_("Temperature over "), max_duration),
                self.max_y.clone(),
                &mut self.graph,
            )],
            stats: vec![stats],
            stat_legends: vec![],
            info,
            composition: None,
            swap_compression: None,
        }
    }

    pub fn data_summary(&self) -> String {
        let unsupported = i18n("Unsupported");
        let limit = |value: Option<f32>| {
            value
                .map(|v| temperature_label(Some(v)))
                .unwrap_or_else(|| unsupported.clone())
        };
        format!(
            r#"Temperature

    {}
    {} · {}

    Current Temperature: {}
    High limit:          {}
    Critical limit:      {}"#,
            self.title,
            self.chip,
            self.channel,
            self.current,
            limit(self.max),
            limit(self.crit),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_levels_and_bars() {
        assert_eq!(limit(Some(65_262_000)), None, "placeholder limit");
        assert_eq!(limit(Some(0)), None);
        assert_eq!(limit(Some(110_000)), Some(110.));
        assert_eq!(level(Some(0.), None, None), Level::Normal);
        assert_eq!(level(Some(90.), Some(80.), Some(100.)), Level::High);
        assert_eq!(level(Some(100.), Some(80.), Some(100.)), Level::Critical);
        assert_eq!(level(None, Some(80.), Some(100.)), Level::Normal);
        assert_eq!(bar(Some(55.), Some(80.), Some(110.)), Some(0.5));
        assert_eq!(bar(Some(40.), Some(80.), None), Some(0.5));
        assert_eq!(bar(Some(40.), None, None), None);
        assert_eq!(bar(Some(-5.), None, Some(100.)), None);
        assert_eq!(temperature_label(None), "—");
    }
}
