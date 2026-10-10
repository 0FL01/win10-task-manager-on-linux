/* performance_page/network.rs
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

use std::cell::{Cell, OnceCell, RefCell};

use adw::gio::Settings;
use adw::subclass::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::{gio, glib, prelude::*};

use magpie_types::network::{Connection, ConnectionKind, ConnectionState};

use crate::i18n::*;
use crate::performance_page::widgets::{
    AnimationFrame, DatasetGroup, FillingSettings, GraphWidget, GraphWidgetSettingsExt,
    RoundingSettings, ScalingSettings,
};
use crate::{application::INTERVAL_STEP, to_short_human_readable_time};
use crate::{settings, DataType};

use super::PageExt;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePageNetwork)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/network.ui")]
    pub struct PerformancePageNetwork {
        #[template_child]
        pub title_connection_type: TemplateChild<gtk::Label>,
        #[template_child]
        pub device_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub max_y: TemplateChild<gtk::Label>,
        #[template_child]
        pub usage_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub graph_max_duration: TemplateChild<gtk::Label>,
        #[template_child]
        pub context_menu: TemplateChild<gtk::Popover>,

        #[property(get, set)]
        base_color: Cell<gtk::gdk::RGBA>,
        #[property(get, set)]
        summary_mode: Cell<bool>,

        #[property(get = Self::interface_name, set = Self::set_interface_name, type = String)]
        pub interface_name: RefCell<String>,
        pub connection_type: Cell<ConnectionKind>,

        #[property(get = Self::infobar_content, type = Option < gtk::Widget >)]
        pub infobar_content: OnceCell<gtk::Box>,

        pub legend_send: OnceCell<gtk::Picture>,
        pub speed_send: OnceCell<gtk::Label>,
        pub total_sent: OnceCell<gtk::Label>,
        pub legend_recv: OnceCell<gtk::Picture>,
        pub speed_recv: OnceCell<gtk::Label>,
        pub total_recv: OnceCell<gtk::Label>,
        pub interface_name_label: OnceCell<gtk::Label>,
        pub connection_type_label: OnceCell<gtk::Label>,
        pub connection_status_label: OnceCell<gtk::Label>,
        pub ssid: OnceCell<gtk::Label>,
        pub signal_strength: OnceCell<gtk::Image>,
        pub max_bitrate: OnceCell<gtk::Label>,
        pub frequency: OnceCell<gtk::Label>,
        pub hw_address: OnceCell<gtk::Label>,
        pub ipv4_address: OnceCell<gtk::Label>,
        pub ipv6_address: OnceCell<gtk::Label>,

        signal_strength_percent: Cell<Option<u8>>,
        pub use_bytes: Cell<bool>,
        // in bps
        pub max_speed: Cell<Option<u64>>,
    }

    impl Default for PerformancePageNetwork {
        fn default() -> Self {
            Self {
                title_connection_type: Default::default(),
                device_name: Default::default(),
                max_y: Default::default(),
                usage_graph: Default::default(),
                graph_max_duration: Default::default(),
                context_menu: Default::default(),

                base_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                summary_mode: Cell::new(false),

                interface_name: RefCell::new(String::new()),
                connection_type: Cell::new(ConnectionKind::Other),

                infobar_content: Default::default(),

                legend_send: Default::default(),
                speed_send: Default::default(),
                total_sent: Default::default(),
                legend_recv: Default::default(),
                speed_recv: Default::default(),
                total_recv: Default::default(),
                interface_name_label: Default::default(),
                connection_type_label: Default::default(),
                connection_status_label: Default::default(),
                ssid: Default::default(),
                signal_strength: Default::default(),
                max_bitrate: Default::default(),
                frequency: Default::default(),
                hw_address: Default::default(),
                ipv4_address: Default::default(),
                ipv6_address: Default::default(),

                signal_strength_percent: Cell::new(None),
                use_bytes: Cell::new(false),
                max_speed: Cell::new(None),
            }
        }
    }

    impl PerformancePageNetwork {
        fn interface_name(&self) -> String {
            self.interface_name.borrow().clone()
        }

        fn set_interface_name(&self, interface_name: String) {
            if interface_name == *self.interface_name.borrow() {
                return;
            }

            self.interface_name.replace(interface_name);
        }

        fn infobar_content(&self) -> Option<gtk::Widget> {
            self.infobar_content.get().map(|ic| ic.clone().into())
        }
    }

    impl PerformancePageNetwork {
        fn configure_actions(this: &super::PerformancePageNetwork) {
            let actions = gio::SimpleActionGroup::new();
            this.insert_action_group("graph", Some(&actions));

            let action = gio::SimpleAction::new("network-settings", None);
            action.connect_activate({
                let this = this.downgrade();
                move |_, _| {
                    if let Some(this) = this.upgrade() {
                        PerformancePageNetwork::gnome_settings_activate_action(
                            if this.imp().connection_type.get() == ConnectionKind::Wireless {
                                "('launch-panel', [<('wifi', [<''>])>], {})"
                            } else {
                                "('launch-panel', [<('network', [<''>])>], {})"
                            },
                        )
                    }
                }
            });
            actions.add_action(&action);

            let action = gio::SimpleAction::new("copy", None);
            action.connect_activate({
                let this = this.downgrade();
                move |_, _| {
                    if let Some(this) = this.upgrade() {
                        let clipboard = this.clipboard();
                        clipboard.set_text(this.imp().data_summary().as_str());
                    }
                }
            });
            actions.add_action(&action);
        }

        fn configure_context_menu(this: &super::PerformancePageNetwork) {
            let right_click_controller = gtk::GestureClick::new();
            right_click_controller.set_button(3); // Secondary click (AKA right click)
            right_click_controller.connect_released({
                let this = this.downgrade();
                move |_click, _n_press, x, y| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        this.context_menu
                            .set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                                x.round() as i32,
                                y.round() as i32,
                                1,
                                1,
                            )));
                        this.context_menu.popup();
                    }
                }
            });
            this.add_controller(right_click_controller);
        }

        fn gnome_settings_activate_action(variant_str: &str) {
            use gtk::glib::{self, g_critical};

            let proxy = match gio::DBusProxy::for_bus_sync(
                gio::BusType::Session,
                gio::DBusProxyFlags::NONE,
                None,
                "org.gnome.Settings",
                "/org/gnome/Settings",
                "org.freedesktop.Application",
                gio::Cancellable::NONE,
            ) {
                Ok(proxy) => proxy,
                Err(e) => {
                    g_critical!("MissionCenter", "Failed to open settings panel, failed connect to 'org.gnome.Settings': {e}");
                    return;
                }
            };

            let method_params = match glib::Variant::parse(
                Some(glib::VariantTy::new("(sava{sv})").unwrap()),
                variant_str,
            ) {
                Ok(params) => params,
                Err(e) => {
                    g_critical!(
                        "MissionCenter",
                        "Failed to open settings panel, failed set-up D-Bus call parameters: {e}"
                    );
                    return;
                }
            };

            if let Err(e) = proxy.call_sync(
                "org.freedesktop.Application.ActivateAction",
                Some(&method_params),
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
            ) {
                g_critical!("MissionCenter", "Failed to open settings panel, failed to call 'org.freedesktop.Application.ActivateAction': {e}");
            }
        }
    }

    impl PerformancePageNetwork {
        pub fn set_static_information(
            this: &super::PerformancePageNetwork,
            connection: &Connection,
        ) -> bool {
            let this = this.imp();

            let interface_name = this.interface_name.take();
            let connection_type = this.connection_type.get();

            if let Some(adapter_name) = &connection.device_name {
                this.device_name.set_text(adapter_name);
            } else {
                this.device_name.set_text(&i18n("Unknown"));
            }

            if let Some(interface_name_label) = this.interface_name_label.get() {
                interface_name_label.set_text(&interface_name);
            }

            let conn_type = match connection_type {
                ConnectionKind::Wireless => {
                    if let Some(ssid) = this.ssid.get() {
                        ssid.set_visible(true);
                    }
                    if let Some(signal_strength) = this.signal_strength.get() {
                        signal_strength.set_visible(true);
                    }
                    if let Some(frequency) = this.frequency.get() {
                        frequency.set_visible(true);
                    }

                    connection_type.as_str_name()
                }
                _ => connection_type.as_str_name(),
            };

            if let Some(max_bitrate) = this.max_bitrate.get() {
                if connection_type == ConnectionKind::Wireless
                    || connection.max_speed_bytes_ps.is_some()
                {
                    max_bitrate.set_visible(true);
                }
            }

            if let Some(connection_type_label) = this.connection_type_label.get() {
                connection_type_label.set_text(conn_type);
            }
            this.title_connection_type.set_text(conn_type);

            if let Some(legend_send) = this.legend_send.get() {
                legend_send
                    .set_resource(Some("/io/missioncenter/MissionCenter/line-dashed-net.svg"));
            }

            if let Some(legend_recv) = this.legend_recv.get() {
                legend_recv
                    .set_resource(Some("/io/missioncenter/MissionCenter/line-solid-net.svg"));
            }

            this.interface_name.replace(interface_name);

            this.usage_graph.set_filled(0, FillingSettings::None);
            this.usage_graph.set_dashed(0, true);

            this.max_speed.set(connection.max_speed_bytes_ps);

            true
        }

        pub fn update_readings(
            this: &super::PerformancePageNetwork,
            connection: &Connection,
        ) -> bool {
            let this = this.imp();

            this.usage_graph.add_data_point(vec![
                vec![connection.tx_rate_bytes_ps],
                vec![connection.rx_rate_bytes_ps],
            ]);

            let send_speed = connection.tx_rate_bytes_ps;
            let rec_speed = connection.rx_rate_bytes_ps;

            if let Some(wireless_info) = &connection.wireless_connection {
                if let Some(ssid) = this.ssid.get() {
                    ssid.set_text(
                        &wireless_info
                            .ssid
                            .as_ref()
                            .map_or(i18n("Unknown"), |ssid| ssid.clone()),
                    );
                }
                this.signal_strength_percent.set(
                    wireless_info
                        .signal_strength_percent
                        .map(|p| p.min(100) as u8)
                        .clone(),
                );
                if let Some(signal_strength) = this.signal_strength.get() {
                    signal_strength.set_icon_name(Some(
                        if let Some(percentage) = wireless_info.signal_strength_percent.as_ref() {
                            if *percentage <= 25_u32 {
                                "nm-signal-25-symbolic"
                            } else if *percentage <= 50_u32 {
                                "nm-signal-50-symbolic"
                            } else if *percentage <= 75_u32 {
                                "nm-signal-75-symbolic"
                            } else {
                                "nm-signal-100-symbolic"
                            }
                        } else {
                            "nm-signal-00-symbolic"
                        },
                    ));
                }
                if let Some(frequency) = this.frequency.get() {
                    frequency.set_text(&wireless_info.frequency_mhz.as_ref().map_or(
                        i18n("Unknown"),
                        |freq| {
                            crate::to_human_readable_nice(
                                *freq as f32 * 1_000_000.,
                                &DataType::Hertz,
                            )
                        },
                    ));
                }
            }

            if let Some(max_bitrate) = this.max_bitrate.get() {
                if let Some(max_speed) = connection.max_speed_bytes_ps {
                    let max_label = crate::to_human_readable_nice(
                        max_speed as f32,
                        &DataType::NetworkBytesPerSecond,
                    );

                    max_bitrate.set_text(max_label.as_str());

                    max_bitrate.set_visible(true);
                } else {
                    max_bitrate.set_visible(false);
                }
            }

            let max_y = crate::to_human_readable_nice(
                this.usage_graph.get_dataset_max_scale(0),
                &DataType::NetworkBytesPerSecond,
            );
            this.max_y.set_text(&max_y);

            let speed_send_info =
                crate::to_human_readable_nice(send_speed, &DataType::NetworkBytesPerSecond);
            if let Some(speed_send) = this.speed_send.get() {
                speed_send.set_text(&speed_send_info);
            }

            let speed_recv_info =
                crate::to_human_readable_nice(rec_speed, &DataType::NetworkBytesPerSecond);
            if let Some(speed_recv) = this.speed_recv.get() {
                speed_recv.set_text(&speed_recv_info);
            }

            let sent = crate::to_human_readable_nice(
                connection.tx_total_bytes as f32,
                &DataType::NetworkBytes,
            );
            if let Some(total_sent) = this.total_sent.get() {
                total_sent.set_text(&sent);
            }
            let received = crate::to_human_readable_nice(
                connection.rx_total_bytes as f32,
                &DataType::NetworkBytes,
            );
            if let Some(total_recv) = this.total_recv.get() {
                total_recv.set_text(&received);
            }

            if let Some(hw_address) = this.hw_address.get() {
                hw_address.set_text(&connection.hw_address)
            }

            if let Some(ipv4_address) = this.ipv4_address.get() {
                ipv4_address.set_text(connection.ipv4_address.as_ref().unwrap_or(&i18n("N/A")));
            }

            if let Some(ipv6_address) = this.ipv6_address.get() {
                if let Some(address) = connection.ipv6_address.as_ref().map(|a| a.as_str()) {
                    ipv6_address.set_text(address);
                    ipv6_address.set_tooltip_text(Some(address));
                } else {
                    ipv6_address.set_text(&i18n("N/A"));
                    ipv6_address.set_tooltip_text(None);
                }
            }

            if let Some(connection_status_label) = this.connection_status_label.get() {
                // translators: see NMDeviceState https://www.networkmanager.dev/docs/api/latest/nm-dbus-types.html#NMDeviceState
                let conn_status =
                    match ConnectionState::try_from(connection.state).unwrap_or_default() {
                        ConnectionState::Unknown => i18n("Unknown"),
                        ConnectionState::Failed => i18n("Failed"),
                        ConnectionState::Disconnected => i18n("Disconnected"),
                        ConnectionState::Disconnecting => i18n("Disconnecting"),
                        ConnectionState::Connected => i18n("Connected"),
                        ConnectionState::Connecting => i18n("Connecting"),
                        ConnectionState::ConfiguringIp => i18n("Configuring"),
                        ConnectionState::NeedsAuth => i18n("Waiting for auth"),
                        ConnectionState::Unavailable => i18n("Unavailable"),
                        ConnectionState::Loading => i18n("Loading"),
                    };

                connection_status_label.set_text(&conn_status);
            }

            true
        }

        pub fn update_animations(
            this: &super::PerformancePageNetwork,
            ticks: AnimationFrame,
        ) -> bool {
            let this = this.imp();

            this.usage_graph.update_animation(ticks);

            true
        }

        fn data_summary(&self) -> String {
            let unknown = i18n("Unknown");
            let unknown = unknown.as_str();

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
                self.title_connection_type.label(),
                self.device_name.label(),
                self.interface_name_label
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.connection_type_label
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                if self.connection_type.get() == ConnectionKind::Wireless {
                    format!(
                        r#"
    SSID:             {}
    Signal strength:  {}
    Max bitrate:      {}
    Frequency:        {}"#,
                        self.ssid.get().map(|l| l.label()).unwrap_or(unknown.into()),
                        self.signal_strength_percent
                            .get()
                            .map_or(i18n("Unknown"), |percent| format!("{}%", percent)),
                        self.max_bitrate
                            .get()
                            .map(|l| l.label())
                            .unwrap_or(unknown.into()),
                        self.frequency
                            .get()
                            .map(|l| l.label())
                            .unwrap_or(unknown.into()),
                    )
                } else {
                    "".to_owned()
                },
                self.hw_address
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.ipv4_address
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.ipv6_address
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.speed_send
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
                self.speed_recv
                    .get()
                    .map(|l| l.label())
                    .unwrap_or(unknown.into()),
            )
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePageNetwork {
        const NAME: &'static str = "PerformancePageNetwork";
        type Type = super::PerformancePageNetwork;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePageNetwork {
        fn properties() -> &'static [ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }

        fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
            self.derived_property(id, pspec)
        }

        fn constructed(&self) {
            self.parent_constructed();

            let mut tx_dataset = DatasetGroup::new();
            let rx_dataset = DatasetGroup::new();

            // scaling will be set by settings
            tx_dataset.dataset_settings.fill = FillingSettings::None;
            tx_dataset.dataset_settings.dashed = true;

            self.usage_graph.add_dataset(tx_dataset);
            self.usage_graph.add_dataset(rx_dataset);

            self.usage_graph.connect_datasets(0, 1);
            self.usage_graph.connect_datasets(1, 0);

            self.usage_graph.connect_to_settings(&settings!());

            let obj = self.obj();
            let this = obj.upcast_ref::<super::PerformancePageNetwork>().clone();

            Self::configure_actions(&this);
            Self::configure_context_menu(&this);

            let sidebar_content_builder = gtk::Builder::from_resource(
                "/io/missioncenter/MissionCenter/ui/performance_page/network_details.ui",
            );

            let _ = self.infobar_content.set(
                sidebar_content_builder
                    .object::<gtk::Box>("root")
                    .expect("Could not find `root` object in details pane"),
            );

            let _ = self.legend_send.set(
                sidebar_content_builder
                    .object::<gtk::Picture>("legend_send")
                    .expect("Could not find `legend_send` object in details pane"),
            );
            let _ = self.speed_send.set(
                sidebar_content_builder
                    .object::<gtk::Label>("speed_send")
                    .expect("Could not find `speed_send` object in details pane"),
            );
            let _ = self.total_sent.set(
                sidebar_content_builder
                    .object::<gtk::Label>("total_sent")
                    .expect("Could not find `total_send` object in details pane"),
            );
            let _ = self.legend_recv.set(
                sidebar_content_builder
                    .object::<gtk::Picture>("legend_recv")
                    .expect("Could not find `legend_recv` object in details pane"),
            );
            let _ = self.speed_recv.set(
                sidebar_content_builder
                    .object::<gtk::Label>("speed_recv")
                    .expect("Could not find `speed_recv` object in details pane"),
            );
            let _ = self.total_recv.set(
                sidebar_content_builder
                    .object::<gtk::Label>("total_recv")
                    .expect("Could not find `total_recv` object in details pane"),
            );
            let _ = self.interface_name_label.set(
                sidebar_content_builder
                    .object::<gtk::Label>("interface_name_label")
                    .expect("Could not find `interface_name_label` object in details pane"),
            );
            let _ = self.connection_type_label.set(
                sidebar_content_builder
                    .object::<gtk::Label>("connection_type_label")
                    .expect("Could not find `connection_type_label` object in details pane"),
            );
            let _ = self.connection_status_label.set(
                sidebar_content_builder
                    .object::<gtk::Label>("connection_status_label")
                    .expect("Could not find `connection_status_label` object in details pane"),
            );
            let _ = self.ssid.set(
                sidebar_content_builder
                    .object::<gtk::Label>("ssid")
                    .expect("Could not find `ssid` object in details pane"),
            );
            let _ = self.signal_strength.set(
                sidebar_content_builder
                    .object::<gtk::Image>("signal_strength")
                    .expect("Could not find `signal_strength` object in details pane"),
            );
            let _ = self.max_bitrate.set(
                sidebar_content_builder
                    .object::<gtk::Label>("max_bitrate")
                    .expect("Could not find `max_bitrate` object in details pane"),
            );
            let _ = self.frequency.set(
                sidebar_content_builder
                    .object::<gtk::Label>("frequency")
                    .expect("Could not find `frequency` object in details pane"),
            );
            let _ = self.hw_address.set(
                sidebar_content_builder
                    .object::<gtk::Label>("hw_address")
                    .expect("Could not find `hw_address` object in details pane"),
            );
            let _ = self.ipv4_address.set(
                sidebar_content_builder
                    .object::<gtk::Label>("ipv4_address")
                    .expect("Could not find `ipv4_address` object in details pane"),
            );
            let _ = self.ipv6_address.set(
                sidebar_content_builder
                    .object::<gtk::Label>("ipv6_address")
                    .expect("Could not find `ipv6_address` object in details pane"),
            );
        }
    }

    impl WidgetImpl for PerformancePageNetwork {}

    impl BoxImpl for PerformancePageNetwork {}
}

glib::wrapper! {
    pub struct PerformancePageNetwork(ObjectSubclass<imp::PerformancePageNetwork>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PageExt for PerformancePageNetwork {
    fn infobar_collapsed(&self) {
        self.imp()
            .infobar_content
            .get()
            .and_then(|ic| Some(ic.set_margin_top(10)));
    }

    fn infobar_uncollapsed(&self) {
        self.imp()
            .infobar_content
            .get()
            .and_then(|ic| Some(ic.set_margin_top(65)));
    }
}

impl PerformancePageNetwork {
    pub fn new(
        interface_name: &str,
        connection_kind: ConnectionKind,
        settings: &gio::Settings,
    ) -> Self {
        let this: Self = glib::Object::builder()
            .property("interface-name", interface_name)
            .build();

        this.imp().connection_type.set(connection_kind);

        fn update_refresh_rate_sensitive_labels(
            this: &PerformancePageNetwork,
            settings: &gio::Settings,
        ) {
            let data_points = settings.int("performance-page-data-points") as u32;
            let delay = settings.uint64("app-update-interval-u64");
            let graph_max_duration =
                (((delay as f64) * INTERVAL_STEP) * (data_points as f64)).round() as u32;

            let this = this.imp();
            this.graph_max_duration
                .set_text(&to_short_human_readable_time(graph_max_duration));
        }
        update_refresh_rate_sensitive_labels(&this, settings);

        this.imp()
            .use_bytes
            .set(settings.boolean("performance-page-network-use-bytes"));

        this.update_graph_scaling(settings);

        settings.connect_changed(Some("performance-page-network-dynamic-scaling"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    this.update_graph_scaling(settings);
                }
            }
        });

        settings.connect_changed(Some("performance-page-network-use-base2"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    this.update_graph_scaling(settings);
                }
            }
        });

        settings.connect_changed(Some("performance-page-network-use-bytes"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    this.imp()
                        .use_bytes
                        .set(settings.boolean("performance-page-network-use-bytes"));
                    this.update_graph_scaling(settings);
                }
            }
        });

        settings.connect_changed(Some("performance-page-data-points"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        settings.connect_changed(Some("app-update-interval-u64"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        this
    }

    pub fn set_static_information(&self, connection: &Connection) -> bool {
        imp::PerformancePageNetwork::set_static_information(self, connection)
    }

    pub fn update_readings(&self, connection: &Connection) -> bool {
        imp::PerformancePageNetwork::update_readings(self, connection)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePageNetwork::update_animations(self, ticks)
    }

    pub fn infobar_collapsed(&self) {
        self.imp()
            .infobar_content
            .get()
            .and_then(|ic| Some(ic.set_margin_top(10)));
    }

    pub fn infobar_uncollapsed(&self) {
        self.imp()
            .infobar_content
            .get()
            .and_then(|ic| Some(ic.set_margin_top(65)));
    }

    pub fn use_bytes(&self) -> bool {
        self.imp().use_bytes.get()
    }

    fn update_graph_scaling(&self, settings: &Settings) {
        let this = self.imp();
        let usage_graph = &this.usage_graph;

        if let Some(max_speed) = this.max_speed.get() {
            let dynamic_scaling = settings.boolean("performance-page-network-dynamic-scaling");

            if dynamic_scaling {
                usage_graph.set_all_datasets_scaling(ScalingSettings::ScaleUp);

                let base2 = settings.boolean("performance-page-network-use-base2");

                if base2 {
                    usage_graph.set_all_datasets_rounding(RoundingSettings::Pow2);
                } else {
                    usage_graph.set_all_datasets_rounding(RoundingSettings::Pow2Base10);

                    usage_graph.set_all_datasets_watermarking_multiplier(if this.use_bytes.get() {
                        1.
                    } else {
                        8.
                    });
                }

                usage_graph.reset_auto_scaling();
            } else {
                usage_graph.set_all_datasets_scaling(ScalingSettings::Fixed);

                usage_graph.set_all_datasets_max_scale(max_speed as f32);
            }
        } else {
            usage_graph.set_all_datasets_scaling(ScalingSettings::ScaleUp);

            let base2 = settings.boolean("performance-page-network-use-base2");

            if base2 {
                usage_graph.set_all_datasets_rounding(RoundingSettings::Pow2);
            } else {
                usage_graph.set_all_datasets_rounding(RoundingSettings::Pow2Base10);

                usage_graph.set_all_datasets_watermarking_multiplier(if this.use_bytes.get() {
                    1.
                } else {
                    8.
                });
            }

            usage_graph.reset_auto_scaling();
        }
    }
}
