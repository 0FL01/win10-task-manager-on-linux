/* src/battery.rs
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

use crate::{async_runtime, sync, system_bus, util::stream_has_contents};
use magpie_platform::battery::{Battery, HistoryPoint};

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::read_to_string;
use std::path::Path;
use std::str::FromStr;
use std::task::{Context, Poll};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use futures::stream::Stream;

use zbus::proxy::PropertyStream;
use zbus::zvariant::ObjectPath;

use upower_dbus::{BatteryType, DeviceAddedStream, DeviceProxy, DeviceRemovedStream, UPowerProxy};

const BAT_BASE_DIR: &str = "/sys/class/power_supply/";

struct UPowerData {
    device: DeviceProxy<'static>,
    stream: PropertyStream<'static, f64>,
    objpath: ObjectPath<'static>,
}

pub struct BatteryCache {
    batteries: Vec<Battery>,

    upower: Option<UPowerProxy<'static>>,
    devices: HashMap<String, (Option<UPowerData>, bool)>,

    denylist: HashSet<String>,

    last_full_update: Instant,
    devices_stream: Option<(DeviceAddedStream, DeviceRemovedStream)>,
}

fn read_from_sysfs(name: &str, appendix: &str) -> Option<String> {
    let path = format!("{}{}/{}", BAT_BASE_DIR, name, &appendix);
    match read_to_string(path) {
        Ok(c) => Some(c.trim().to_string()),
        Err(e) => {
            log::info!(
                "Could not read file: `{}{}/{}`: {}",
                BAT_BASE_DIR,
                name,
                appendix,
                e
            );
            None
        }
    }
}

fn parse_str<T: std::str::FromStr>(c: String) -> Option<T>
where
    <T as FromStr>::Err: std::fmt::Display,
{
    match c.parse::<T>() {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!("Could not parse `{}`: {}", c, e);
            None
        }
    }
}

fn zero_to_none<T: IsZero>(x: T) -> Option<T> {
    if x.is_zero() {
        return None;
    }
    Some(x)
}

fn bit_set(x: u32, b: u32) -> bool {
    (x & 2_u32.pow(b)) != 0
}

trait IsZero {
    fn is_zero(&self) -> bool;
}

impl IsZero for f32 {
    #[inline]
    fn is_zero(&self) -> bool {
        self.abs() == 0.0
    }
}

impl IsZero for i32 {
    #[inline]
    fn is_zero(&self) -> bool {
        self.abs() == 0
    }
}

impl IsZero for u32 {
    #[inline]
    fn is_zero(&self) -> bool {
        *self == 0 || *self == u32::MAX
    }
}

impl IsZero for String {
    #[inline]
    fn is_zero(&self) -> bool {
        self.is_empty()
    }
}

macro_rules! get_upower_value {
    ($device:ident, $upower_name:ident, $typ:ty, $zero:expr, $math:tt) => {
        $device
            .$upower_name()
            .await
            .ok()
            .map(|x| $math(x) as $typ)
            .and_then(|x| {
                if $zero && x.is_zero() {
                    return None;
                }
                Some(x)
            })
    };
}

macro_rules! get_sysfs_value {
    ($name:ident, $file_name:literal, $typ:ty, $zero:expr, $math:tt) => {
        read_from_sysfs(&$name, $file_name)
            .and_then(|x| parse_str(x))
            .and_then(|x: $typ| {
                if $zero {
                    zero_to_none($math(x))
                } else {
                    Some($math(x))
                }
            })
    };
}

async fn new_battery(
    name: String,
    device_upower: &mut Option<UPowerData>,
    sysfs_exists: bool,
) -> Option<Battery> {
    let mut kind = None;
    if let Some(device_upower) = device_upower {
        let batterytype = device_upower.device.type_().await.ok();
        if let Some(batterytype) = batterytype {
            match batterytype {
                BatteryType::LinePower => return None,
                v => kind = zero_to_none(v as i32),
            }
        }
    } else if sysfs_exists {
        if let Some(v) = read_from_sysfs(&name, "type") {
            kind = match &*v {
                "Mains" | "Unknown" => return None,
                "Battery" => Some(BatteryType::Battery as i32),
                _ => {
                    log::warn!("unknown battery type: {}, please open an issue. Falling back to upower for type", v);
                    None
                }
            }
        }
    }

    let mut vendor = None;
    let mut model = None;
    let mut serial = None;

    let mut energy_empty = None;
    let mut energy_full = None;
    let mut energy_full_design = None;
    let mut voltage_min_design = None;
    let mut voltage_max_design = None;
    let mut capacity = None;
    let mut technology = None;

    if sysfs_exists {
        macro_rules! set_sysfs_value {
            ($e:ident, $typ:ty, $math:tt) => {
                $e = get_sysfs_value!(name, "$e", $typ, true, $math);
            };
            ($e:ident, $i:literal, $typ:ty, $math:tt) => {
                $e = get_sysfs_value!(name, $i, $typ, true, $math);
            };
        }

        let div_k = |x| x / 1000;
        let div_m = |x| x / 1_000_000.;

        vendor = read_from_sysfs(&name, "manufacturer");
        model = read_from_sysfs(&name, "model_name");
        serial = read_from_sysfs(&name, "serial_number");

        set_sysfs_value!(energy_full, u32, div_k);
        set_sysfs_value!(energy_full_design, u32, div_k);
        set_sysfs_value!(energy_empty, u32, div_k);

        set_sysfs_value!(voltage_min_design, f32, div_m);
        set_sysfs_value!(voltage_max_design, f32, div_m);

        capacity = if let (Some(energy_full), Some(energy_full_design)) =
            (energy_full, energy_full_design)
        {
            Some(energy_full as f32 / energy_full_design as f32)
        } else {
            None
        };

        technology = match read_from_sysfs(&name, "technology") {
            Some(v) => match v.as_str() {
                "Li-poly" => Some(2),
                "Unknown" => None,
                _ => {
                    log::warn!("unknown battery technology: {}, please open an issue. Falling back to upower for technology", v);
                    None
                }
            },
            None => None,
        };
    }

    let (power_supply, charge_threshold_supported) = if let Some(device_upower) = device_upower {
        let device = &device_upower.device;
        macro_rules! set_upower_value {
            ($e:ident, $typ:ty) => {
                set_upower_value!($e, $typ, (|x| { x }))
            };
            ($var_name:ident, $typ:ty, $math:tt) => {
                $var_name = $var_name.or(get_upower_value!(device, $var_name, $typ, true, $math))
            };
        }

        set_upower_value!(vendor, String);
        set_upower_value!(model, String);
        set_upower_value!(serial, String);

        set_upower_value!(energy_full, u32, (|x| x * 1000.));
        set_upower_value!(energy_full_design, u32, (|x| x * 1000.));
        set_upower_value!(energy_empty, u32, (|x| x * 1000.));

        set_upower_value!(capacity, f32, (|x| x / 100.));

        set_upower_value!(voltage_min_design, f32);
        set_upower_value!(voltage_max_design, f32);

        set_upower_value!(technology, i32);

        (
            device.power_supply().await.ok(),
            device
                .charge_threshold_settings_supported()
                .await
                .unwrap_or(0),
        )
    } else {
        (None, 0)
    };
    let design_capacity = crate::smbios::design_capacity_mwh();
    let mut battery = Battery {
        name,
        vendor,
        model,
        serial,

        kind,
        technology,
        power_supply,

        capacity,
        energy_empty,
        energy_full,
        energy_full_design: energy_full_design.or(design_capacity),
        voltage_min_design,
        voltage_max_design,

        charge_threshold_supported,

        ..Battery::default()
    };

    if !update_battery(&mut battery, device_upower, sysfs_exists, true).await {
        return None;
    }

    Some(battery)
}

async fn update_battery(
    battery: &mut Battery,
    device: &mut Option<UPowerData>,
    sysfs_exists: bool,
    mut update_upower: bool,
) -> bool {
    battery.percentage = {
        let parsed: std::option::Option<f32> =
            read_from_sysfs(&battery.name, "capacity").and_then(parse_str);
        if let Some(v) = parsed {
            if let Some(device_upower) = device {
                if update_upower {
                    let _ = stream_has_contents(&mut device_upower.stream).await;
                } else {
                    update_upower = stream_has_contents(&mut device_upower.stream)
                        .await
                        .is_some()
                }
            }
            v / 100.
        } else if update_upower {
            if let Some(device_upower) = device {
                let v = device_upower.device.percentage().await.ok();
                let _ = stream_has_contents(&mut device_upower.stream).await; // empty stream
                if let Some(v) = v {
                    v as f32 / 100.
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            if let Some(device_upower) = device {
                if let Some(Ok(v)) = stream_has_contents(&mut device_upower.stream).await {
                    update_upower = true;
                    v as f32 / 100.
                } else {
                    battery.percentage
                }
            } else {
                battery.percentage
            }
        }
    };

    if sysfs_exists {
        battery.energy = None;
        battery.voltage = None;
        battery.charge_cycles = None;
        battery.power = None;
    }

    if sysfs_exists {
        let name = &battery.name;
        macro_rules! set_sysfs_value {
            ($e:ident, $typ:ty, $zero:expr, $math:tt) => {
                set_sysfs_value!($e, "$e", $typ, $zero, $math);
            };
            ($e:ident, $i:literal, $typ:ty, $zero:expr, $math:tt) => {
                battery.$e = get_sysfs_value!(name, $i, $typ, $zero, $math);
            };
        }

        let div_k = |x| x / 1000;
        let div_m = |x| x / 1_000_000.;
        let nothing = |x| x;

        set_sysfs_value!(energy, "energy_now", u32, false, div_k);
        set_sysfs_value!(voltage, "voltage_now", f32, true, div_m);

        set_sysfs_value!(power, "power_now", f32, false, div_m);
        if let (None, Some(voltage)) = (battery.power, battery.voltage) {
            set_sysfs_value!(power, "current_now", f32, false, (|x| div_m(x) * voltage));
        }

        set_sysfs_value!(charge_cycles, "cycle_count", u32, false, nothing);

        battery.state = read_from_sysfs(&battery.name, "status").and_then(|v| match v.as_str() {
            "Charging" => Some(1),
            "Discharging" => Some(2),
            "Empty" => Some(3),
            "Full" => Some(4),
            "Not charging" => Some(5),
            "Pending discharge" => Some(6),
            "Unknown" => None,
            v => {
                log::warn!("Unknonwn battery status: {v}, please open an issue. Falling back to upower for state");
                None // falls back to upower
            },
        });

        if bit_set(battery.charge_threshold_supported, 2) {
            // second bit is HW charge controll support
            if let Some(s) = read_from_sysfs(&battery.name, "charge_types") {
                if let Some((_, s)) = s.split_once('[') {
                    if let Some((s, _)) = s.split_once(']') {
                        battery.charge_threshold_enabled = s == "Long_Life"
                    }
                }
            }
        }
    }

    if update_upower {
        if let Some(device_upower) = device {
            let device = &device_upower.device;
            macro_rules! set_upower_value {
                ($e:ident, $typ:ty, $zero:expr) => {
                    set_upower_value!($e, $e, $typ, $zero, (|x| {x}))
                };
                ($var_name:ident, $upower_name:ident, $typ:ty, $zero:expr) => {
                    set_upower_value!($var_name, $upower_name, $typ, $zero, (|x| {x}))
                };
                ($var_name:ident, $upower_name:ident, $typ:ty, $zero:expr, $math:tt) => {
                    battery.$var_name = battery.$var_name.or(get_upower_value!(device, $upower_name, $typ, $zero, $math))
                };
            }

            set_upower_value!(energy, energy, u32, true, (|x| x * 1000.));

            set_upower_value!(voltage, f32, true);
            set_upower_value!(state, i32, true);
            set_upower_value!(
                power,
                energy_rate,
                f32,
                !battery.power_supply.unwrap_or(false)
            );

            set_upower_value!(time_to_full, u32, true);
            set_upower_value!(time_to_empty, u32, true);

            set_upower_value!(temp, temperature, f32, true);

            battery.icon_name = device.icon_name().await.ok();

            if bit_set(battery.charge_threshold_supported, 0) {
                set_upower_value!(charge_start_threshold, u32, true);
            }
            if bit_set(battery.charge_threshold_supported, 1) {
                set_upower_value!(charge_end_threshold, u32, true);
            }

            if battery.charge_threshold_supported != 0
                && !bit_set(battery.charge_threshold_supported, 2)
            {
                battery.charge_threshold_enabled =
                    device.charge_threshold_enabled().await.unwrap_or(false);
            }

            if device.has_history().await.unwrap_or(false) {
                const TOTAL_SECS: usize = 3600 * 24 * 7;

                battery.history.clear();

                match device
                    .get_history("charge".to_string(), TOTAL_SECS as u32, u32::MAX)
                    .await
                {
                    Ok(data) => {
                        let time_now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("Error getting seconds since epoch: Your clock is probably insanely wrong.")
                            .as_secs();

                        for (time, value, state) in data {
                            let x = if let Some(x) = (time_now as usize).checked_sub(time as usize)
                            {
                                x
                            } else {
                                continue;
                            };
                            if x >= TOTAL_SECS {
                                break;
                            }
                            let x = x as f32;
                            let mut y = value as f32 / 100.;
                            if state == 0 {
                                y = f32::NAN
                            }
                            let d = HistoryPoint { x, y };
                            battery.history.push(d);
                        }
                        battery.history_changed = true;
                    }
                    Err(e) => {
                        log::warn!(
                            "Could not get battery history information for battery {}: {}",
                            &battery.name,
                            e
                        );
                        battery.history_changed = false;
                    }
                }
            } else {
                battery.history_changed = false;
            }
        }
    }

    true
}

impl BatteryCache {
    async fn update_batteries(&mut self) {
        let Some(upower) = &self.upower else {
            log::info!("UPower not available, using sysfs instead");

            let paths = match fs::read_dir(BAT_BASE_DIR) {
                Ok(paths) => paths,
                Err(e) => {
                    log::warn!("Could not read /sys/class/power_supply directory: {e}");
                    return;
                }
            };

            for path in paths.filter_map(|x| x.ok()).map(|x| x.path()) {
                if path.is_dir() {
                    let name = {
                        if let Some(v) = path.file_name() {
                            v.display().to_string()
                        } else {
                            "unknown".into()
                        }
                    };
                    if let Entry::Vacant(e) = self.devices.entry(name) {
                        if let Some(battery) = new_battery(e.key().clone(), &mut None, true).await {
                            e.insert((None, true));
                            self.batteries.push(battery);
                        } else {
                            self.denylist.insert(e.key().clone());
                        }
                    }
                }
            }
            return;
        };

        match upower.enumerate_devices().await {
            Ok(devices_upower) => {
                for device in devices_upower {
                    self.add_battery_upower(device.into()).await;
                }
            }
            Err(e) => {
                log::warn!("Could not get UPower Devices: {e}")
            }
        }
    }

    async fn add_battery_upower(&mut self, objpath: ObjectPath<'static>) {
        let Some(system_bus) = system_bus() else {
            log::error!("Could not get system bus for UPower Device");
            return;
        };
        let device = match DeviceProxy::new(system_bus, &objpath).await {
            Ok(device) => device,
            Err(e) => {
                log::warn!("Could not get device for UPower Device: {e}");
                return;
            }
        };

        let Some(name) = device.native_path().await.ok() else {
            return;
        };
        if self.devices.contains_key(&name) || self.denylist.contains(&name) {
            return;
        }

        let sysfs_exists = Path::new(format!("{}{}", BAT_BASE_DIR, &name).as_str()).exists();

        let stream = device.receive_percentage_changed().await;
        let mut device = Some(UPowerData {
            device,
            stream,
            objpath,
        });

        if let Some(battery) = new_battery(name.clone(), &mut device, sysfs_exists).await {
            self.devices.insert(name, (device, sysfs_exists));
            self.batteries.push(battery);
        } else {
            self.denylist.insert(name);
        }
    }
}

impl magpie_platform::battery::BatteryCache for BatteryCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let rt = async_runtime();

        let (batteries, devices, denylist) = (Vec::new(), HashMap::new(), HashSet::new());
        let last_full_update = Instant::now();

        if let Some(connection) = system_bus() {
            sync!(rt, async {
                let upower = UPowerProxy::new(connection).await.ok();
                let devices_stream = if let Some(ref upower) = upower {
                    if let (Ok(add), Ok(rm)) = (
                        upower.receive_device_added().await,
                        upower.receive_device_removed().await,
                    ) {
                        Some((add, rm))
                    } else {
                        None
                    }
                } else {
                    None
                };
                let mut batterycache = BatteryCache {
                    batteries,
                    upower,
                    devices,
                    denylist,
                    last_full_update,
                    devices_stream,
                };
                batterycache.update_batteries().await;
                batterycache
            })
        } else {
            sync!(rt, async {
                let mut batterycache = BatteryCache {
                    batteries,
                    upower: None,
                    devices,
                    denylist,
                    last_full_update,
                    devices_stream: None,
                };
                batterycache.update_batteries().await;
                batterycache
            })
        }
    }

    fn refresh(&mut self) {
        let rt = async_runtime();
        let update_full = {
            let mut out = DevChange::None;
            if let Some((stream_add, stream_remove)) = self.devices_stream.as_mut() {
                sync!(rt, async {
                    let mut stream_add = std::pin::pin!(stream_add);
                    let mut stream_remove = std::pin::pin!(stream_remove);

                    if let Poll::Ready(Some(r)) = stream_add
                        .as_mut()
                        .poll_next(&mut Context::from_waker(&futures::task::noop_waker()))
                    {
                        if let Ok(deviceadded) = r.args() {
                            out = DevChange::UPowerAdd(deviceadded.device.into_owned())
                        }
                    }

                    if let Poll::Ready(Some(r)) = stream_remove
                        .as_mut()
                        .poll_next(&mut Context::from_waker(&futures::task::noop_waker()))
                    {
                        if let Ok(deviceremoved) = r.args() {
                            out = DevChange::UPowerRemove(deviceremoved.device.into_owned())
                        }
                    }
                })
            } else {
                if Instant::now()
                    .duration_since(self.last_full_update)
                    .as_secs()
                    >= 30
                {
                    out = DevChange::Sysfs
                }
            }
            out
        };
        match update_full {
            DevChange::None => (),
            DevChange::Sysfs => sync!(rt, async {
                self.update_batteries().await;
                self.last_full_update = Instant::now();
            }),
            DevChange::UPowerAdd(dev) => sync!(rt, async { self.add_battery_upower(dev).await }),
            DevChange::UPowerRemove(dev) => sync!(rt, async {
                self.devices.retain(|name, upowerstuff| {
                    if let Some(upowerstuff) = &upowerstuff.0 {
                        if upowerstuff.objpath == dev {
                            self.batteries.retain(|b| &b.name != name);
                            return false;
                        }
                    }
                    true
                })
            }),
        }
        self.batteries.retain_mut(|battery: &mut Battery| {
            sync!(rt, async {
                let (device, sysfs_exists) = self.devices.get_mut(&battery.name).unwrap();
                if !update_battery(battery, device, *sysfs_exists, false).await {
                    self.devices.remove(&battery.name); // remove entry in case of dbus, but don't blacklist in case of reconnect
                    false
                } else {
                    true
                }
            })
        });
    }

    fn cached_entries(&self) -> &[Battery] {
        &self.batteries
    }
}

enum DevChange<'a> {
    None,
    Sysfs,
    UPowerAdd(ObjectPath<'a>),
    UPowerRemove(ObjectPath<'a>),
}
