/* crates/mc-ui/src/perf/fan.rs
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

//! One fan of the Cooling page: its own speed/PWM history and details. Fans carry no
//! temperature: hwmon fan and temperature channels sharing a number are unrelated.

use super::{GraphPanel, PageView, StatLegend, over};
use mc_core::i18n::{i18n, i18n_f, n_};
use mc_graph::{DatasetGroup, FillingSettings, Graph, ScalingSettings};
use mc_magpie::Fan;

const COLOR: egui::Color32 = crate::win10::palette::FAN.line;

/// Stable identity within a session: hwmon device, driver and channel.
pub fn key(fan: &Fan) -> String {
    let chip = fan.chip.as_ref().and_then(|c| c.name.as_deref());
    format!(
        "fan:{}-{}-{}",
        fan.hwmon_index,
        chip.unwrap_or_default(),
        fan.fan_index
    )
}

/// `None` when the reading is unknown; a legacy daemon cannot report validity.
pub fn rpm(fan: &Fan) -> Option<u32> {
    (fan.rpm_valid != Some(false)).then_some(fan.rpm)
}

/// The driver's label, otherwise the channel ("Fan 1").
pub fn label(fan: &Fan) -> String {
    fan.fan_label
        .clone()
        .unwrap_or_else(|| i18n_f("Fan {}", &[&fan.fan_index.to_string()]))
}

pub fn pwm_label(pwm: Option<f32>) -> String {
    pwm.map(|p| i18n_f(n_("{}%"), &[&format!("{:.0}", p * 100.)]))
        .unwrap_or_default()
}

pub fn rpm_label(rpm: Option<u32>) -> String {
    rpm.map(|rpm| i18n_f("{} RPM", &[&rpm.to_string()]))
        .unwrap_or_else(|| "—".into())
}

pub struct FanEntry {
    key: String,
    title: String,
    chip: String,
    channel: String,
    speed_graph: Graph,
    max_rpm: Option<u32>,
    has_pwm: bool,
    speed: String,
    pwm: String,
    speed_max_y: String,
}

impl FanEntry {
    pub fn new(fan: &Fan) -> Self {
        let mut speed_graph = Graph::new();
        speed_graph.set_base_color(COLOR);
        let mut rpm = DatasetGroup::new_with_fill(f32::NAN);
        rpm.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
        let mut pwm = DatasetGroup::new_with_fill(f32::NAN);
        pwm.dataset_settings.dashed = true;
        pwm.dataset_settings.fill = FillingSettings::None;
        pwm.dataset_settings.high_watermark = 1.;
        speed_graph.add_dataset(rpm);
        speed_graph.add_dataset(pwm);
        Self {
            key: key(fan),
            title: String::new(),
            chip: String::new(),
            channel: format!("fan{}", fan.fan_index),
            speed_graph,
            max_rpm: None,
            has_pwm: false,
            speed: String::new(),
            pwm: String::new(),
            speed_max_y: String::new(),
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn update(&mut self, fan: &Fan, chip_title: &str) {
        self.title = label(fan);
        self.chip = chip_title.to_owned();
        let rpm = rpm(fan);
        self.speed = rpm_label(rpm);
        self.has_pwm |= fan.pwm_percent.is_some();
        self.pwm = match fan.pwm_percent {
            Some(p) => pwm_label(Some(p)),
            None => "—".into(),
        };
        // The reported maximum may appear or change while the daemon rescans.
        if fan.max_rpm != self.max_rpm {
            self.max_rpm = fan.max_rpm;
            let scaling = match self.max_rpm {
                Some(_) => ScalingSettings::Fixed,
                None => ScalingSettings::StickyUp,
            };
            self.speed_graph.set_dataset_scaling(0, scaling);
            if let Some(max) = self.max_rpm {
                self.speed_graph.set_dataset_max_scale(0, max as f32);
            }
        }
        self.speed_graph.add_data_point(vec![
            vec![rpm.map_or(f32::NAN, |rpm| rpm as f32)],
            vec![fan.pwm_percent.unwrap_or(f32::NAN)],
        ]);
        self.speed_max_y = i18n_f(
            "{} RPM",
            &[&self.speed_graph.dataset_max_scale(0).round().to_string()],
        );
    }

    pub fn set_grid_offset(&mut self, offset: u32) -> bool {
        self.speed_graph.set_grid_offset(offset)
    }

    pub fn detail_view(&mut self, max_duration: &str) -> PageView<'_> {
        let mut stats = vec![(i18n("Fan Speed"), self.speed.clone())];
        let mut stat_legends = vec![((0, 0), StatLegend::Solid)];
        if self.has_pwm {
            stats.push((i18n("PWM Percentage"), self.pwm.clone()));
            stat_legends.push(((0, 1), StatLegend::Dashed));
        }
        let mut info = vec![(i18n("Sensor:"), format!("{} · {}", self.chip, self.channel))];
        if let Some(max) = self.max_rpm {
            info.push((
                i18n("Maximum speed:"),
                i18n_f("{} RPM", &[&max.to_string()]),
            ));
        }
        PageView {
            model_tooltip: None,
            title: self.title.clone(),
            model: self.title.clone(),
            panels: vec![GraphPanel::single(
                over(n_("Fan speed over "), max_duration),
                self.speed_max_y.clone(),
                &mut self.speed_graph,
            )],
            stats: vec![stats],
            stat_legends,
            info,
            composition: None,
            swap_compression: None,
        }
    }

    pub fn data_summary(&self) -> String {
        let unsupported = i18n("Unsupported");
        format!(
            r#"Fan

    {}
    {} · {}

    Speed:               {}
    PWM Percentage:      {}
    Maximum speed:       {}"#,
            self.title,
            self.chip,
            self.channel,
            self.speed,
            if self.has_pwm {
                self.pwm.clone()
            } else {
                unsupported.clone()
            },
            self.max_rpm
                .map(|max| i18n_f("{} RPM", &[&max.to_string()]))
                .unwrap_or(unsupported),
        )
    }
}
