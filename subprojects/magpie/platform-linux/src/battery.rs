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

use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::task::{Context, Poll};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use futures::stream::Stream;

use zbus::proxy::PropertyStream;
use zbus::zvariant::ObjectPath;

use upower_dbus::{BatteryType, DeviceAddedStream, DeviceProxy, DeviceRemovedStream, UPowerProxy};

mod hidpp;

const BAT_BASE_DIR: &str = "/sys/class/power_supply/";

struct UPowerData {
    device: DeviceProxy<'static>,
    stream: PropertyStream<'static, f64>,
    objpath: ObjectPath<'static>,
}

type Devices = HashMap<String, (Option<UPowerData>, bool)>;

pub struct BatteryCache {
    batteries: Vec<Battery>,
    hidpp: hidpp::Collector,
    published: Vec<Battery>,

    upower: Option<UPowerProxy<'static>>,
    devices: Devices,

    denylist: HashSet<String>,

    last_full_update: Instant,
    devices_stream: Option<(DeviceAddedStream, DeviceRemovedStream)>,
}

fn read_from_sysfs(root: &Path, name: &str, appendix: &str) -> Option<String> {
    let path = root.join(name).join(appendix);
    match read_to_string(path) {
        Ok(c) => Some(c.trim().to_string()),
        Err(e) => {
            log::info!(
                "Could not read file: `{}`: {}",
                root.join(name).join(appendix).display(),
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
    ($root:ident, $name:ident, $file_name:expr, $typ:ty, $zero:expr, $math:tt) => {
        read_from_sysfs($root, &$name, $file_name)
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
    root: &Path,
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
        if let Some(v) = read_from_sysfs(root, &name, "type") {
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
                $e = get_sysfs_value!(root, name, stringify!($e), $typ, true, $math);
            };
            ($e:ident, $i:literal, $typ:ty, $math:tt) => {
                $e = get_sysfs_value!(root, name, $i, $typ, true, $math);
            };
        }

        let div_k = |x| x / 1000;
        let div_m = |x| x / 1_000_000.;

        vendor = read_from_sysfs(root, &name, "manufacturer");
        model = read_from_sysfs(root, &name, "model_name");
        serial = read_from_sysfs(root, &name, "serial_number");

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

        technology = match read_from_sysfs(root, &name, "technology") {
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
        (sysfs_power_supply(root, &name), 0)
    };
    if device_upower.is_none() && power_supply == Some(false) {
        // Device batteries are not laptop batteries. Only classify a game controller when
        // the associated input device actually exposes joystick/gamepad buttons.
        kind = sysfs_device_kind(&root.join(&name));
    }
    let design_capacity =
        if power_supply != Some(false) && kind == Some(BatteryType::Battery as i32) {
            crate::smbios::design_capacity_mwh()
        } else {
            None
        };
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

    if !update_battery(root, &mut battery, device_upower, sysfs_exists, true).await {
        return None;
    }

    Some(battery)
}

async fn update_battery(
    root: &Path,
    battery: &mut Battery,
    device: &mut Option<UPowerData>,
    sysfs_exists: bool,
    mut update_upower: bool,
) -> bool {
    if sysfs_exists
        && device.is_none()
        && (!root.join(&battery.name).is_dir()
            || read_from_sysfs(root, &battery.name, "present").as_deref() == Some("0"))
    {
        return false;
    }
    battery.percentage = {
        let parsed: std::option::Option<f32> = read_from_sysfs(root, &battery.name, "capacity")
            .and_then(parse_str)
            .filter(|value: &f32| value.is_finite() && (0. ..=100.).contains(value));
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
        } else if device.is_none() {
            // Do not retain stale sysfs-only readings or turn an unknown value into 0%.
            return false;
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
                set_sysfs_value!($e, stringify!($e), $typ, $zero, $math);
            };
            ($e:ident, $i:expr, $typ:ty, $zero:expr, $math:tt) => {
                battery.$e = get_sysfs_value!(root, name, $i, $typ, $zero, $math);
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

        battery.state = read_from_sysfs(root, &battery.name, "status").and_then(|v| match v.as_str() {
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
            if let Some(s) = read_from_sysfs(root, &battery.name, "charge_types") {
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

fn sysfs_power_supply(root: &Path, name: &str) -> Option<bool> {
    match read_from_sysfs(root, name, "scope").as_deref() {
        Some("Device") => Some(false),
        Some("System") => Some(true),
        _ => None,
    }
}

fn sysfs_device_kind(path: &Path) -> Option<i32> {
    let device = fs::canonicalize(path)
        .ok()?
        .parent()?
        .parent()?
        .join("input");
    for input in fs::read_dir(device).ok()?.flatten() {
        let Ok(keys) = read_to_string(input.path().join("capabilities/key")) else {
            continue;
        };
        // Linux sysfs bitmaps are machine-word groups, printed most significant first.
        let words: Vec<_> = keys.split_whitespace().rev().collect();
        if (0x120..0x140).any(|bit| {
            words
                .get(bit / usize::BITS as usize)
                .and_then(|word| usize::from_str_radix(word, 16).ok())
                .is_some_and(|word| word & (1 << (bit % usize::BITS as usize)) != 0)
        }) {
            return Some(BatteryType::GamingInput as i32);
        }
    }
    None
}

fn native_name(root: &Path, native: &str) -> String {
    let path = Path::new(native);
    if path.is_absolute() {
        if let Some(name) = path.file_name() {
            let candidate = root.join(name);
            if fs::canonicalize(&candidate)
                .ok()
                .zip(fs::canonicalize(path).ok())
                .is_some_and(|(a, b)| a == b)
            {
                return name.to_string_lossy().into_owned();
            }
        }
    }
    native.to_owned()
}

/// The kernel may expose HID batteries that an otherwise functional UPower omits.
/// Reconcile every battery refresh: no UPower signal is generated for these devices.
async fn supplement_sysfs(root: &Path, devices: &mut Devices, batteries: &mut Vec<Battery>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("Could not read {} directory: {e}", root.display());
            return;
        }
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !devices.contains_key(name)
            && path.is_dir()
            && read_from_sysfs(root, name, "type").as_deref() == Some("Battery")
        {
            if let Some(battery) = new_battery(root, name.to_owned(), &mut None, true).await {
                devices.insert(name.to_owned(), (None, true));
                batteries.push(battery);
            }
        }
    }
}

async fn refresh_entries(root: &Path, devices: &mut Devices, batteries: &mut Vec<Battery>) {
    let mut index = 0;
    while index < batteries.len() {
        let battery = &mut batteries[index];
        let keep = if let Some((device, sysfs_exists)) = devices.get_mut(&battery.name) {
            update_battery(root, battery, device, *sysfs_exists, false).await
        } else {
            false
        };
        if keep {
            index += 1;
        } else {
            devices.remove(&batteries.remove(index).name);
        }
    }
}

impl BatteryCache {
    async fn update_batteries(&mut self) {
        if let Some(upower) = &self.upower {
            match upower.enumerate_devices().await {
                Ok(devices_upower) => {
                    for device in devices_upower {
                        self.add_battery_upower(device.into()).await;
                    }
                }
                Err(e) => log::warn!("Could not get UPower Devices: {e}"),
            }
        }
        supplement_sysfs(
            Path::new(BAT_BASE_DIR),
            &mut self.devices,
            &mut self.batteries,
        )
        .await;
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
        let name = native_name(Path::new(BAT_BASE_DIR), &name);
        if self
            .devices
            .get(&name)
            .is_some_and(|(device, _)| device.is_some())
            || self.denylist.contains(&name)
        {
            return;
        }

        let sysfs_exists = Path::new(format!("{}{}", BAT_BASE_DIR, &name).as_str()).exists();

        let stream = device.receive_percentage_changed().await;
        let mut device = Some(UPowerData {
            device,
            stream,
            objpath,
        });

        if let Some(battery) = new_battery(
            Path::new(BAT_BASE_DIR),
            name.clone(),
            &mut device,
            sysfs_exists,
        )
        .await
        {
            // UPower may discover a previously sysfs-only device later. Upgrade its data
            // source instead of keeping two copies or discarding the fallback prematurely.
            self.batteries.retain(|entry| entry.name != name);
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
                    hidpp: hidpp::Collector::new(),
                    published: Vec::new(),
                    upower,
                    devices,
                    denylist,
                    last_full_update,
                    devices_stream,
                };
                batterycache.update_batteries().await;
                batterycache
                    .hidpp
                    .merge(&batterycache.batteries, &mut batterycache.published);
                batterycache
            })
        } else {
            sync!(rt, async {
                let mut batterycache = BatteryCache {
                    batteries,
                    hidpp: hidpp::Collector::new(),
                    published: Vec::new(),
                    upower: None,
                    devices,
                    denylist,
                    last_full_update,
                    devices_stream: None,
                };
                batterycache.update_batteries().await;
                batterycache
                    .hidpp
                    .merge(&batterycache.batteries, &mut batterycache.published);
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
            }
            out
        };
        match update_full {
            DevChange::None => (),
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
        sync!(rt, async {
            if self.devices_stream.is_none() && self.last_full_update.elapsed().as_secs() >= 30 {
                self.update_batteries().await;
                self.last_full_update = Instant::now();
            }
            supplement_sysfs(
                Path::new(BAT_BASE_DIR),
                &mut self.devices,
                &mut self.batteries,
            )
            .await;
            refresh_entries(
                Path::new(BAT_BASE_DIR),
                &mut self.devices,
                &mut self.batteries,
            )
            .await;
        });
        self.hidpp.merge(&self.batteries, &mut self.published);
    }

    fn cached_entries(&self) -> &[Battery] {
        &self.published
    }
}

enum DevChange<'a> {
    None,
    UPowerAdd(ObjectPath<'a>),
    UPowerRemove(ObjectPath<'a>),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "magpie-battery-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn battery(&self, name: &str, value: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).unwrap();
            for (key, value) in [
                ("type", "Battery"),
                ("scope", "Device"),
                ("present", "1"),
                ("capacity", value),
                ("status", "Discharging"),
                ("model_name", "Example Wireless Controller"),
            ] {
                fs::write(path.join(key), value).unwrap();
            }
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sysfs_supplements_existing_inventory_updates_and_removes_disconnected_devices() {
        let fixture = Fixture::new();
        let path = fixture.battery("hid-controller-battery", "52");
        let mut devices = Devices::new();
        let mut batteries = vec![Battery {
            name: "upower-only-device".into(),
            ..Default::default()
        }];
        let rt = async_runtime();
        sync!(rt, async {
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            assert_eq!(batteries.len(), 2);
            let battery = &batteries[1];
            assert_eq!(battery.percentage, 0.52);
            assert_eq!(battery.state, Some(2));
            assert_eq!(battery.power_supply, Some(false));
            assert_eq!(battery.energy_full_design, None); // Not the laptop's SMBIOS battery.
            assert_ne!(battery.kind, Some(BatteryType::Battery as i32));
            // An existing entry with this exact native name (including a UPower-backed
            // one) wins; supplementing does not replace its metadata or duplicate it.
            batteries[1].vendor = Some("existing source".into());
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            assert_eq!(batteries.len(), 2);
            assert_eq!(batteries[1].vendor.as_deref(), Some("existing source"));
            fs::write(path.join("capacity"), "0").unwrap();
            fs::write(path.join("status"), "Charging").unwrap();
            assert!(update_battery(&fixture.0, &mut batteries[1], &mut None, true, false).await);
            assert_eq!(batteries[1].percentage, 0.);
            assert_eq!(batteries[1].state, Some(1));
            batteries.remove(0); // The discovery-only sentinel has no D-Bus proxy in this fixture.
            fs::remove_dir_all(&path).unwrap();
            refresh_entries(&fixture.0, &mut devices, &mut batteries).await;
            assert!(batteries.is_empty());
            assert!(devices.is_empty());
            fixture.battery("hid-controller-battery", "87");
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            assert_eq!(batteries.len(), 1);
            assert_eq!(batteries[0].percentage, 0.87);
        });
    }

    #[test]
    fn invalid_absent_and_temporarily_unreadable_sysfs_batteries_are_not_stale_or_zero() {
        let fixture = Fixture::new();
        let path = fixture.battery("hid-controller", "101");
        let mut devices = Devices::new();
        let mut batteries = Vec::new();
        let rt = async_runtime();
        sync!(rt, async {
            let mains = fixture.0.join("AC");
            fs::create_dir(&mains).unwrap();
            fs::write(mains.join("type"), "Mains").unwrap();
            fs::write(mains.join("capacity"), "100").unwrap();
            for value in ["101", "-1", "NaN", "inf", "Unknown"] {
                fs::write(path.join("capacity"), value).unwrap();
                supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
                assert!(batteries.is_empty());
                fs::write(path.join("capacity"), "50").unwrap();
                supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
                assert_eq!(batteries[0].percentage, 0.5);
                fs::write(path.join("capacity"), value).unwrap();
                refresh_entries(&fixture.0, &mut devices, &mut batteries).await;
                assert!(batteries.is_empty());
                assert!(devices.is_empty());
            }
            fs::write(path.join("capacity"), "100").unwrap();
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            assert_eq!(batteries[0].percentage, 1.);
            fs::write(path.join("present"), "0").unwrap();
            refresh_entries(&fixture.0, &mut devices, &mut batteries).await;
            assert!(batteries.is_empty());
            fs::write(path.join("present"), "1").unwrap();
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            fs::remove_file(path.join("capacity")).unwrap();
            refresh_entries(&fixture.0, &mut devices, &mut batteries).await;
            assert!(batteries.is_empty());
            fs::write(path.join("capacity"), "75").unwrap();
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            assert_eq!(batteries[0].percentage, 0.75);
            fs::write(path.join("energy_full"), "2400000").unwrap();
            fs::write(path.join("voltage_now"), "3000000").unwrap();
            devices.clear();
            batteries.clear();
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            assert_eq!(batteries.len(), 1); // Never add the mains adapter.
            assert_eq!(batteries[0].energy_full, Some(2400));
            assert_eq!(batteries[0].voltage, Some(3.));
        });
    }

    #[test]
    fn exact_native_path_deduplication_and_input_capabilities_not_model_guesses() {
        let fixture = Fixture::new();
        let root = fixture.0.join("class");
        fs::create_dir(&root).unwrap();
        let controller = fixture.0.join("device/power_supply/hid-controller");
        fs::create_dir_all(&controller).unwrap();
        let keys = fixture.0.join("device/input/input1/capabilities");
        fs::create_dir_all(&keys).unwrap();
        let mut words = vec![0_usize; 0x130 / usize::BITS as usize + 1];
        words[0x130 / usize::BITS as usize] = 1 << (0x130 % usize::BITS as usize);
        let bitmap = words
            .iter()
            .rev()
            .map(|word| format!("{word:x}"))
            .collect::<Vec<_>>()
            .join(" ");
        fs::write(keys.join("key"), bitmap).unwrap();
        let alias = root.join("hid-controller");
        std::os::unix::fs::symlink(&controller, &alias).unwrap();
        assert_eq!(
            sysfs_device_kind(&alias),
            Some(BatteryType::GamingInput as i32)
        );
        fs::write(keys.join("key"), "0").unwrap();
        assert_eq!(sysfs_device_kind(&alias), None);
        assert_eq!(
            native_name(&root, controller.to_str().unwrap()),
            "hid-controller"
        );
        assert_eq!(native_name(&root, "hid-controller"), "hid-controller");
        let unrelated = fixture.0.join("other/hid-controller");
        fs::create_dir_all(&unrelated).unwrap();
        assert_eq!(
            native_name(&root, unrelated.to_str().unwrap()),
            unrelated.to_str().unwrap()
        );
        assert_eq!(
            native_name(&root, "/other/hid-controller"),
            "/other/hid-controller"
        );
    }

    #[test]
    fn same_model_different_native_paths_are_distinct_devices() {
        let fixture = Fixture::new();
        fixture.battery("hid-controller-one", "25");
        fixture.battery("hid-controller-two", "75");
        let mut devices = Devices::new();
        let mut batteries = Vec::new();
        let rt = async_runtime();
        sync!(rt, async {
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
            supplement_sysfs(&fixture.0, &mut devices, &mut batteries).await;
        });
        assert_eq!(batteries.len(), 2);
        assert_eq!(batteries[0].model, batteries[1].model);
        assert_eq!(batteries[0].percentage, 0.25);
        assert_eq!(batteries[1].percentage, 0.75);
    }
}
