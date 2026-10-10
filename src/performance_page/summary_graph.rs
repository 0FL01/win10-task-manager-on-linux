/* performance_page/summary_graph.rs
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

use std::cell::Cell;
use std::collections::HashMap;

use adw::subclass::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::{gdk, glib, prelude::*};

use crate::performance_page::widgets::{GraphWidget, SidebarDropHint};
use crate::settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkGroup {
    Wired,
    Wireless,
    Vpn,
    Virtual,
    Other,
}

impl NetworkGroup {
    pub fn from_connection_kind(kind: magpie_types::network::ConnectionKind) -> Self {
        use magpie_types::network::ConnectionKind;
        match kind {
            ConnectionKind::Wired => NetworkGroup::Wired,
            ConnectionKind::Wireless => NetworkGroup::Wireless,
            ConnectionKind::Vpn => NetworkGroup::Vpn,
            ConnectionKind::Docker
            | ConnectionKind::Multipass
            | ConnectionKind::Bridge
            | ConnectionKind::Virtual => NetworkGroup::Virtual,
            ConnectionKind::Bluetooth
            | ConnectionKind::InfiniBand
            | ConnectionKind::Wwan
            | ConnectionKind::Other => NetworkGroup::Other,
        }
    }

    pub fn settings_key(&self) -> &'static str {
        match self {
            NetworkGroup::Wired => "performance-show-network-wired",
            NetworkGroup::Wireless => "performance-show-network-wireless",
            NetworkGroup::Vpn => "performance-show-network-vpn",
            NetworkGroup::Virtual => "performance-show-network-virtual",
            NetworkGroup::Other => "performance-show-network-other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Cpu,
    Memory,
    Disk,
    Network(NetworkGroup),
    Gpu,
    Fan,
    Battery,
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceOverride {
    Show,
    Hide,
}

pub fn encode_device_name(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());

    for ch in name.chars() {
        match ch {
            '%' => encoded.push_str("%25"),
            ';' => encoded.push_str("%3B"),
            ':' => encoded.push_str("%3A"),
            _ => encoded.push(ch),
        }
    }

    encoded
}

pub fn decode_device_name(encoded: &str) -> String {
    let mut name = String::with_capacity(encoded.len());
    let mut rest = encoded;

    while let Some(at) = rest.find('%') {
        name.push_str(&rest[..at]);

        let (decoded, consumed) = match rest.as_bytes().get(at + 1..at + 3) {
            Some(b"25") => ('%', 3),
            Some(b"3B") => (';', 3),
            Some(b"3A") => (':', 3),
            _ => ('%', 1),
        };

        name.push(decoded);
        rest = &rest[at + consumed..];
    }

    name.push_str(rest);
    name
}

pub fn parse_device_overrides(raw: &str) -> HashMap<String, DeviceOverride> {
    raw.split(';')
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let (name, action) = entry.rsplit_once(':')?;
            let ovr = match action {
                "show" => DeviceOverride::Show,
                "hide" => DeviceOverride::Hide,
                _ => return None,
            };
            Some((decode_device_name(name), ovr))
        })
        .collect()
}

pub fn serialize_device_overrides(overrides: &HashMap<String, DeviceOverride>) -> String {
    let mut entries = overrides
        .iter()
        .map(|(name, ovr)| {
            let action = match ovr {
                DeviceOverride::Show => "show",
                DeviceOverride::Hide => "hide",
            };
            format!("{}:{}", encode_device_name(name), action)
        })
        .collect::<Vec<_>>();

    entries.sort_unstable();
    entries.join(";")
}

pub fn resolve_device_visibility(
    device_name: &str,
    overrides: &HashMap<String, DeviceOverride>,
    category_visible: bool,
) -> bool {
    match overrides.get(device_name) {
        Some(DeviceOverride::Show) => true,
        Some(DeviceOverride::Hide) => false,
        None => category_visible,
    }
}

mod imp {
    use super::*;
    use crate::performance_page::widgets::GraphWidget;
    use std::marker::PhantomData;

    #[derive(Properties)]
    #[properties(wrapper_type = super::SummaryGraph)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/summary_graph.ui")]
    #[allow(dead_code)]
    pub struct SummaryGraph {
        pub drop_hint: SidebarDropHint,

        #[template_child]
        pub drag_handle_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub graph_widget: TemplateChild<GraphWidget>,
        #[template_child]
        label_heading: TemplateChild<gtk::Label>,
        #[template_child]
        label_info1: TemplateChild<gtk::Label>,
        #[template_child]
        label_info2: TemplateChild<gtk::Label>,
        #[template_child]
        pub enabled_switch: TemplateChild<gtk::Switch>,

        #[property(get = Self::base_color, set = Self::set_base_color)]
        base_color: PhantomData<gdk::RGBA>,
        #[property(get = Self::heading, set = Self::set_heading)]
        heading: PhantomData<String>,
        #[property(get = Self::info1, set = Self::set_info1)]
        info1: PhantomData<String>,
        #[property(get = Self::info2, set = Self::set_info2)]
        info2: PhantomData<String>,

        pub device_type: Cell<DeviceType>,

        pub user_activated: Cell<bool>,
    }

    impl Default for SummaryGraph {
        fn default() -> Self {
            Self {
                drop_hint: SidebarDropHint::new(),
                drag_handle_icon: Default::default(),
                graph_widget: Default::default(),
                label_heading: Default::default(),
                label_info1: Default::default(),
                label_info2: Default::default(),
                enabled_switch: Default::default(),

                base_color: PhantomData,
                heading: PhantomData,
                info1: PhantomData,
                info2: PhantomData,

                device_type: Cell::new(DeviceType::Unspecified),

                user_activated: Cell::new(true),
            }
        }
    }

    impl SummaryGraph {
        fn base_color(&self) -> gdk::RGBA {
            self.graph_widget.base_color()
        }

        fn set_base_color(&self, base_color: gdk::RGBA) {
            self.graph_widget.set_base_color(base_color);
        }

        fn heading(&self) -> String {
            self.label_heading.text().to_string()
        }

        fn set_heading(&self, heading: String) {
            self.label_heading.set_text(&heading);
        }

        fn info1(&self) -> String {
            self.label_info1.text().to_string()
        }

        fn set_info1(&self, info1: String) {
            self.label_info1.set_text(&info1);
            if info1.is_empty() {
                self.label_info1.set_visible(false);
            } else {
                self.label_info1.set_visible(true);
            }
        }

        fn info2(&self) -> String {
            self.label_info2.text().to_string()
        }

        fn set_info2(&self, info2: String) {
            self.label_info2.set_text(&info2);
            if info2.is_empty() {
                self.label_info2.set_visible(false);
            } else {
                self.label_info2.set_visible(true);
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SummaryGraph {
        const NAME: &'static str = "SummaryGraph";
        type Type = super::SummaryGraph;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SummaryGraph {
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

            self.enabled_switch.connect_active_notify({
                let this = self.obj().downgrade();
                move |switch| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    if !this.imp().user_activated.get() {
                        return;
                    }

                    let settings = settings!();

                    let raw = settings.string("performance-sidebar-device-overrides");
                    let mut overrides = parse_device_overrides(&raw);
                    let name = this.widget_name();

                    if switch.is_active() {
                        overrides.insert(name.to_string(), DeviceOverride::Show);
                    } else {
                        overrides.insert(name.to_string(), DeviceOverride::Hide);
                    }

                    let output = serialize_device_overrides(&overrides);

                    settings
                        .set_string("performance-sidebar-device-overrides", &output)
                        .unwrap_or_else(|_| {
                            glib::g_warning!(
                                "MissionCenter::PerformancePage",
                                "Failed to set performance-sidebar-device-overrides setting"
                            );
                        });
                }
            });
        }
    }

    impl WidgetImpl for SummaryGraph {}

    impl BoxImpl for SummaryGraph {}
}

glib::wrapper! {
    pub struct SummaryGraph(ObjectSubclass<imp::SummaryGraph>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl SummaryGraph {
    pub fn new(device_type: DeviceType) -> Self {
        let this: SummaryGraph = glib::Object::builder().build();
        this.imp().device_type.set(device_type);
        this
    }

    pub fn set_edit_mode(&self, edit_mode: bool, switch_enabled: bool) {
        let imp = self.imp();
        imp.drag_handle_icon.set_visible(edit_mode);
        imp.enabled_switch.set_visible(edit_mode);

        self.set_switch_active(switch_enabled);
    }

    pub fn set_switch_active(&self, active: bool) {
        let imp = self.imp();
        imp.user_activated.set(false);
        imp.enabled_switch.set_active(active);
        imp.user_activated.set(true);
    }

    pub fn graph_widget(&self) -> GraphWidget {
        self.imp().graph_widget.clone()
    }

    pub fn show_drop_hint_top(&self) {
        self.hide_drop_hint();

        self.imp().drop_hint.set_margin_bottom(20);

        self.prepend(&self.imp().drop_hint);
    }

    pub fn show_drop_hint_bottom(&self) {
        self.hide_drop_hint();

        self.imp().drop_hint.set_margin_top(20);

        self.append(&self.imp().drop_hint);
    }

    pub fn hide_drop_hint(&self) {
        if !self.is_drop_hint_visible() {
            return;
        }

        self.imp().drop_hint.set_margin_top(0);
        self.imp().drop_hint.set_margin_bottom(0);

        self.remove(&self.imp().drop_hint);
    }

    pub fn is_drop_hint_visible(&self) -> bool {
        self.imp().drop_hint.parent().is_some()
    }

    pub fn is_drop_hint_top(&self) -> bool {
        if !self.is_drop_hint_visible() {
            return false;
        }

        self.imp().drop_hint.prev_sibling().is_none()
    }

    pub fn is_drop_hint_bottom(&self) -> bool {
        if !self.is_drop_hint_visible() {
            return false;
        }

        self.imp().drop_hint.next_sibling().is_none()
    }

    pub fn device_type(&self) -> DeviceType {
        self.imp().device_type.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        for name in [
            "net-eth0",
            "net-a;b",
            "50%",
            "odd:name",
            "%3B",
            "",
            "net-a;b:hide",
            "%%%",
            "üñî;çø∂é",
        ] {
            assert_eq!(decode_device_name(&encode_device_name(name)), name);
        }
    }

    #[test]
    fn encoded_names_are_free_of_separators() {
        let encoded = encode_device_name("net-a;b:c");
        assert!(!encoded.contains(';'));
        assert!(!encoded.contains(':'));
        assert_eq!(encoded, "net-a%3Bb%3Ac");
    }

    #[test]
    fn ordinary_names_are_untouched() {
        for name in [
            "cpu",
            "memory",
            "disk-sda",
            "net-eth0",
            "gpu-0",
            "battery-BAT0",
        ] {
            assert_eq!(encode_device_name(name), name);
            assert_eq!(decode_device_name(name), name);
        }
    }

    #[test]
    fn decode_leaves_legacy_values_unchanged() {
        for legacy in ["50%", "%", "%zz", "%3", "100%done", "a%b%c"] {
            assert_eq!(decode_device_name(legacy), legacy);
        }
    }

    #[test]
    fn overrides_roundtrip_through_settings_string() {
        let mut overrides = HashMap::new();
        overrides.insert("net-a;b".to_string(), DeviceOverride::Hide);
        overrides.insert("cpu".to_string(), DeviceOverride::Show);
        overrides.insert("weird:show".to_string(), DeviceOverride::Show);
        overrides.insert("50%".to_string(), DeviceOverride::Hide);

        let serialized = serialize_device_overrides(&overrides);
        assert_eq!(serialized.matches(';').count(), overrides.len() - 1);
        assert_eq!(parse_device_overrides(&serialized), overrides);
    }

    #[test]
    fn overrides_resolve_for_names_holding_separators() {
        let mut overrides = HashMap::new();
        overrides.insert("net-a;b".to_string(), DeviceOverride::Hide);

        let parsed = parse_device_overrides(&serialize_device_overrides(&overrides));
        assert!(!resolve_device_visibility("net-a;b", &parsed, true));
        assert!(resolve_device_visibility("net-a", &parsed, true));
        assert!(resolve_device_visibility("b", &parsed, true));
    }

    #[test]
    fn legacy_unencoded_overrides_still_parse() {
        let parsed = parse_device_overrides("cpu:show;disk-sda:hide");
        assert_eq!(parsed.get("cpu"), Some(&DeviceOverride::Show));
        assert_eq!(parsed.get("disk-sda"), Some(&DeviceOverride::Hide));
    }

    #[test]
    fn serialized_overrides_do_not_depend_on_insertion_order() {
        let entries = [
            ("cpu", DeviceOverride::Show),
            ("disk-sda", DeviceOverride::Hide),
            ("net-eth0", DeviceOverride::Hide),
            ("gpu-0", DeviceOverride::Show),
        ];

        let mut forward = HashMap::new();
        for (name, ovr) in entries {
            forward.insert(name.to_string(), ovr);
        }

        let mut backward = HashMap::new();
        for (name, ovr) in entries.iter().rev() {
            backward.insert(name.to_string(), *ovr);
        }

        let serialized = serialize_device_overrides(&forward);
        assert_eq!(serialized, serialize_device_overrides(&backward));
        assert_eq!(
            serialized,
            "cpu:show;disk-sda:hide;gpu-0:show;net-eth0:hide"
        );
        assert_eq!(parse_device_overrides(&serialized), forward);
    }
}
