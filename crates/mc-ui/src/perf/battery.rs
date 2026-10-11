/* crates/mc-ui/src/perf/battery.rs
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

use super::{GraphPanel, PageView};
use mc_core::config::Config;
use mc_core::format::{DataType, to_human_readable_nice, to_long_human_readable_time};
use mc_core::i18n::n_;
use mc_core::i18n::{i18n, i18n_f};
use mc_core::sidebar::DeviceType;
use mc_graph::{DatasetGroup, FillingSettings, Graph, RoundingSettings, ScalingSettings};
use mc_magpie::{Battery, Readings};

use super::{DevicePage, SummaryGraph, UpdateEnv};
const COLOR: egui::Color32 = crate::win10::palette::BATTERY.line;

const TOTAL_SECS: f32 = 3600. * 24. * 7.;

pub fn page_name(b: &Battery) -> String {
    format!(
        "battery-{}-{}",
        b.power_supply.map(|x| !x as u8).unwrap_or(2),
        b.name
    )
}

fn kind_to_str(kind: i32) -> String {
    let s = match kind {
        1 => "LinePower",
        2 => n_("Battery"),
        3 => "UPS",
        4 => "Monitor",
        5 => "Mouse",
        6 => "Keyboard",
        7 => "PDA",
        8 => "Phone",
        9 => "Media player",
        10 => "Tablet",
        11 => "Computer",
        12 => "Gaming Input",
        13 => "Pen",
        14 => "Touchpad",
        15 => "Modem",
        16 => n_("Network"),
        17 => "Headset",
        18 => "Speakers",
        19 => "Headphones",
        20 => "Video",
        21 => "Other Audio",
        22 => "Remote Control",
        23 => "Printer",
        34 => "Scanner",
        35 => "Camera",
        36 => "Wearable",
        37 => "Toy",
        38 => "Generic bluetooth",
        _ => return String::new(),
    };
    i18n(s)
}

fn technology_to_str(t: i32) -> String {
    let s = match t {
        1 => "Lithium Ion",
        2 => "Lithium Polymer",
        3 => "Lithium Iron Phosphate",
        4 => "Lead Acid",
        5 => "Nickel Cadmium",
        6 => "Nickel Metal Hydride",
        _ => return String::new(),
    };
    i18n(s)
}

fn state_to_str(s: i32) -> String {
    let s = match s {
        1 => "Charging",
        2 => "Discharging",
        3 => n_("Empty"),
        4..=6 => n_("Full"),
        _ => return String::new(),
    };
    i18n(s)
}

#[derive(Default)]
struct Labels {
    percentage: String,
    energy: Option<String>,
    voltage: Option<String>,
    power: Option<String>,
    state: String,
    time_to: Option<(String, String)>,
    charge_cycles: Option<String>,
    threshold: Option<String>,
    threshold_start: Option<String>,
    threshold_end: Option<String>,
    rate_max_y: String,
    rate_min_y: Option<String>,
}

pub struct BatteryPage {
    name: String,
    battery_name: String,
    power_supply: Option<bool>,
    index: Option<usize>,
    summary: SummaryGraph,
    rate_graph: Graph,
    history_graph: Graph,
    has_history: bool,
    power_mode: bool,
    title: String,
    model: String,
    plain_model: String,
    serial: Option<String>,
    kind: Option<String>,
    power_supply_label: Option<String>,
    technology: Option<String>,
    capacity: Option<String>,
    energy_empty: Option<String>,
    energy_full: Option<String>,
    energy_full_design: Option<String>,
    voltage_min_design: Option<String>,
    voltage_max_design: Option<String>,
    labels: Labels,
    max_duration: String,
}

impl BatteryPage {
    pub fn new(b: &Battery, config: &Config) -> Self {
        let wh = |v: u32| to_human_readable_nice(v as f32 / 1000., &DataType::WattHours);
        let volts = |v: f32| to_human_readable_nice(v, &DataType::Volts);

        let mut summary = SummaryGraph::percent(COLOR, i18n("Battery"));
        summary.info1 = b.model.clone().unwrap_or_default();

        let power_mode = b.power_supply == Some(true) && b.state.is_some();
        let mut rate_graph = Graph::new();
        rate_graph.set_base_color(COLOR);
        if power_mode {
            let mut d = DatasetGroup::new_with_fill(0.);
            d.dataset_settings.scaling_settings = ScalingSettings::StickyUpDownEqualMagnitude;
            d.dataset_settings.high_watermark = 0.;
            d.dataset_settings.low_watermark = 0.;
            d.dataset_settings.fill = FillingSettings::FillToZero;
            d.dataset_settings.rounding_settings = RoundingSettings::Pow10;
            rate_graph.add_dataset(d);
        } else {
            let mut d = DatasetGroup::new();
            d.dataset_settings.high_watermark = 1.;
            rate_graph.add_dataset(d);
        }

        let model = match (&b.vendor, &b.model) {
            (Some(v), Some(m)) => format!("{v} {m}"),
            (Some(v), None) => v.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => i18n("Unknown"),
        };
        let kind = b.kind.map(kind_to_str);

        let mut page = Self {
            name: page_name(b),
            battery_name: b.name.clone(),
            power_supply: b.power_supply,
            index: None,
            summary,
            rate_graph,
            history_graph: Graph::new(),
            has_history: false,
            power_mode,
            title: kind.clone().unwrap_or_else(|| i18n("Battery")),
            model,
            plain_model: b.model.clone().unwrap_or_default(),
            serial: b.serial.clone(),
            kind,
            power_supply_label: b
                .power_supply
                .map(|p| if p { i18n("Yes") } else { i18n("No") }),
            technology: b.technology.map(technology_to_str),
            capacity: b
                .capacity
                .map(|c| i18n_f("{}%", &[&format!("{:.0}", c * 100.)])),
            energy_empty: b.energy_empty.map(wh),
            energy_full: b.energy_full.map(wh),
            energy_full_design: b.energy_full_design.map(wh),
            voltage_min_design: b.voltage_min_design.map(volts),
            voltage_max_design: b.voltage_max_design.map(volts),
            labels: Labels::default(),
            max_duration: super::max_duration(config),
        };
        page.history_graph.set_base_color(COLOR);
        if b.power_supply == Some(true) && b.history.len() >= 2 {
            page.update_history(b);
        }
        page
    }

    fn update_history(&mut self, b: &Battery) {
        let d: Vec<(f32, f32)> = b.history.iter().map(|p| (p.x, p.y)).collect();
        if d.len() < 2 {
            return;
        }
        let mut first = d[d.len() - 1];
        if first.1.is_nan() {
            first = d[d.len() - 2];
        }

        let mut his_interpol = vec![
            (TOTAL_SECS, first.1),
            (first.0, first.1),
            (first.0, f32::NAN),
        ];
        let mut his = vec![first];
        for w in d.windows(3).rev() {
            if w[1].1.is_nan() {
                his_interpol.push(w[0]);
                his_interpol.push(w[2]);
                his_interpol.push((w[2].0, f32::NAN));
            }
            his.push(w[1]);
        }
        his.push((0., b.percentage));

        let mut main = DatasetGroup::new_with_datas(vec![his]);
        main.dataset_settings.high_watermark = 1.;
        main.dataset_settings.vertical_dropoff_lines = false;
        let mut interpol = DatasetGroup::new_with_datas(vec![his_interpol]);
        interpol.dataset_settings.high_watermark = 1.;
        interpol.dataset_settings.dashed = true;
        interpol.dataset_settings.vertical_dropoff_lines = false;

        let g = &mut self.history_graph;
        g.clear_datasets();
        g.set_data_points(TOTAL_SECS as u32);
        g.add_dataset(main);
        g.add_dataset(interpol);
        for group in g.groups_mut() {
            for set in group.datas.iter_mut() {
                set.used_data = usize::MAX;
            }
        }
        self.has_history = true;
    }
}

impl DevicePage for BatteryPage {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Battery
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
            Some(i) => i18n_f("Battery {}", &[&i.to_string()]),
            None => i18n("Battery"),
        };
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let Some(b) = readings
            .batteries
            .iter()
            .find(|b| b.name == self.battery_name && b.power_supply == self.power_supply)
        else {
            return false;
        };
        self.max_duration = env.max_duration.clone();

        self.title = match (&self.kind, self.index) {
            (Some(k), Some(i)) if !k.is_empty() => format!("{k} {i}"),
            (Some(k), None) if !k.is_empty() => k.clone(),
            (_, Some(i)) => i18n_f("Battery {}", &[&i.to_string()]),
            (_, None) => i18n("Battery"),
        };

        let discharging = b.state == Some(2);
        let power = b.power.map(|p| if discharging && p != 0. { -p } else { p });

        let l = &mut self.labels;
        l.percentage = i18n_f("{}%", &[&format!("{:.0}", b.percentage * 100.)]);
        l.voltage = b
            .voltage
            .map(|v| to_human_readable_nice(v, &DataType::Volts));
        l.energy = b
            .energy
            .map(|e| to_human_readable_nice(e as f32 / 1000., &DataType::WattHours));
        l.power = power.map(|p| to_human_readable_nice(p, &DataType::Watts));
        l.time_to = if let Some(t) = b.time_to_full {
            Some((i18n("Full"), to_long_human_readable_time(t as u64)))
        } else {
            b.time_to_empty
                .map(|t| (i18n("Empty"), to_long_human_readable_time(t as u64)))
        };
        l.state = b.state.map(state_to_str).unwrap_or_else(|| i18n("Unknown"));
        l.charge_cycles = b.charge_cycles.map(|c| c.to_string());
        if b.charge_threshold_supported != 0 {
            l.threshold = Some(
                if b.charge_threshold_enabled && b.charge_threshold_supported & 4 != 0 {
                    i18n("Firmware")
                } else if b.charge_threshold_enabled {
                    i18n("Yes")
                } else {
                    i18n("No")
                },
            );
            let pct = |v: Option<u32>| {
                v.filter(|_| b.charge_threshold_enabled)
                    .map(|v| i18n_f("{}%", &[&v.to_string()]))
            };
            l.threshold_start = pct(b.charge_start_threshold);
            l.threshold_end = pct(b.charge_end_threshold);
        } else {
            l.threshold = None;
            l.threshold_start = None;
            l.threshold_end = None;
        }

        if self.power_mode {
            self.rate_graph
                .add_data_point(vec![vec![power.unwrap_or(0.)]]);
            l.rate_max_y =
                to_human_readable_nice(self.rate_graph.dataset_max_scale(0), &DataType::Watts);
            l.rate_min_y = Some(to_human_readable_nice(
                self.rate_graph.dataset_min_scale(0),
                &DataType::Watts,
            ));
        } else {
            self.rate_graph.add_data_point(vec![vec![b.percentage]]);
            l.rate_max_y = i18n("100%");
            l.rate_min_y = None;
        }

        if b.history_changed && b.history.len() >= 2 && b.power_supply == Some(true) {
            self.update_history(b);
        }

        self.summary
            .graph
            .add_data_point(vec![vec![b.percentage * 100.]]);
        self.summary.info1 = self.plain_model.clone();
        let temp = b.temp.map(|t| format!(" ({t} °C)")).unwrap_or_default();
        self.summary.info2 = format!("{:.0}%{temp}", b.percentage * 100.);
        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        self.rate_graph.set_grid_offset(offset)
    }

    fn win10_view(&mut self, _config: &Config) -> PageView<'_> {
        let l = &self.labels;
        let mut first = vec![(i18n("Percentage"), l.percentage.clone())];
        if let Some(e) = &l.energy {
            first.push((i18n("Energy"), e.clone()));
        }
        let mut second = vec![];
        if let Some(v) = &l.voltage {
            second.push((i18n("Voltage"), v.clone()));
        }
        if let Some(p) = &l.power {
            second.push((i18n("Power"), p.clone()));
        }
        let mut third = vec![(i18n("State"), l.state.clone())];
        if let Some((direction, time)) = &l.time_to {
            third.push((format!("{}{}", i18n("Time to "), direction), time.clone()));
        }
        if let Some(c) = &l.charge_cycles {
            third.push((i18n("Charge Cycles"), c.clone()));
        }
        let stats: Vec<_> = [first, second, third]
            .into_iter()
            .filter(|r| !r.is_empty())
            .collect();
        let mut info = vec![];
        let mut push = |label: &str, v: &Option<String>| {
            if let Some(v) = v
                && !v.is_empty()
            {
                info.push((i18n(label), v.clone()));
            }
        };
        push(n_("Serial:"), &self.serial);
        push(n_("Type:"), &self.kind);
        push("Powering System:", &self.power_supply_label);
        push("Technology:", &self.technology);
        push(n_("Capacity:"), &self.capacity);
        push("Energy Empty:", &self.energy_empty);
        push("Energy Full:", &self.energy_full);
        push("Energy Full (design):", &self.energy_full_design);
        push("Charge Threshold:", &l.threshold);
        let label = if self.power_mode {
            i18n("Power Input")
        } else {
            i18n("Percentage")
        };
        let rate_caption = format!("{label}{}{}", i18n(" over "), self.max_duration);
        let rate_max = l.rate_max_y.clone();
        let mut panels = vec![GraphPanel::single(
            rate_caption,
            rate_max,
            &mut self.rate_graph,
        )];
        if self.has_history {
            panels.push(GraphPanel::single(
                i18n("Battery Charge over past week"),
                i18n("100%"),
                &mut self.history_graph,
            ));
        }
        PageView {
            title: self.title.clone(),
            model: self.model.clone(),
            panels,
            stats,
            info,
        }
    }

    fn data_summary(&self) -> String {
        let l = &self.labels;
        let unsupported = i18n("Unsupported");
        let o = |v: &Option<String>| {
            v.clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| unsupported.clone())
        };
        let time_to_line = match &l.time_to {
            Some((direction, time)) => format!("\n    Time to {direction:<5}:          {time}"),
            None => String::new(),
        };
        format!(
            r#"Battery

    {} {}

    Type:                   {}
    Energy Full:            {}
    Technology:             {}
    capacity:               {}
    Energy Full (design):   {}
    Energy Empty            {}
    Voltage Max (design):   {}
    Voltage Min (design):   {}
    Powering System:        {}

    Percentage:             {}
    State:                  {}{}
    Charge Cycles:          {}
    Power Output:           {}
    Voltage:                {}

    Charge Threshold:       {}
    Charge Start Threshold: {}
    Charge End Threshold:   {}"#,
            self.model,
            o(&self.serial),
            o(&self.kind),
            o(&self.energy_full),
            o(&self.technology),
            o(&self.capacity),
            o(&self.energy_full_design),
            o(&self.energy_empty),
            o(&self.voltage_max_design),
            o(&self.voltage_min_design),
            o(&self.power_supply_label),
            l.percentage,
            l.state,
            time_to_line,
            o(&l.charge_cycles),
            o(&l.power),
            o(&l.voltage),
            o(&l.threshold),
            o(&l.threshold_start),
            o(&l.threshold_end),
        )
    }
}
