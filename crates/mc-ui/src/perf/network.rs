/* crates/mc-ui/src/perf/network.rs
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
use egui::Ui;
use mc_core::config::Config;
use mc_core::format::{DataType, UnitSettings, to_human_readable_nice};
use mc_core::i18n::n_;
use mc_core::i18n::{i18n, i18n_f, strip_mnemonic};
use mc_core::sidebar::{DeviceType, NetworkGroup};
use mc_graph::{
    AnimationFrame, DatasetGroup, FillingSettings, Graph, GraphSettings, RoundingSettings,
    ScalingSettings,
};
use mc_magpie::magpie_types::network::{ConnectionKind, ConnectionState};
use mc_magpie::{Connection, Readings};

use super::{DevicePage, SummaryGraph, UpdateEnv};
use crate::cx::{Command, Cx};
const COLOR: egui::Color32 = crate::win10::palette::NETWORK.line;

pub fn page_name(if_id: &str) -> String {
    format!("net-{if_id}")
}

fn state_label(state: ConnectionState) -> String {
    // translators: see NMDeviceState https://www.networkmanager.dev/docs/api/latest/nm-dbus-types.html#NMDeviceState
    i18n(match state {
        ConnectionState::Unknown => n_("Unknown"),
        ConnectionState::Failed => n_("Failed"),
        ConnectionState::Disconnected => n_("Disconnected"),
        ConnectionState::Disconnecting => n_("Disconnecting"),
        ConnectionState::Connected => n_("Connected"),
        ConnectionState::Connecting => n_("Connecting"),
        ConnectionState::ConfiguringIp => n_("Configuring"),
        ConnectionState::NeedsAuth => n_("Waiting for auth"),
        ConnectionState::Unavailable => n_("Unavailable"),
        ConnectionState::Loading => n_("Loading"),
    })
}

#[derive(Default)]
struct Labels {
    speed_send: String,
    speed_recv: String,
    total_recv: String,
    total_sent: String,
    status: String,
    ssid: String,
    signal: Option<u32>,
    max_bitrate: Option<String>,
    frequency: String,
    hw_address: String,
    ipv4: String,
    ipv6: String,
    max_y: String,
}

pub struct NetworkPage {
    name: String,
    if_id: String,
    kind: ConnectionKind,
    summary: SummaryGraph,
    graph: Graph,
    device_name: String,
    max_speed: Option<u64>,
    labels: Labels,
    units: UnitSettings,
    scaling_key: (bool, bool, bool),
    max_duration: String,
}

fn traffic_graph(color: egui::Color32) -> Graph {
    let mut g = Graph::new();
    g.set_base_color(color);
    let mut tx = DatasetGroup::new();
    tx.dataset_settings.fill = FillingSettings::None;
    tx.dataset_settings.dashed = true;
    tx.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
    tx.dataset_settings.rounding_settings = RoundingSettings::Pow2;
    let mut rx = DatasetGroup::new();
    rx.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
    rx.dataset_settings.rounding_settings = RoundingSettings::Pow2;
    g.add_dataset(tx);
    g.add_dataset(rx);
    g.connect_datasets(0, 1);
    g.connect_datasets(1, 0);
    g
}

impl NetworkPage {
    pub fn new(connection: &Connection, config: &Config) -> Self {
        let kind = connection.kind();
        let mut summary =
            SummaryGraph::new(COLOR, format!("{} ({})", kind.as_str_name(), connection.id));
        summary.graph = traffic_graph(COLOR);
        summary.graph.set_grid_visible(false);
        summary.graph.set_data_points(60);

        let mut page = Self {
            name: page_name(&connection.id),
            if_id: connection.id.clone(),
            kind,
            summary,
            graph: traffic_graph(COLOR),
            device_name: connection
                .device_name
                .clone()
                .unwrap_or_else(|| i18n("Unknown")),
            max_speed: connection.max_speed_bytes_ps,
            labels: Labels::default(),
            units: config.units(),
            scaling_key: (false, false, false),
            max_duration: super::max_duration(config),
        };
        page.update_graph_scaling(config);
        page
    }

    fn update_graph_scaling(&mut self, config: &Config) {
        self.scaling_key = (
            config.performance_page_network_dynamic_scaling,
            config.performance_page_network_use_base2,
            config.performance_page_network_use_bytes,
        );
        for graph in [&mut self.graph, &mut self.summary.graph] {
            match self.max_speed {
                Some(max) if !config.performance_page_network_dynamic_scaling => {
                    graph.set_all_datasets_scaling(ScalingSettings::Fixed);
                    graph.set_all_datasets_max_scale(max as f32);
                }
                _ => {
                    graph.set_all_datasets_scaling(ScalingSettings::ScaleUp);
                    if config.performance_page_network_use_base2 {
                        graph.set_all_datasets_rounding(RoundingSettings::Pow2);
                    } else {
                        graph.set_all_datasets_rounding(RoundingSettings::Pow2Base10);
                        graph.set_all_datasets_watermarking_multiplier(
                            if config.performance_page_network_use_bytes {
                                1.
                            } else {
                                8.
                            },
                        );
                    }
                    graph.reset_auto_scaling();
                }
            }
        }
    }

    fn is_wireless(&self) -> bool {
        self.kind == ConnectionKind::Wireless
    }
}

impl DevicePage for NetworkPage {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Network(NetworkGroup::from_connection_kind(self.kind))
    }

    fn summary(&self) -> &SummaryGraph {
        &self.summary
    }

    fn summary_mut(&mut self) -> &mut SummaryGraph {
        &mut self.summary
    }

    fn apply_graph_settings(&mut self, settings: &GraphSettings) {
        self.graph.apply_settings(settings);
        self.summary.graph.set_do_animation(settings.sliding_graphs);
    }

    fn update(&mut self, readings: &Readings, env: &UpdateEnv) -> bool {
        let Some(c) = readings
            .network_connections
            .values()
            .find(|c| c.id == self.if_id)
        else {
            return false;
        };
        self.units = env.units;
        self.max_duration = env.max_duration.clone();
        self.max_speed = c.max_speed_bytes_ps;
        let key = (
            env.config.performance_page_network_dynamic_scaling,
            env.config.performance_page_network_use_base2,
            env.config.performance_page_network_use_bytes,
        );
        if key != self.scaling_key {
            self.update_graph_scaling(env.config);
        }
        let units = self.units;
        let h = |v: f32, t: DataType| to_human_readable_nice(v, &t, &units);

        self.graph
            .add_data_point(vec![vec![c.tx_rate_bytes_ps], vec![c.rx_rate_bytes_ps]]);
        let unknown = i18n("Unknown");
        let na = i18n("N/A");
        let wireless = c.wireless_connection.as_ref();
        let l = &mut self.labels;
        l.ssid = wireless
            .and_then(|w| w.ssid.clone())
            .unwrap_or_else(|| unknown.clone());
        l.signal = wireless
            .and_then(|w| w.signal_strength_percent)
            .map(|p| p.min(100));
        l.frequency = wireless
            .and_then(|w| w.frequency_mhz)
            .map(|f| h(f as f32 * 1_000_000., DataType::Hertz))
            .unwrap_or_else(|| unknown.clone());
        l.max_bitrate = c
            .max_speed_bytes_ps
            .map(|m| h(m as f32, DataType::NetworkBytesPerSecond));
        l.max_y = h(
            self.graph.dataset_max_scale(0),
            DataType::NetworkBytesPerSecond,
        );
        l.speed_send = h(c.tx_rate_bytes_ps, DataType::NetworkBytesPerSecond);
        l.speed_recv = h(c.rx_rate_bytes_ps, DataType::NetworkBytesPerSecond);
        l.total_sent = h(c.tx_total_bytes as f32, DataType::NetworkBytes);
        l.total_recv = h(c.rx_total_bytes as f32, DataType::NetworkBytes);
        l.hw_address = c.hw_address.clone();
        l.ipv4 = c.ipv4_address.clone().unwrap_or_else(|| na.clone());
        l.ipv6 = c.ipv6_address.clone().unwrap_or(na);
        l.status = state_label(c.state());

        self.summary
            .graph
            .add_data_point(vec![vec![c.tx_rate_bytes_ps], vec![c.rx_rate_bytes_ps]]);
        self.summary.opacity = if c.state() == ConnectionState::Unavailable {
            0.6
        } else {
            1.
        };
        if c.state() != ConnectionState::Connected {
            self.summary.info1 = i18n_f("{}", &[c.state().as_str_name()]);
            self.summary.info2.clear();
        } else {
            self.summary.info1 = i18n_f("{}: {}", &["S", &l.speed_send]);
            self.summary.info2 = i18n_f("{}: {}", &["R", &l.speed_recv]);
        }
        true
    }

    fn animate(&mut self, frame: AnimationFrame) -> bool {
        self.graph.update_animation(frame)
    }

    fn win10_view(&mut self, _config: &Config) -> PageView<'_> {
        let l = &self.labels;
        let stats = vec![
            vec![
                (i18n("Send"), l.speed_send.clone()),
                (i18n("Receive"), l.speed_recv.clone()),
            ],
            vec![
                (i18n("Total Sent"), l.total_sent.clone()),
                (i18n("Total Received"), l.total_recv.clone()),
            ],
        ];
        let wireless = self.is_wireless();
        let mut info = vec![
            (i18n("Interface name:"), self.if_id.clone()),
            (i18n("Connection type:"), self.kind.as_str_name().to_owned()),
            (i18n("Connection status:"), l.status.clone()),
        ];
        if wireless {
            info.push((i18n("SSID:"), l.ssid.clone()));
            info.push((
                i18n("Signal strength:"),
                l.signal
                    .map(|p| format!("{p}%"))
                    .unwrap_or_else(|| i18n("N/A")),
            ));
        }
        if let Some(max) = &l.max_bitrate
            && (wireless || self.max_speed.is_some())
        {
            info.push((i18n("Maximum Bitrate:"), max.clone()));
        }
        if wireless {
            info.push((i18n("Frequency:"), l.frequency.clone()));
        }
        info.push((i18n("Hardware address:"), l.hw_address.clone()));
        info.push((i18n("IPv4 address:"), l.ipv4.clone()));
        info.push((i18n("IPv6 address:"), l.ipv6.clone()));
        let max_y = l.max_y.clone();
        PageView {
            title: self.kind.as_str_name().to_owned(),
            model: self.device_name.clone(),
            panels: vec![GraphPanel::single(
                over("Throughput over ", &self.max_duration),
                max_y,
                &mut self.graph,
            )],
            stats,
            info,
        }
    }

    fn win10_menu(&mut self, ui: &mut Ui, cx: &mut Cx) {
        if crate::win10::menu::item(ui, &strip_mnemonic(&i18n("Network Se_ttings")), "", true) {
            cx.send(Command::OpenNetworkSettings(self.is_wireless()));
        }
    }

    fn data_summary(&self) -> String {
        let l = &self.labels;
        let wireless = if self.is_wireless() {
            format!(
                "\n\n    SSID:             {}\n    Signal strength:  {}\n    Max bitrate:      {}\n    Frequency:        {}",
                l.ssid,
                l.signal
                    .map(|s| format!("{s}%"))
                    .unwrap_or_else(|| i18n("Unknown")),
                l.max_bitrate.clone().unwrap_or_else(|| i18n("Unknown")),
                l.frequency,
            )
        } else {
            String::new()
        };
        format!(
            r#"{}

    {}

    Interface name:   {}
    Connection type:  {}{}
    Hardware address: {}
    IPv4 address:     {}
    IPv6 address:     {}

    Send:            {}
    Receive:         {}"#,
            self.kind.as_str_name(),
            self.device_name,
            self.if_id,
            self.kind.as_str_name(),
            wireless,
            l.hw_address,
            l.ipv4,
            l.ipv6,
            l.speed_send,
            l.speed_recv,
        )
    }
}
