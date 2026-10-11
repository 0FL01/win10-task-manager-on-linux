/* crates/mc-core/src/sidebar.rs
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

//! Performance sidebar ordering and per-device visibility, extracted from the GTK
//! `performance_page/mod.rs` and `performance_page/summary_graph.rs` together with their tests.

use std::collections::{HashMap, HashSet};

use mc_magpie::magpie_types::network::ConnectionKind;

use crate::config::Config;

pub const MAX_REMEMBERED_DEVICES: usize = 256;

/// Docker, bridge, veth and Multipass adapters: hidden unless View → "Show virtual network
/// adapters" is checked.
pub fn is_virtual_connection(kind: ConnectionKind) -> bool {
    matches!(
        kind,
        ConnectionKind::Docker
            | ConnectionKind::Multipass
            | ConnectionKind::Bridge
            | ConnectionKind::Virtual
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Cpu,
    Memory,
    Disk,
    Network { virtual_adapter: bool },
    Gpu,
    Fan,
    Battery,
    Unspecified,
}

impl DeviceType {
    /// Whether devices of this type are listed (virtual network adapters only on request).
    pub fn category_visible(&self, config: &Config) -> bool {
        match self {
            DeviceType::Network { virtual_adapter } => {
                !virtual_adapter || config.performance_show_network_virtual
            }
            _ => true,
        }
    }
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

pub fn prune_sidebar_order(order: &mut Vec<String>, present: &HashSet<String>, max: usize) {
    let mut excess = order.len().saturating_sub(max);
    let mut index = order.len();

    while excess > 0 && index > 0 {
        index -= 1;

        if !present.contains(&order[index]) {
            order.remove(index);
            excess -= 1;
        }
    }
}

pub fn canonical_key(name: &str) -> (u8, &str) {
    const CATEGORIES: [&str; 7] = ["cpu", "memory", "disk", "net", "gpu", "fan", "battery"];

    let category = CATEGORIES
        .iter()
        .position(|prefix| name.starts_with(prefix))
        .unwrap_or(CATEGORIES.len()) as u8;

    (category, name)
}

pub fn view_action_name(page_name: &str) -> Option<&'static str> {
    Some(match page_name.split('-').next().unwrap_or_default() {
        "cpu" => "cpu",
        "memory" => "memory",
        "disk" => "disk",
        "net" => "network",
        "gpu" => "gpu",
        "fan" => "fan",
        "battery" => "battery",
        _ => return None,
    })
}

pub fn load_sidebar_order(raw: &str) -> Vec<String> {
    raw.split(';')
        .filter(|entry| !entry.is_empty())
        .map(decode_device_name)
        .collect()
}

pub fn store_sidebar_order(order: &[String]) -> String {
    order
        .iter()
        .map(|name| encode_device_name(name))
        .collect::<Vec<_>>()
        .join(";")
}

pub fn merge_new_devices(order: &mut Vec<String>, mut present: Vec<String>) {
    present.sort_unstable_by(|a, b| canonical_key(a).cmp(&canonical_key(b)));

    for name in present {
        if order.iter().any(|known| known == &name) {
            continue;
        }

        let key = canonical_key(&name);
        let at = order
            .iter()
            .rposition(|known| {
                let known_key = canonical_key(known);
                known_key.0 == key.0 && known_key.1 < key.1
            })
            .or_else(|| {
                order
                    .iter()
                    .rposition(|known| canonical_key(known).0 == key.0)
            })
            .or_else(|| order.iter().rposition(|known| canonical_key(known) < key))
            .map_or(0, |index| index + 1);

        order.insert(at, name);
    }
}

/// Moves `dragged` before or after `target`. Returns `false` if `target` is unknown.
pub fn move_in_order(
    order: &mut Vec<String>,
    dragged: &str,
    target: &str,
    after_target: bool,
) -> bool {
    if dragged == target {
        return true;
    }

    let mut candidate = order.clone();
    candidate.retain(|name| name != dragged);

    let at = match candidate.iter().position(|name| name == target) {
        Some(index) if after_target => index + 1,
        Some(index) => index,
        None => return false,
    };
    candidate.insert(at, dragged.to_owned());
    *order = candidate;
    true
}

/// Computes the sidebar order for the devices currently present, updating the stored order
/// string in `config`. Returns the names in display order (absent devices omitted).
pub fn rebuild_order(config: &mut Config, present: &[String]) -> Vec<String> {
    let mut order = load_sidebar_order(&config.performance_sidebar_order);
    let present_set = present.iter().cloned().collect::<HashSet<_>>();

    merge_new_devices(&mut order, present.to_vec());
    prune_sidebar_order(&mut order, &present_set, MAX_REMEMBERED_DEVICES);

    let encoded = store_sidebar_order(&order);
    if encoded != config.performance_sidebar_order {
        config.performance_sidebar_order = encoded;
    }

    order
        .into_iter()
        .filter(|n| present_set.contains(n))
        .collect()
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    fn present(list: &[&str]) -> HashSet<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn prune_leaves_a_short_order_alone() {
        let mut order = names(&["cpu", "memory", "disk-sda"]);
        prune_sidebar_order(&mut order, &present(&["cpu"]), 256);
        assert_eq!(order, names(&["cpu", "memory", "disk-sda"]));
    }

    #[test]
    fn prune_drops_absent_devices_from_the_end() {
        let mut order = names(&["a", "b", "c", "d", "e"]);
        prune_sidebar_order(&mut order, &HashSet::new(), 3);
        assert_eq!(order, names(&["a", "b", "c"]));
    }

    #[test]
    fn prune_never_drops_a_present_device() {
        let mut order = names(&["a", "b", "c", "d", "e"]);
        prune_sidebar_order(&mut order, &present(&["d", "e"]), 3);
        assert_eq!(order, names(&["a", "d", "e"]));
    }

    #[test]
    fn prune_keeps_every_present_device_even_above_the_cap() {
        let mut order = names(&["a", "b", "c", "d", "e"]);
        prune_sidebar_order(&mut order, &present(&["a", "b", "c", "d", "e"]), 2);
        assert_eq!(order, names(&["a", "b", "c", "d", "e"]));
    }

    #[test]
    fn prune_keeps_the_cap_when_there_are_repeated_changes() {
        let mut order = names(&["cpu", "memory"]);

        for index in 0..1000 {
            let arriving = format!("net-veth{}", index);
            order.push(arriving.clone());
            prune_sidebar_order(
                &mut order,
                &present(&["cpu", "memory", arriving.as_str()]),
                MAX_REMEMBERED_DEVICES,
            );
        }

        assert_eq!(order.len(), MAX_REMEMBERED_DEVICES);
        assert!(order.contains(&"cpu".to_string()));
        assert!(order.contains(&"memory".to_string()));
        assert!(order.contains(&"net-veth999".to_string()));
    }

    fn merged(stored: &[&str], present: &[&str]) -> Vec<String> {
        let mut order = names(stored);
        merge_new_devices(&mut order, names(present));
        order
    }

    #[test]
    fn merge_into_an_empty_order_gives_default_order() {
        assert_eq!(
            merged(
                &[],
                &[
                    "net-eth1", "cpu", "disk-sdb", "net-eth0", "disk-sda", "memory"
                ]
            ),
            names(&[
                "cpu", "memory", "disk-sda", "disk-sdb", "net-eth0", "net-eth1"
            ])
        );
    }

    #[test]
    fn merge_joins_a_sibling_the_user_dragged_to_the_top() {
        assert_eq!(
            merged(
                &["net-eth0", "cpu", "memory", "disk-sda"],
                &["cpu", "memory", "disk-sda", "net-eth0", "net-eth1"]
            ),
            names(&["net-eth0", "net-eth1", "cpu", "memory", "disk-sda"])
        );
    }

    #[test]
    fn merge_leaves_the_top_row_alone() {
        assert_eq!(
            merged(
                &["net-eth1", "cpu", "memory"],
                &["cpu", "memory", "net-eth0", "net-eth1"]
            ),
            names(&["net-eth1", "net-eth0", "cpu", "memory"])
        );
    }

    #[test]
    fn merge_rejoins_a_group_a_drag_split_in_two() {
        assert_eq!(
            merged(
                &["net-eth0", "cpu", "net-eth2", "memory"],
                &["cpu", "memory", "net-eth0", "net-eth1", "net-eth2"]
            ),
            names(&["net-eth0", "net-eth1", "cpu", "net-eth2", "memory"])
        );
    }

    #[test]
    fn merge_uses_the_default_position_for_a_new_category() {
        assert_eq!(
            merged(&["net-eth0", "disk-sda"], &["cpu", "disk-sda", "net-eth0"]),
            names(&["cpu", "net-eth0", "disk-sda"])
        );
    }

    #[test]
    fn merge_keeps_siblings_arriving_together_contiguous() {
        assert_eq!(
            merged(
                &["cpu", "net-eth5"],
                &["cpu", "net-eth1", "net-eth2", "net-eth5"]
            ),
            names(&["cpu", "net-eth5", "net-eth1", "net-eth2"])
        );
    }

    #[test]
    fn merge_puts_a_newcomer_next_to_the_sibling_it_sorts_after() {
        assert_eq!(
            merged(
                &["net-eth0", "cpu", "memory", "disk-sda", "net-wlan0"],
                &[
                    "cpu",
                    "memory",
                    "disk-sda",
                    "net-eth0",
                    "net-wlan0",
                    "net-veth1a2b3c"
                ]
            ),
            names(&[
                "net-eth0",
                "net-veth1a2b3c",
                "cpu",
                "memory",
                "disk-sda",
                "net-wlan0"
            ])
        );
    }

    #[test]
    fn merge_does_not_change_order_once_every_device_is_known() {
        let stored = ["net-eth0", "cpu", "memory", "disk-sda"];
        let present = ["cpu", "memory", "disk-sda", "net-eth0"];

        let once = merged(&stored, &present);
        let mut twice = once.clone();
        merge_new_devices(&mut twice, names(&present));

        assert_eq!(once, names(&stored));
        assert_eq!(twice, once);
    }

    #[test]
    fn canonical_key_orders_categories() {
        let ordered = [
            "cpu",
            "memory",
            "disk-sda",
            "net-eth0",
            "gpu-0",
            "fan-0-0",
            "battery-BAT0",
        ];

        for pair in ordered.windows(2) {
            assert!(
                canonical_key(pair[0]) < canonical_key(pair[1]),
                "{} should sort before {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn canonical_key_puts_unknown_names_last() {
        let unknown = canonical_key("something-else");
        assert_eq!(unknown.0, 7);
        assert!(canonical_key("battery-BAT0") < unknown);
    }

    #[test]
    fn canonical_key_groups_a_category_together() {
        let mut sorted = names(&["net-eth1", "cpu", "disk-sdb", "net-eth0", "disk-sda"]);
        sorted.sort_by(|a, b| canonical_key(a).cmp(&canonical_key(b)));

        assert_eq!(
            sorted,
            names(&["cpu", "disk-sda", "disk-sdb", "net-eth0", "net-eth1"])
        );
    }

    #[test]
    fn view_action_name_maps_every_category() {
        let expected = [
            ("cpu", "cpu"),
            ("memory", "memory"),
            ("disk-sda", "disk"),
            ("net-eth0", "network"),
            ("gpu-0000:01:00.0", "gpu"),
            ("fan-0-1", "fan"),
            ("battery-0-BAT0", "battery"),
        ];

        for (page_name, action) in expected {
            assert_eq!(
                view_action_name(page_name),
                Some(action),
                "{} should use the {} action",
                page_name,
                action
            );
        }
    }

    #[test]
    fn view_action_name_rejects_names_outside_the_categories() {
        for page_name in ["", "cpufreq", "netlink-0", "something-else"] {
            assert_eq!(view_action_name(page_name), None);
        }
    }

    // Tests ported from summary_graph.rs

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

    // New behaviour of the egui port

    #[test]
    fn move_in_order_places_relative_to_target() {
        let mut order = names(&["cpu", "memory", "disk-sda"]);
        assert!(move_in_order(&mut order, "disk-sda", "cpu", false));
        assert_eq!(order, names(&["disk-sda", "cpu", "memory"]));
        assert!(move_in_order(&mut order, "disk-sda", "memory", true));
        assert_eq!(order, names(&["cpu", "memory", "disk-sda"]));
        assert!(!move_in_order(&mut order, "cpu", "gpu-0", true));
        assert_eq!(order, names(&["cpu", "memory", "disk-sda"]));
    }

    #[test]
    fn rebuild_order_stores_and_filters() {
        let mut config = Config::default();
        config.performance_sidebar_order = "net-eth0;cpu;gone".into();
        let shown = rebuild_order(&mut config, &names(&["cpu", "memory", "net-eth0"]));
        assert_eq!(shown, names(&["net-eth0", "cpu", "memory"]));
        assert_eq!(config.performance_sidebar_order, "net-eth0;cpu;memory;gone");
    }
}
