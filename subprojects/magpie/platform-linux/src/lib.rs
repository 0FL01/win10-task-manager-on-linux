/* src/apps.rs
 *
 * Copyright 2025 Mission Center Developers
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

use util::*;

mod about;
mod apps;
mod battery;
mod config_dir;
mod cpu;
mod disks;
mod fan;
mod gpus;
mod hardware;
mod memory;
mod network;
mod processes;
mod services;
pub mod smbios;

mod util;

pub struct DataCacheTypes;

impl magpie_platform::DataCacheTypes for DataCacheTypes {
    type AboutCache = about::AboutCache;
    type AppCache = apps::AppCache;
    type BatteryCache = battery::BatteryCache;
    type CpuCache = cpu::CpuCache;
    type DisksCache = disks::DisksCache;
    type DisksManager = disks::DisksManager;
    type FanCache = fan::FanCache;
    type GpuCache = gpus::GpuCache;
    type MemoryCache = memory::MemoryCache;
    type NetworkCache = network::NetworkCache;
    type ProcessCache = processes::ProcessCache;
    type ProcessManager = processes::ProcessManager;
    type ServiceCache = services::ServiceCache;
    type ServicesManager = services::ServiceManager;
}
