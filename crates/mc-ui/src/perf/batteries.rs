/* crates/mc-ui/src/perf/batteries.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! One sidebar page; each child's identity, history and details remain independent.

use std::collections::{HashMap, HashSet};

use egui::Color32;
use mc_core::config::Config;
use mc_core::i18n::{i18n, ni18n_f};
use mc_core::sidebar::DeviceType;
use mc_graph::{DatasetGroup, FillingSettings, ScalingSettings};
use mc_magpie::magpie_types::battery::BatteryType;
use mc_magpie::{Battery, Readings};

use super::battery::{self, BatteryPage};
use super::{DevicePage, GraphPanel, PageView, SummaryGraph, UpdateEnv};
use crate::win10::palette::{BATTERY, BATTERY_SERIES};

pub struct BatteryCard {
    pub key: String,
    pub name: String,
    pub kind: BatteryType,
    pub kind_label: String,
    pub percentage: Option<f32>,
    pub percentage_label: String,
    pub state: String,
    pub accessible_label: String,
    pub colors: [Color32; 2],
}

pub struct BatteriesView<'a> {
    pub overview: PageView<'a>,
    pub cards: &'a [BatteryCard],
    pub detail: Option<PageView<'a>>,
}

struct Entry {
    page: BatteryPage,
    color: usize,
}

pub struct BatteriesPage {
    entries: Vec<Entry>,
    cards: Vec<BatteryCard>,
    summary: SummaryGraph,
    caption: String,
}

impl Default for BatteriesPage {
    fn default() -> Self {
        Self::new()
    }
}

impl BatteriesPage {
    pub fn new() -> Self {
        let mut summary = SummaryGraph::new(BATTERY.line, i18n("Batteries"));
        summary.graph.set_grid_visible(true);
        Self {
            entries: vec![],
            cards: vec![],
            summary,
            caption: String::new(),
        }
    }

    fn reconcile(&mut self, batteries: &[&Battery], config: &Config) {
        if self.entries.len() == batteries.len()
            && self
                .entries
                .iter()
                .zip(batteries)
                .all(|(entry, b)| entry.page.name() == battery::page_name(b))
        {
            return;
        }

        let groups = std::mem::take(self.summary.graph.groups_mut());
        let mut previous: HashMap<_, _> = self
            .entries
            .drain(..)
            .zip(groups)
            .map(|(entry, group)| (entry.page.name().to_owned(), (entry, group)))
            .collect();
        let present: HashSet<_> = batteries.iter().map(|b| battery::page_name(b)).collect();
        let mut colors: HashSet<_> = previous
            .iter()
            .filter(|(key, _)| present.contains(*key))
            .map(|(_, (entry, _))| entry.color)
            .collect();
        self.summary.graph.clear_datasets();
        for b in batteries {
            let (entry, group) = previous.remove(&battery::page_name(b)).unwrap_or_else(|| {
                let mut color = 0;
                while colors.contains(&color) {
                    color += 1;
                }
                colors.insert(color);
                let mut group = DatasetGroup::new_with_fill(f32::NAN);
                group.dataset_settings.scaling_settings = ScalingSettings::Fixed;
                group.dataset_settings.low_watermark = 0.;
                group.dataset_settings.high_watermark = 100.;
                group.dataset_settings.fill = FillingSettings::None;
                group.dataset_settings.vertical_dropoff_lines = false;
                (
                    Entry {
                        page: BatteryPage::new(b, config),
                        color,
                    },
                    group,
                )
            });
            self.entries.push(entry);
            // add_dataset also normalizes a new group's window to the graph's full 60 samples.
            self.summary.graph.add_dataset(group);
        }
        self.summary.graph.set_group_colors(
            self.entries
                .iter()
                .map(|entry| BATTERY_SERIES[entry.color % BATTERY_SERIES.len()])
                .collect(),
        );
    }
}

fn overview<'a>(summary: &'a mut SummaryGraph, caption: &str) -> PageView<'a> {
    PageView {
        model_tooltip: None,
        title: summary.heading.clone(),
        model: summary.info2.clone(),
        panels: vec![GraphPanel::single(
            caption.into(),
            i18n("100%"),
            &mut summary.graph,
        )],
        stats: vec![],
        stat_legends: vec![],
        info: vec![],
        composition: None,
        swap_compression: None,
    }
}

impl DevicePage for BatteriesPage {
    fn name(&self) -> &str {
        "battery"
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

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let mut seen = HashSet::new();
        let mut batteries: Vec<_> = readings
            .batteries
            .iter()
            .filter(|b| seen.insert(battery::page_name(b)))
            .collect();
        batteries.sort_by_cached_key(|b| {
            (
                !matches!(b.kind(), BatteryType::Bat | BatteryType::Ups),
                battery::page_name(b),
            )
        });
        if batteries.is_empty() {
            return false;
        }
        self.reconcile(&batteries, env.config);
        self.cards.clear();
        for (entry, b) in self.entries.iter_mut().zip(batteries) {
            entry.page.update(readings, env);
            let mut card = BatteryCard {
                key: entry.page.name().into(),
                name: if b.vendor.is_none() && b.model.is_none() {
                    b.name.clone()
                } else {
                    battery::model_label(b)
                },
                kind: b.kind(),
                kind_label: b
                    .kind
                    .map(battery::kind_to_str)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| i18n("Battery")),
                percentage: battery::percentage(b.percentage),
                percentage_label: battery::percentage_label(b.percentage),
                state: b
                    .state
                    .map(battery::state_to_str)
                    .unwrap_or_else(|| i18n("Unknown")),
                accessible_label: String::new(),
                colors: BATTERY_SERIES[entry.color % BATTERY_SERIES.len()],
            };
            card.accessible_label = format!(
                "{} — {}: {}, {}",
                card.name, card.kind_label, card.percentage_label, card.state
            );
            self.cards.push(card);
        }
        self.summary.graph.add_data_point(
            self.cards
                .iter()
                .map(|card| vec![card.percentage.unwrap_or(f32::NAN) * 100.])
                .collect(),
        );
        self.summary.info2 = ni18n_f(
            "{} device",
            "{} devices",
            self.cards.len() as u32,
            &[&self.cards.len().to_string()],
        );
        self.summary.info1 = self
            .cards
            .iter()
            .map(|card| card.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        self.summary.tooltip = Some(
            self.cards
                .iter()
                .map(|card| format!("{}: {} — {}", card.name, card.percentage_label, card.state))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        self.caption = format!(
            "{}{}{}",
            i18n("Percentage"),
            i18n(" over "),
            env.max_duration
        );
        true
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        // PerformancePage advances summary.graph separately, once per global clock tick.
        self.entries.iter_mut().fold(false, |repaint, entry| {
            entry.page.set_grid_offset(offset) | repaint
        })
    }

    fn data_summary(&self) -> String {
        self.entries
            .iter()
            .map(|entry| format!("{}\n{}", entry.page.name(), entry.page.data_summary()))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn win10_view(&mut self, _config: &Config) -> PageView<'_> {
        overview(&mut self.summary, &self.caption)
    }

    fn win10_batteries_view(
        &mut self,
        config: &Config,
        selected: Option<&str>,
    ) -> Option<BatteriesView<'_>> {
        let detail = self
            .entries
            .iter_mut()
            .find(|entry| Some(entry.page.name()) == selected)
            .map(|entry| entry.page.detail_view(config));
        Some(BatteriesView {
            overview: overview(&mut self.summary, &self.caption),
            cards: &self.cards,
            detail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyed_charge_histories_and_all_child_copy() {
        let config = Config::default();
        let env = UpdateEnv::new(&config);
        let mut readings = mc_magpie::synthetic_readings(0, 8);
        readings.batteries = vec![Battery {
            name: "BAT0".into(),
            model: Some("Laptop".into()),
            kind: Some(2),
            power_supply: Some(false),
            percentage: 0.,
            ..Default::default()
        }];
        let mut page = BatteriesPage::new();
        assert!(
            page.summary.graph.groups().is_empty(),
            "construction is sample-free"
        );
        for _ in 0..12 {
            assert!(page.update(&readings, &env));
        }
        let host_key = battery::page_name(&readings.batteries[0]);
        let host_color = page.cards[0].colors;
        let mouse = Battery {
            name: "mouse-a".into(),
            model: Some("Same model".into()),
            kind: Some(5),
            power_supply: Some(false),
            percentage: 1.,
            ..Default::default()
        };
        let mouse_key = battery::page_name(&mouse);
        readings.batteries.push(mouse.clone());
        readings.batteries.push(mouse); // Exact duplicates collapse, not equal model names.
        readings.batteries.push(Battery {
            name: "mouse-b".into(),
            model: Some("Same model".into()),
            kind: Some(5),
            power_supply: Some(false),
            percentage: f32::NAN,
            ..Default::default()
        });
        assert!(page.update(&readings, &env));
        assert_eq!(page.cards.len(), 3);
        assert_eq!(page.cards[0].key, host_key);
        assert_eq!(page.cards[0].colors, host_color);
        let groups = page.summary.graph.groups();
        assert_eq!(groups.len(), 3);
        let host_data = groups[0].datas[0].get_data();
        assert_eq!(host_data.len(), 60);
        assert_eq!(host_data.iter().filter(|v| v.is_finite()).count(), 13);
        let data = groups[1].datas[0].get_data();
        assert_eq!(data[0], 100.);
        assert!(data[1..].iter().all(|v| v.is_nan()));
        let mouse_color = page.cards[1].colors;
        assert_ne!(mouse_color, host_color);
        assert_eq!(page.cards[2].percentage_label, "—");
        assert!(groups[2].datas[0].get_data().iter().all(|v| v.is_nan()));
        let copy = page.data_summary();
        for key in [&host_key, &mouse_key, "battery-1-mouse-b"] {
            assert!(copy.contains(key));
        }
        assert!(copy.contains("Percentage:             0%"));
        assert!(copy.contains("Percentage:             100%"));
        assert!(copy.contains("Percentage:             —"));

        readings.batteries.reverse();
        assert!(page.update(&readings, &env));
        assert_eq!(page.cards[1].key, mouse_key);
        assert_eq!(page.cards[1].colors, mouse_color);
        assert_eq!(
            page.summary.graph.groups()[1].datas[0].get_data()[..2],
            [100., 100.]
        );
        // Same-count replacement must not inherit the removed child's history.
        readings.batteries.retain(|b| b.name != "mouse-a");
        readings.batteries.push(Battery {
            name: "mouse-c".into(),
            percentage: 0.4,
            kind: Some(5),
            power_supply: Some(false),
            ..Default::default()
        });
        assert!(page.update(&readings, &env));
        let last = page.summary.graph.groups().last().unwrap().datas[0].get_data();
        assert_eq!(last[0], 40.);
        assert!(last[1..].iter().all(|v| v.is_nan()));
        assert_eq!(page.cards[0].colors, host_color);
        let generation = page.summary.graph.generation();
        for _ in 0..2 {
            drop(page.win10_batteries_view(&config, Some(&host_key)));
        }
        assert_eq!(
            page.summary.graph.generation(),
            generation,
            "views do not feed histories"
        );

        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1, 1.1] {
            readings.batteries[0].percentage = invalid;
            page.update(&readings, &env);
            let key = battery::page_name(&readings.batteries[0]);
            let view = page.win10_batteries_view(&config, Some(&key)).unwrap();
            let card = view.cards.iter().find(|c| c.key == key).unwrap();
            assert_eq!(card.percentage, None);
            assert_eq!(card.percentage_label, "—");
            assert_eq!(view.detail.unwrap().stats[0][0].1, "—");
            assert!(page.data_summary().contains("Percentage:             —"));
        }
    }
}
