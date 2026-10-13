/* src/services/systemd/properties.rs
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

//! Unit property tracking without zbus property proxies.
//!
//! A cached zbus `Proxy` stores every property of its interface (the systemd `Unit` and
//! `Service` interfaces have hundreds, including large exec structures) and registers its own
//! match rule. Doing that for each unit on both buses dominated the daemon heap. Only four
//! properties are used, so they are read with `Get` and kept current from one bus-wide
//! `PropertiesChanged` subscription. Both arrive as stamped observations in [`PendingMap`];
//! the newest message wins, as with the zbus property cache.

use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ahash::AHasher;
use futures_lite::StreamExt;
use zbus::fdo::PropertiesChanged;
use zbus::message::Type;
use zbus::names::OwnedUniqueName;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, Message, MessageStream};

const SYSTEMD_DESTINATION: &str = "org.freedesktop.systemd1";
const UNIT_PATH_NAMESPACE: &str = "/org/freedesktop/systemd1/unit";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";
const SERVICE_INTERFACE: &str = "org.freedesktop.systemd1.Service";

/// Signals queued between polls of the subscription; the watcher drains it continuously and
/// coalesces per unit, so this only absorbs bursts (zbus applies back-pressure when full).
const SIGNAL_QUEUE: usize = 256;
/// Initial reads in flight; enough to keep the bus busy without buffering every reply of a
/// full unit list at once.
const CONCURRENT_READS: usize = 32;

pub fn service_id(bus_id: &str, unit_path: &str) -> u64 {
    let mut hasher = AHasher::default();
    hasher.write(bus_id.as_bytes());
    hasher.write(unit_path.as_bytes());
    hasher.finish()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tracked {
    ActiveState,
    MainPid,
    User,
    Group,
}

impl Tracked {
    pub const ALL: [Self; 4] = [Self::ActiveState, Self::MainPid, Self::User, Self::Group];

    fn interface(self) -> &'static str {
        match self {
            Self::ActiveState => UNIT_INTERFACE,
            Self::MainPid | Self::User | Self::Group => SERVICE_INTERFACE,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::ActiveState => "ActiveState",
            Self::MainPid => "MainPID",
            Self::User => "User",
            Self::Group => "Group",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn from_signal(interface: &str, name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.interface() == interface && p.name() == name)
    }

    /// Properties a unit exposes; only `.service` units implement the `Service` interface.
    pub fn for_unit(unit_name: &str) -> &'static [Self] {
        if unit_name.ends_with(".service") {
            &Self::ALL
        } else {
            &Self::ALL[..1]
        }
    }

    fn parse(self, value: &Value<'_>) -> Option<TrackedValue> {
        match self {
            Self::MainPid => u32::try_from(value).ok().map(TrackedValue::Pid),
            Self::ActiveState | Self::User | Self::Group => <&str>::try_from(value)
                .ok()
                .map(|text| TrackedValue::Text(text.to_owned())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackedValue {
    Text(String),
    Pid(u32),
}

/// Position of a message in the sender's stream. Serials are only ordered within one
/// connection: after systemd re-executes it reconnects with a new unique name, and the
/// message received later wins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    sender: Option<OwnedUniqueName>,
    serial: u32,
}

impl Stamp {
    fn of(message: &Message) -> Self {
        Self {
            sender: message.header().sender().map(|s| s.to_owned().into()),
            serial: message.primary_header().serial_num().get(),
        }
    }

    /// Whether `self`, received after `previous`, carries newer state.
    pub fn supersedes(&self, previous: Option<&Stamp>) -> bool {
        match previous {
            None => true,
            Some(previous) if previous.sender != self.sender => true,
            Some(previous) => self.serial > previous.serial,
        }
    }

    #[cfg(test)]
    pub fn test(sender: &str, serial: u32) -> Self {
        Self {
            sender: Some(OwnedUniqueName::try_from(sender).unwrap()),
            serial,
        }
    }
}

/// A received value; `None` when systemd invalidated the property without a value.
pub type Observation = (Stamp, Option<TrackedValue>);

/// Newest unapplied observation per tracked property of one unit.
#[derive(Debug, Default)]
pub struct Pending([Option<Observation>; 4]);

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    fn record(&mut self, property: Tracked, stamp: Stamp, value: Option<TrackedValue>) {
        let slot = &mut self.0[property.index()];
        if stamp.supersedes(slot.as_ref().map(|(s, _)| s)) {
            *slot = Some((stamp, value));
        }
    }

    pub fn take(&mut self) -> impl Iterator<Item = (Tracked, Observation)> + '_ {
        Tracked::ALL
            .into_iter()
            .zip(self.0.iter_mut())
            .filter_map(|(property, slot)| slot.take().map(|o| (property, o)))
    }
}

/// Pending changes keyed by service id. Only units present in the service cache have an
/// entry, so the map is bounded by the cache and changes of other units are dropped.
#[derive(Clone, Default)]
pub struct PendingMap(Arc<Mutex<HashMap<u64, Pending>>>);

impl PendingMap {
    pub fn lock(&self) -> MutexGuard<'_, HashMap<u64, Pending>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Subscribes to unit property changes on `bus`. Call before reading initial values so no
/// change between the read and the subscription is lost.
pub async fn subscribe(bus: &Connection) -> zbus::Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(SYSTEMD_DESTINATION)?
        .interface(PROPERTIES_INTERFACE)?
        .member("PropertiesChanged")?
        .path_namespace(UNIT_PATH_NAMESPACE)?
        .build();
    MessageStream::for_match_rule(rule, bus, Some(SIGNAL_QUEUE)).await
}

/// Records every tracked change for cached units until the connection closes.
pub async fn watch(mut stream: MessageStream, bus_id: &'static str, pending: PendingMap) {
    while let Some(message) = stream.next().await {
        let Ok(message) = message else {
            continue;
        };
        let Some(path) = message.header().path().map(|p| p.to_string()) else {
            continue;
        };
        let stamp = Stamp::of(&message);
        let Some(signal) = PropertiesChanged::from_message(message) else {
            continue;
        };
        let Ok(args) = signal.args() else {
            continue;
        };
        let interface = args.interface_name().as_str();

        let mut pending = pending.lock();
        let Some(entry) = pending.get_mut(&service_id(bus_id, &path)) else {
            continue;
        };
        for (name, value) in args.changed_properties() {
            if let Some(property) = Tracked::from_signal(interface, name) {
                // An unparsable value is refetched like an invalidated one.
                entry.record(property, stamp.clone(), property.parse(value));
            }
        }
        for name in args.invalidated_properties().iter() {
            if let Some(property) = Tracked::from_signal(interface, name) {
                entry.record(property, stamp.clone(), None);
            }
        }
    }
}

/// Reads properties in the background and records the values as observations. Units missing
/// from `pending` (removed meanwhile) are skipped.
pub async fn read(
    bus: &Connection,
    reads: Vec<(u64, OwnedObjectPath, Tracked)>,
    pending: PendingMap,
) {
    // `futures_lite::StreamExt` is imported for `next`; this combinator is from `futures`.
    futures::StreamExt::for_each_concurrent(
        futures::stream::iter(reads),
        CONCURRENT_READS,
        |(id, path, property)| {
            let pending = &pending;
            async move {
                if let Some((stamp, value)) = fetch(bus, &path, property).await {
                    if let Some(entry) = pending.lock().get_mut(&id) {
                        entry.record(property, stamp, Some(value));
                    }
                }
            }
        },
    )
    .await;
}

/// Reads one property; `None` when the unit or interface does not provide it.
async fn fetch(
    bus: &Connection,
    path: &ObjectPath<'_>,
    property: Tracked,
) -> Option<(Stamp, TrackedValue)> {
    let reply = bus
        .call_method(
            Some(SYSTEMD_DESTINATION),
            path,
            Some(PROPERTIES_INTERFACE),
            "Get",
            &(property.interface(), property.name()),
        )
        .await
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    let value = property.parse(&value)?;
    Some((Stamp::of(&reply), value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_serial_from_same_sender_wins() {
        let mut pending = Pending::default();
        let pid = |n| Some(TrackedValue::Pid(n));
        pending.record(Tracked::MainPid, Stamp::test(":1.1", 10), pid(1));
        pending.record(Tracked::MainPid, Stamp::test(":1.1", 9), pid(2));
        pending.record(Tracked::User, Stamp::test(":1.1", 3), None);

        let taken: Vec<_> = pending.take().collect();
        assert_eq!(
            taken,
            vec![
                (Tracked::MainPid, (Stamp::test(":1.1", 10), pid(1))),
                (Tracked::User, (Stamp::test(":1.1", 3), None)),
            ]
        );
        assert_eq!(pending.take().count(), 0);
    }

    #[test]
    fn reconnected_sender_supersedes_higher_old_serial() {
        let old = Stamp::test(":1.1", 5000);
        assert!(Stamp::test(":1.9", 3).supersedes(Some(&old)));
        assert!(!Stamp::test(":1.1", 4999).supersedes(Some(&old)));
        assert!(!Stamp::test(":1.1", 5000).supersedes(Some(&old)));
        assert!(Stamp::test(":1.1", 1).supersedes(None));
    }

    #[test]
    fn signal_names_map_to_their_interface_only() {
        assert_eq!(
            Tracked::from_signal(UNIT_INTERFACE, "ActiveState"),
            Some(Tracked::ActiveState)
        );
        assert_eq!(
            Tracked::from_signal(SERVICE_INTERFACE, "MainPID"),
            Some(Tracked::MainPid)
        );
        assert_eq!(Tracked::from_signal(SERVICE_INTERFACE, "ActiveState"), None);
        assert_eq!(Tracked::from_signal(UNIT_INTERFACE, "SubState"), None);
        assert_eq!(Tracked::for_unit("a.socket"), &[Tracked::ActiveState]);
        assert_eq!(Tracked::for_unit("a.service").len(), 4);
    }

    #[test]
    fn values_parse_by_property_type() {
        assert_eq!(
            Tracked::MainPid.parse(&Value::from(42u32)),
            Some(TrackedValue::Pid(42))
        );
        assert_eq!(
            Tracked::User.parse(&Value::from("root")),
            Some(TrackedValue::Text("root".into()))
        );
        assert_eq!(Tracked::MainPid.parse(&Value::from("x")), None);
    }
}
