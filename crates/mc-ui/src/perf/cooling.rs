/* crates/mc-ui/src/perf/cooling.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! The consolidated Cooling page: every hwmon fan and temperature sensor under one sidebar
//! entry, grouped by the device that reports them. Children keep independent histories.

use std::collections::{HashMap, HashSet};

use egui::Color32;
use mc_core::config::Config;
use mc_core::i18n::{i18n, i18n_f, n_, ni18n_f};
use mc_core::sidebar::DeviceType;
use mc_graph::{DatasetGroup, FillingSettings, ScalingSettings};
use mc_magpie::{Fan, HwmonChip, Readings, TemperatureSensor};

use super::fan::{self, FanEntry};
use super::temperature::{self, Level, SensorEntry};
use super::{DevicePage, GraphPanel, PageView, SummaryGraph, UpdateEnv, over};
use crate::win10::palette::{COOLING_SERIES, FAN};

/// Smallest overview axis; the shared scale only grows, in whole steps.
const RPM_SCALE_MIN: f32 = 1000.;
const RPM_SCALE_STEP: f32 = 500.;

pub struct FanCard {
    pub key: String,
    pub name: String,
    /// Valid speed; drives the glyph's rotor
    pub rpm: Option<u32>,
    pub rpm_label: String,
    /// "PWM 21%", empty without PWM; formatted once per reading, not per frame
    pub pwm_label: String,
    /// PWM duty cycle, otherwise speed relative to the reported maximum
    pub bar: Option<f32>,
    pub bar_tooltip: String,
    pub stopped: bool,
    pub accessible_label: String,
    /// Overview series ink, [Light, Dark]; fans that never spun have no line
    pub colors: Option<[Color32; 2]>,
}

pub struct SensorCard {
    pub key: String,
    pub name: String,
    pub value_label: String,
    pub bar: Option<f32>,
    pub level: Level,
    /// "High"/"Critical", empty when normal
    pub level_label: String,
    pub accessible_label: String,
}

/// Cards reported by one hwmon device ("Motherboard · nct6687").
pub struct ChipGroup<T> {
    pub title: String,
    pub cards: Vec<T>,
}

pub struct CoolingView<'a> {
    /// Title, device counts and the shared RPM graph (no panels without fans)
    pub overview: PageView<'a>,
    /// Spinning fans and fans with unknown speed
    pub fans: &'a [ChipGroup<FanCard>],
    /// Fans that report 0 RPM: unused headers or zero-RPM fan modes
    pub stopped: &'a [ChipGroup<FanCard>],
    pub stopped_count: usize,
    /// "Stopped (N)"
    pub stopped_label: &'a str,
    pub stopped_names: &'a str,
    pub sensors: &'a [ChipGroup<SensorCard>],
    pub detail: Option<PageView<'a>>,
    /// Changes with every applied reading: cached layouts key on it
    pub generation: u64,
}

/// Where a channel's device header comes from.
struct ChipInfo {
    rank: u8,
    hwmon_index: u32,
    title: String,
}

/// Super-I/O and board embedded-controller drivers.
fn is_board(name: &str) -> bool {
    const PREFIXES: [&str; 12] = [
        "nct", "it87", "it86", "f71", "f81", "w83", "asus", "gigabyte", "sch5", "dell", "thinkpad",
        "applesmc",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn chip_info(chip: Option<&HwmonChip>, hwmon_index: u32, readings: &Readings) -> ChipInfo {
    let name = chip
        .and_then(|c| c.name.clone())
        .unwrap_or_else(|| format!("hwmon{hwmon_index}"));
    let gpu = chip.and_then(|c| {
        readings
            .gpus
            .values()
            .find(|g| Some(&g.id) == c.drm_card.as_ref() || Some(&g.id) == c.pci_slot.as_ref())
    });
    let (rank, friendly) = if let Some(gpu) = gpu {
        (
            2,
            Some(gpu.device_name.clone().unwrap_or_else(|| i18n("GPU"))),
        )
    } else if is_board(&name) {
        (0, Some(i18n("Motherboard")))
    } else if matches!(name.as_str(), "k10temp" | "coretemp" | "zenpower") {
        (1, Some(i18n("CPU")))
    } else if let Some(model) = chip.and_then(|c| c.device_model.clone()) {
        (3, Some(model))
    } else {
        (4, None)
    };
    ChipInfo {
        rank,
        hwmon_index,
        title: match friendly {
            Some(friendly) => format!("{friendly} · {name}"),
            None => name,
        },
    }
}

fn fan_hwmon(fan: &Fan) -> u32 {
    fan.chip.as_ref().map_or(fan.hwmon_index, |c| c.hwmon_index)
}

fn sensor_hwmon(sensor: &TemperatureSensor) -> u32 {
    sensor.chip.as_ref().map_or(0, |c| c.hwmon_index)
}

/// Appends a card to the trailing group, or starts the device's group.
fn push_card<T>(
    groups: &mut Vec<ChipGroup<T>>,
    last: &mut Option<(u8, u32)>,
    info: &ChipInfo,
    card: T,
) {
    let id = (info.rank, info.hwmon_index);
    if *last != Some(id) || groups.is_empty() {
        groups.push(ChipGroup {
            title: info.title.clone(),
            cards: vec![],
        });
        *last = Some(id);
    }
    groups.last_mut().unwrap().cards.push(card);
}

struct FanSlot {
    entry: FanEntry,
    /// Assigned the first time the fan spins, then kept while the fan is present
    color: Option<usize>,
}

pub struct CoolingPage {
    fans: Vec<FanSlot>,
    sensors: Vec<SensorEntry>,
    fan_groups: Vec<ChipGroup<FanCard>>,
    stopped_groups: Vec<ChipGroup<FanCard>>,
    stopped_count: usize,
    stopped_label: String,
    stopped_names: String,
    sensor_groups: Vec<ChipGroup<SensorCard>>,
    summary: SummaryGraph,
    caption: String,
    max_duration: String,
    rpm_scale: f32,
    scale_label: String,
    fastest_rpm: Option<u32>,
    generation: u64,
}

impl Default for CoolingPage {
    fn default() -> Self {
        Self::new()
    }
}

impl CoolingPage {
    pub fn new() -> Self {
        let mut summary = SummaryGraph::new(FAN.line, i18n("Cooling"));
        summary.graph.set_grid_visible(true);
        Self {
            fans: vec![],
            sensors: vec![],
            fan_groups: vec![],
            stopped_groups: vec![],
            stopped_count: 0,
            stopped_label: String::new(),
            stopped_names: String::new(),
            sensor_groups: vec![],
            summary,
            caption: String::new(),
            max_duration: String::new(),
            rpm_scale: RPM_SCALE_MIN,
            scale_label: String::new(),
            fastest_rpm: None,
            generation: 0,
        }
    }

    /// Keeps surviving fans' entries, overview histories and colours across reorder/add/remove.
    fn reconcile_fans(&mut self, fans: &[&Fan]) {
        if self.fans.len() == fans.len()
            && self
                .fans
                .iter()
                .zip(fans)
                .all(|(slot, f)| slot.entry.key() == fan::key(f))
        {
            return;
        }
        let groups = std::mem::take(self.summary.graph.groups_mut());
        let mut previous: HashMap<_, _> = self
            .fans
            .drain(..)
            .zip(groups)
            .map(|(slot, group)| (slot.entry.key().to_owned(), (slot, group)))
            .collect();
        self.summary.graph.clear_datasets();
        for f in fans {
            let (slot, group) = previous.remove(&fan::key(f)).unwrap_or_else(|| {
                let mut group = DatasetGroup::new_with_fill(f32::NAN);
                group.dataset_settings.scaling_settings = ScalingSettings::Fixed;
                group.dataset_settings.fill = FillingSettings::None;
                group.dataset_settings.vertical_dropoff_lines = false;
                (
                    FanSlot {
                        entry: FanEntry::new(f),
                        color: None,
                    },
                    group,
                )
            });
            self.fans.push(slot);
            self.summary.graph.add_dataset(group);
        }
        self.summary
            .graph
            .set_all_datasets_max_scale(self.rpm_scale);
    }

    fn reconcile_sensors(&mut self, sensors: &[&TemperatureSensor]) {
        if self.sensors.len() == sensors.len()
            && self
                .sensors
                .iter()
                .zip(sensors)
                .all(|(entry, s)| entry.key() == temperature::key(s))
        {
            return;
        }
        let mut previous: HashMap<_, _> = self
            .sensors
            .drain(..)
            .map(|entry| (entry.key().to_owned(), entry))
            .collect();
        for s in sensors {
            let entry = previous
                .remove(&temperature::key(s))
                .unwrap_or_else(|| SensorEntry::new(s));
            self.sensors.push(entry);
        }
    }

    fn assign_color(&mut self, index: usize) {
        if self.fans[index].color.is_some() {
            return;
        }
        let used: HashSet<_> = self.fans.iter().filter_map(|slot| slot.color).collect();
        let mut color = 0;
        while used.contains(&color) {
            color += 1;
        }
        self.fans[index].color = Some(color);
    }

    fn update_fans(&mut self, fans: &[&Fan], readings: &Readings) {
        self.fan_groups.clear();
        self.stopped_groups.clear();
        let mut stopped_names = vec![];
        let (mut last, mut last_stopped) = (None, None);
        let mut samples = Vec::with_capacity(fans.len());
        for (index, f) in fans.iter().enumerate() {
            let info = chip_info(f.chip.as_ref(), fan_hwmon(f), readings);
            self.fans[index].entry.update(f, &info.title);
            let rpm = fan::rpm(f);
            let spinning = rpm.is_some_and(|rpm| rpm > 0);
            if spinning {
                self.assign_color(index);
            }
            samples.push(vec![match rpm {
                Some(rpm) if spinning => rpm as f32,
                _ => f32::NAN,
            }]);
            let (bar, bar_tooltip) = match (f.pwm_percent, f.max_rpm, rpm) {
                (Some(pwm), _, _) => (Some(pwm.clamp(0., 1.)), i18n("PWM duty cycle")),
                (None, Some(max), Some(rpm)) if max > 0 => (
                    Some((rpm as f32 / max as f32).clamp(0., 1.)),
                    i18n("Speed relative to the reported maximum"),
                ),
                _ => (None, String::new()),
            };
            let name = fan::label(f);
            let rpm_label = fan::rpm_label(rpm);
            let pwm_label = match fan::pwm_label(f.pwm_percent) {
                pwm if pwm.is_empty() => pwm,
                pwm => format!("{} {pwm}", i18n("PWM")),
            };
            let mut accessible_label = format!("{name} — {}: {rpm_label}", info.title);
            if !pwm_label.is_empty() {
                accessible_label.push_str(&format!(", {pwm_label}"));
            }
            let card = FanCard {
                key: self.fans[index].entry.key().to_owned(),
                name,
                rpm,
                rpm_label,
                pwm_label,
                bar,
                bar_tooltip,
                stopped: rpm == Some(0),
                accessible_label,
                colors: self.fans[index]
                    .color
                    .map(|color| COOLING_SERIES[color % COOLING_SERIES.len()]),
            };
            if card.stopped {
                stopped_names.push(card.name.clone());
                push_card(&mut self.stopped_groups, &mut last_stopped, &info, card);
            } else {
                push_card(&mut self.fan_groups, &mut last, &info, card);
            }
        }
        self.stopped_count = stopped_names.len();
        self.stopped_label = i18n_f("Stopped ({})", &[&self.stopped_count.to_string()]);
        self.stopped_names = stopped_names.join(", ");

        self.summary.graph.add_data_point(samples);
        // One shared axis: RPM of different fans stays comparable
        let peak = fans.iter().filter_map(|f| fan::rpm(f)).max().unwrap_or(0) as f32;
        let wanted = ((peak / RPM_SCALE_STEP).ceil() * RPM_SCALE_STEP).max(RPM_SCALE_MIN);
        if wanted > self.rpm_scale {
            self.rpm_scale = wanted;
        }
        self.summary
            .graph
            .set_all_datasets_max_scale(self.rpm_scale);
        self.summary.graph.set_group_colors(
            self.fans
                .iter()
                .map(|slot| COOLING_SERIES[slot.color.unwrap_or(0) % COOLING_SERIES.len()])
                .collect(),
        );
        self.scale_label = i18n_f("{} RPM", &[&self.rpm_scale.to_string()]);
    }

    fn update_sensors(&mut self, sensors: &[&TemperatureSensor], readings: &Readings) {
        self.sensor_groups.clear();
        let mut last = None;
        for (entry, s) in self.sensors.iter_mut().zip(sensors) {
            let info = chip_info(s.chip.as_ref(), sensor_hwmon(s), readings);
            entry.update(s, &info.title);
            let current = temperature::celsius(s.input_millicelsius);
            let max = temperature::limit(s.max_millicelsius);
            let crit = temperature::limit(s.crit_millicelsius);
            let name = temperature::label(s);
            let value_label = temperature::temperature_label(current);
            let level = temperature::level(current, max, crit);
            let card = SensorCard {
                key: entry.key().to_owned(),
                accessible_label: format!("{name} — {}: {value_label}", info.title),
                name,
                value_label,
                bar: temperature::bar(current, max, crit),
                level,
                level_label: match level {
                    Level::Normal => String::new(),
                    Level::High => i18n("High"),
                    Level::Critical => i18n("Critical"),
                },
            };
            push_card(&mut self.sensor_groups, &mut last, &info, card);
        }
    }

    fn counts(&self) -> String {
        let mut parts = vec![];
        if !self.fans.is_empty() {
            let total = self.fans.len();
            let spinning = total - self.stopped_count;
            parts.push(ni18n_f(
                "{}/{} fan",
                "{}/{} fans",
                total as u32,
                &[&spinning.to_string(), &total.to_string()],
            ));
        }
        if !self.sensors.is_empty() {
            parts.push(ni18n_f(
                "{} sensor",
                "{} sensors",
                self.sensors.len() as u32,
                &[&self.sensors.len().to_string()],
            ));
        }
        parts.join(" · ")
    }
}

/// Deduplicated by key, ordered by device group then channel.
fn sorted<T>(
    items: &[T],
    key: impl Fn(&T) -> String,
    rank: impl Fn(&T) -> (u8, u32, u32),
) -> Vec<&T> {
    let mut seen = HashSet::new();
    let mut out: Vec<_> = items.iter().filter(|item| seen.insert(key(item))).collect();
    out.sort_by_cached_key(|item| rank(item));
    out
}

impl DevicePage for CoolingPage {
    fn name(&self) -> &str {
        "cooling"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Cooling
    }

    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }

    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let fans = sorted(&readings.fans, fan::key, |f| {
            let info = chip_info(f.chip.as_ref(), fan_hwmon(f), readings);
            (info.rank, info.hwmon_index, f.fan_index)
        });
        let sensors = sorted(&readings.temperatures, temperature::key, |s| {
            let info = chip_info(s.chip.as_ref(), sensor_hwmon(s), readings);
            (info.rank, info.hwmon_index, s.temp_index)
        });
        if fans.is_empty() && sensors.is_empty() {
            return false;
        }
        self.max_duration = env.max_duration.clone();
        self.generation += 1;
        self.reconcile_fans(&fans);
        self.reconcile_sensors(&sensors);
        self.update_fans(&fans, readings);
        self.update_sensors(&sensors, readings);

        self.summary.info1 = self.counts();
        let hottest = readings
            .temperatures
            .iter()
            .filter_map(|s| Some((temperature::celsius(s.input_millicelsius)?, s)))
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let fastest = readings.fans.iter().filter_map(fan::rpm).max();
        self.fastest_rpm = fastest;
        let mut info2 = vec![];
        if let Some((t, _)) = hottest {
            info2.push(i18n_f("Max {}", &[&format!("{t:.0} °C")]));
        }
        if let Some(rpm) = fastest.filter(|rpm| *rpm > 0) {
            info2.push(fan::rpm_label(Some(rpm)));
        }
        self.summary.info2 = info2.join(" · ");
        let mut tooltip = vec![];
        if let Some((t, sensor)) = hottest {
            let info = chip_info(sensor.chip.as_ref(), sensor_hwmon(sensor), readings);
            tooltip.push(format!(
                "{}: {} — {}",
                i18n("Hottest sensor"),
                temperature::temperature_label(Some(t)),
                format_args!("{} ({})", temperature::label(sensor), info.title)
            ));
        }
        if self.stopped_count > 0 {
            tooltip.push(format!(
                "{}: {}",
                i18n_f("Stopped ({})", &[&self.stopped_count.to_string()]),
                self.stopped_names
            ));
        }
        self.summary.tooltip = (!tooltip.is_empty()).then(|| tooltip.join("\n"));
        self.caption = over(n_("Fan speed over "), &env.max_duration);
        true
    }

    fn fastest_fan_rpm(&self) -> Option<u32> {
        self.fastest_rpm
    }

    fn set_grid_offset(&mut self, offset: u32) -> bool {
        // PerformancePage advances summary.graph separately, once per global clock tick.
        let fans = self.fans.iter_mut().fold(false, |repaint, slot| {
            slot.entry.set_grid_offset(offset) | repaint
        });
        self.sensors.iter_mut().fold(fans, |repaint, entry| {
            entry.set_grid_offset(offset) | repaint
        })
    }

    fn data_summary(&self) -> String {
        self.fans
            .iter()
            .map(|slot| slot.entry.data_summary())
            .chain(self.sensors.iter().map(SensorEntry::data_summary))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn win10_view(&mut self, _config: &Config) -> PageView<'_> {
        overview(
            &mut self.summary,
            &self.caption,
            &self.scale_label,
            !self.fans.is_empty(),
        )
    }

    fn win10_cooling_view(
        &mut self,
        _config: &Config,
        selected: Option<&str>,
    ) -> Option<CoolingView<'_>> {
        let max_duration = &self.max_duration;
        let has_fans = !self.fans.is_empty();
        let detail = match selected {
            Some(key) if key.starts_with("fan:") => self
                .fans
                .iter_mut()
                .find(|slot| slot.entry.key() == key)
                .map(|slot| slot.entry.detail_view(max_duration)),
            Some(key) => self
                .sensors
                .iter_mut()
                .find(|entry| entry.key() == key)
                .map(|entry| entry.detail_view(max_duration)),
            None => None,
        };
        Some(CoolingView {
            overview: overview(
                &mut self.summary,
                &self.caption,
                &self.scale_label,
                has_fans,
            ),
            fans: &self.fan_groups,
            stopped: &self.stopped_groups,
            stopped_count: self.stopped_count,
            stopped_label: &self.stopped_label,
            stopped_names: &self.stopped_names,
            sensors: &self.sensor_groups,
            detail,
            generation: self.generation,
        })
    }
}

fn overview<'a>(
    summary: &'a mut SummaryGraph,
    caption: &str,
    scale: &str,
    has_fans: bool,
) -> PageView<'a> {
    let model = summary.info1.clone();
    PageView {
        model_tooltip: None,
        title: summary.heading.clone(),
        model,
        panels: if has_fans {
            vec![GraphPanel::single(
                caption.into(),
                scale.into(),
                &mut summary.graph,
            )]
        } else {
            vec![]
        },
        stats: vec![],
        stat_legends: vec![],
        info: vec![],
        composition: None,
        swap_compression: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fan(hwmon: u32, index: u32, rpm: u32, label: &str) -> Fan {
        Fan {
            fan_label: Some(label.into()),
            fan_index: index,
            hwmon_index: hwmon,
            rpm,
            pwm_percent: Some(0.5),
            chip: Some(HwmonChip {
                hwmon_index: hwmon,
                name: Some(if hwmon == 1 { "amdgpu" } else { "nct6687" }.into()),
                drm_card: (hwmon == 1).then(|| "card1".into()),
                ..Default::default()
            }),
            rpm_valid: Some(true),
            ..Default::default()
        }
    }

    fn update(page: &mut CoolingPage, readings: &Readings) -> bool {
        let config = Config::default();
        page.update(readings, &UpdateEnv::new(&config))
    }

    #[test]
    fn groups_stopped_unknown_and_shared_axis() {
        let mut readings = Readings::new();
        readings.gpus.insert(
            "card1".into(),
            mc_magpie::Gpu {
                id: "card1".into(),
                device_name: Some("Radeon RX 6700 XT".into()),
                ..Default::default()
            },
        );
        readings.fans = vec![
            fan(4, 1, 1000, "CPU Fan"),
            fan(4, 2, 0, "Pump Fan"),
            fan(1, 1, 500, "GPU"),
            Fan {
                rpm_valid: Some(false),
                ..fan(4, 3, 0, "System Fan #1")
            },
        ];
        readings.temperatures = vec![TemperatureSensor {
            temp_index: 7,
            chip: readings.fans[0].chip.clone(),
            label: Some("M2_1".into()),
            input_millicelsius: Some(0),
            ..Default::default()
        }];
        let mut page = CoolingPage::new();
        assert!(update(&mut page, &readings));
        let view = page.win10_cooling_view(&Config::default(), None).unwrap();
        let titles: Vec<_> = view.fans.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                format!("{} · nct6687", i18n("Motherboard")),
                "Radeon RX 6700 XT · amdgpu".into()
            ]
        );
        // Unknown speed is not stopped
        let board: Vec<_> = view.fans[0].cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(board, ["CPU Fan", "System Fan #1"]);
        assert_eq!(view.fans[0].cards[1].rpm_label, "—");
        assert!(view.fans[0].cards[1].colors.is_none());
        assert_eq!(view.stopped_count, 1);
        assert_eq!(view.stopped_names, "Pump Fan");
        assert_eq!(
            view.sensors[0].cards[0].value_label,
            i18n_f("{} °C", &["0.0"])
        );
        drop(view);

        // 500 and 1000 RPM share one 1000 RPM axis
        let graph = &page.summary.graph;
        assert!((0..graph.groups().len()).all(|i| graph.dataset_max_scale(i) == 1000.));
        readings.fans[0].rpm = 1720;
        update(&mut page, &readings);
        let graph = &page.summary.graph;
        assert!((0..graph.groups().len()).all(|i| graph.dataset_max_scale(i) == 2000.));
        readings.fans[0].rpm = 800;
        update(&mut page, &readings);
        assert_eq!(page.summary.graph.dataset_max_scale(0), 2000., "sticky");
    }

    #[test]
    fn colours_and_histories_survive_reorder_and_removal() {
        let mut readings = Readings::new();
        readings.fans = vec![fan(4, 1, 900, "A"), fan(4, 2, 0, "B"), fan(4, 3, 700, "C")];
        let mut page = CoolingPage::new();
        update(&mut page, &readings);
        let colors = |page: &mut CoolingPage| {
            page.fans
                .iter()
                .map(|slot| (slot.entry.key().to_owned(), slot.color))
                .collect::<Vec<_>>()
        };
        let before = colors(&mut page);
        assert_eq!(before[0].1, Some(0));
        assert_eq!(before[1].1, None, "a stopped fan takes no colour");
        assert_eq!(before[2].1, Some(1));

        readings.fans.remove(0);
        readings.fans[0].rpm = 600;
        update(&mut page, &readings);
        let after = colors(&mut page);
        assert_eq!(after[1], before[2], "survivor keeps its colour");
        assert_eq!(after[0].1, Some(0), "freed colour is reused");

        let copy = page.data_summary();
        assert!(copy.contains("B") && copy.contains("C"));
        readings.fans.clear();
        assert!(!update(&mut page, &readings), "nothing left to show");
    }

    #[test]
    fn views_never_add_samples() {
        let mut readings = Readings::new();
        readings.fans = vec![fan(4, 1, 900, "A")];
        let mut page = CoolingPage::new();
        update(&mut page, &readings);
        let generation = page.summary.graph.generation();
        let key = page.fans[0].entry.key().to_owned();
        for _ in 0..3 {
            let view = page
                .win10_cooling_view(&Config::default(), Some(&key))
                .unwrap();
            assert_eq!(view.detail.unwrap().model, "A");
        }
        assert_eq!(page.summary.graph.generation(), generation);
    }
}
