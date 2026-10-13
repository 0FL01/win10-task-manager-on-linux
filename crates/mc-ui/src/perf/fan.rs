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

use super::{GraphPanel, PageView, over};
use mc_core::config::Config;
use mc_core::i18n::n_;
use mc_core::i18n::{i18n, i18n_f};
use mc_core::sidebar::DeviceType;
use mc_graph::{DatasetGroup, FillingSettings, Graph, ScalingSettings};
use mc_magpie::{Fan, Readings};

use super::cpu::{TEMPERATURE_HIGH_WATERMARK, TEMPERATURE_LOW_WATERMARK};
use super::{DevicePage, MK_TO_0_C, SummaryGraph, UpdateEnv};
const COLOR: egui::Color32 = crate::win10::palette::FAN.line;

pub fn page_name(fan: &Fan) -> String {
    format!("fan-{}-{}", fan.hwmon_index, fan.fan_index)
}

fn temp_c(milli_k: u32) -> f32 {
    (milli_k as i32 + MK_TO_0_C) as f32 / 1000.
}

pub struct FanPage {
    name: String,
    hwmon_index: u32,
    fan_index: u32,
    index: Option<usize>,
    summary: SummaryGraph,
    speed_graph: Graph,
    temp_graph: Graph,
    title: String,
    temp_name: String,
    has_pwm: bool,
    has_temp: bool,
    max_rpm: Option<u32>,
    speed: String,
    pwm: String,
    temp: String,
    speed_max_y: String,
    temp_max_y: String,
    max_duration: String,
}

impl FanPage {
    pub fn new(fan: &Fan, config: &Config) -> Self {
        let mut summary = SummaryGraph::new(COLOR, i18n("Fan"));
        let mut s = DatasetGroup::new();
        s.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
        summary.graph.add_dataset(s);

        let mut temp_graph = Graph::new();
        temp_graph.set_base_color(COLOR);
        let mut t = DatasetGroup::new();
        t.dataset_settings.scaling_settings = ScalingSettings::StickyUpDown;
        t.dataset_settings.high_watermark = TEMPERATURE_HIGH_WATERMARK;
        t.dataset_settings.low_watermark = TEMPERATURE_LOW_WATERMARK;
        temp_graph.add_dataset(t);

        let mut speed_graph = Graph::new();
        speed_graph.set_base_color(COLOR);
        let mut rpm = DatasetGroup::new();
        rpm.dataset_settings.scaling_settings = ScalingSettings::StickyUp;
        let mut pwm = DatasetGroup::new();
        pwm.dataset_settings.dashed = true;
        pwm.dataset_settings.fill = FillingSettings::None;
        pwm.dataset_settings.high_watermark = 1.;
        speed_graph.add_dataset(rpm);
        speed_graph.add_dataset(pwm);

        let mut speed_max_y = String::new();
        if let Some(max) = fan.max_rpm {
            speed_max_y = max.to_string();
            speed_graph.set_dataset_scaling(0, ScalingSettings::Fixed);
            speed_graph.set_dataset_max_scale(0, max as f32);
        }

        Self {
            name: page_name(fan),
            hwmon_index: fan.hwmon_index,
            fan_index: fan.fan_index,
            index: None,
            summary,
            speed_graph,
            temp_graph,
            title: i18n("Fan"),
            temp_name: fan.temp_name.clone().unwrap_or_default(),
            has_pwm: fan.pwm_percent.is_some(),
            has_temp: fan.temp_amount.is_some(),
            max_rpm: fan.max_rpm,
            speed: String::new(),
            pwm: String::new(),
            temp: String::new(),
            speed_max_y,
            temp_max_y: String::new(),
            max_duration: super::max_duration(config),
        }
    }
}

impl DevicePage for FanPage {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Fan
    }

    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }

    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }

    fn set_index(&mut self, index: Option<usize>) {
        self.index = index;
        self.summary.heading = match index {
            Some(i) => i18n_f("Fan {}", &[&i.to_string()]),
            None => i18n("Fan"),
        };
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let Some(fan) = readings
            .fans
            .iter()
            .find(|f| f.hwmon_index == self.hwmon_index && f.fan_index == self.fan_index)
        else {
            return false;
        };
        self.max_duration = env.max_duration.clone();

        self.title = match (&fan.fan_label, self.index) {
            (Some(label), _) => label.clone(),
            (None, Some(i)) => i18n_f("Fan {}", &[&i.to_string()]),
            (None, None) => i18n("Fan"),
        };
        self.speed = i18n_f("{} RPM", &[&fan.rpm.to_string()]);
        self.pwm = i18n_f(
            n_("{}%"),
            &[&format!("{:.0}", fan.pwm_percent.unwrap_or(0.) * 100.)],
        );
        let temp = fan.temp_amount.map(temp_c);
        if let Some(t) = temp {
            self.temp = i18n_f("{} °C", &[&format!("{t:.1}")]);
            self.temp_graph.add_data_point(vec![vec![t]]);
            self.temp_max_y = format!("{} °C", self.temp_graph.dataset_max_scale(0));
        }
        self.speed_graph.add_data_point(vec![
            vec![fan.rpm as f32],
            vec![fan.pwm_percent.unwrap_or(0.)],
        ]);
        if self.max_rpm.is_none() {
            self.speed_max_y = i18n_f(
                n_("{} RPM"),
                &[&self.speed_graph.dataset_max_scale(0).to_string()],
            );
        }

        self.summary
            .graph
            .add_data_point(vec![vec![fan.rpm as f32]]);
        self.summary.info1 = fan
            .fan_label
            .clone()
            .or_else(|| fan.temp_name.clone())
            .unwrap_or_default();
        let temp_str = temp.map(|t| format!(" ({t:.0} °C)")).unwrap_or_default();
        self.summary.info2 = match fan.pwm_percent {
            Some(p) => format!("{:.0}%{temp_str}", p * 100.),
            None => format!("{} RPM{temp_str}", fan.rpm),
        };
        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        self.speed_graph.set_grid_offset(offset) | self.temp_graph.set_grid_offset(offset)
    }

    fn win10_view(&mut self, _config: &Config) -> PageView<'_> {
        let mut first = vec![(i18n("Fan Speed"), self.speed.clone())];
        if self.has_pwm {
            first.push((i18n("PWM Percentage"), self.pwm.clone()));
        }
        let mut stats = vec![first];
        if self.has_temp {
            stats.push(vec![(i18n("Current Temperature"), self.temp.clone())]);
        }
        let mut panels = vec![GraphPanel::single(
            over(n_("Fan speed over "), &self.max_duration),
            self.speed_max_y.clone(),
            &mut self.speed_graph,
        )];
        if self.has_temp {
            panels.push(GraphPanel::single(
                over(n_("Temperature over "), &self.max_duration),
                self.temp_max_y.clone(),
                &mut self.temp_graph,
            ));
        }
        PageView {
            title: self.title.clone(),
            model: self.temp_name.clone(),
            panels,
            stats,
            info: vec![],
            composition: None,
            swap_compression: None,
        }
    }

    fn data_summary(&self) -> String {
        let unsupported = i18n("Unsupported");
        let or = |v: &str, shown: bool| {
            if shown && !v.is_empty() {
                v.to_owned()
            } else {
                unsupported.clone()
            }
        };
        format!(
            r#"Fan

    {}
    {}

    Speed:               {}
    PWM Percentage:      {}
    Current Temperature: {}"#,
            self.title,
            self.temp_name,
            or(&self.speed, true),
            or(&self.pwm, self.has_pwm),
            or(&self.temp, self.has_temp),
        )
    }
}
