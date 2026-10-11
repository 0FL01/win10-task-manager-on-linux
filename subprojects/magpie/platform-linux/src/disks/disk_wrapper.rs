/* src/disks/disk_wrapper.rs
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

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use arrayvec::ArrayString;
use glob::glob;
use tokio::join;
use udisks2::block::BlockProxy;
use udisks2::drive::{DriveProxy, RotationRate};
use udisks2::filesystem::FilesystemProxy;
use udisks2::partition::PartitionProxy;
use udisks2::partitiontable::PartitionTableProxy;
use udisks2::{Client, Object};
use uucore::fsext::{statfs, FsUsage, MountInfo};
use zbus::proxy::PropertyStream;
use zbus::zvariant::OwnedObjectPath;

use magpie_platform::disks::{Disk, PartitionInfo};

use crate::disks::stats::Stats;
use crate::disks::util;
use crate::util::{read_i64, read_u64, stream_has_contents};

#[macro_export]
macro_rules! poll_wrapper {
    ($base_obj_option: expr, $stream_option: expr, $cached_function_call: ident, $function_call: ident) => {
        if let Some(base_obj) = $base_obj_option.as_ref() {
            if let Some(ref mut stream) = $stream_option {
                match stream_has_contents(stream).await {
                    Some(Ok(value)) => Some(value),
                    Some(Err(_)) => base_obj.$function_call().await.ok(),
                    None => match base_obj.$cached_function_call() {
                        Ok(value) => value,
                        Err(_) => base_obj.$function_call().await.ok(),
                    },
                }
            } else {
                base_obj.$function_call().await.ok()
            }
        } else {
            None
        }
    };
}

/// Just contains all the PropertyStream's to declutter the DiskWrapper struct
#[derive(Default)]
pub struct StreamListener {
    pub ata_temp_change_stream: Option<PropertyStream<'static, f64>>,

    pub partition_table_change_stream: Option<PropertyStream<'static, Vec<OwnedObjectPath>>>,

    pub capacity_change_stream: Option<PropertyStream<'static, u64>>,
    pub drive_wwn_change_stream: Option<PropertyStream<'static, String>>,
    pub nvme_wwn_change_stream: Option<PropertyStream<'static, String>>,
    pub drive_change_stream: Option<PropertyStream<'static, OwnedObjectPath>>,
    pub ejectable_change_stream: Option<PropertyStream<'static, bool>>,
    pub size_change_stream: Option<PropertyStream<'static, u64>>,
    pub rotation_rate_change_stream: Option<PropertyStream<'static, RotationRate>>,
    pub serial_change_stream: Option<PropertyStream<'static, String>>,
    pub model_change_stream: Option<PropertyStream<'static, String>>,
    pub vendor_change_stream: Option<PropertyStream<'static, String>>,
}

impl StreamListener {
    /// Joins streams from another StreamListener into this one only if the fields are None
    /// Returns itself for daisy-chaining Java style
    pub fn join(&mut self, other: StreamListener) -> &mut StreamListener {
        if self.ata_temp_change_stream.is_none() {
            self.ata_temp_change_stream = other.ata_temp_change_stream;
        }

        if self.partition_table_change_stream.is_none() {
            self.partition_table_change_stream = other.partition_table_change_stream;
        }

        if self.capacity_change_stream.is_none() {
            self.capacity_change_stream = other.capacity_change_stream;
        }

        if self.drive_wwn_change_stream.is_none() {
            self.drive_wwn_change_stream = other.drive_wwn_change_stream;
        }

        if self.nvme_wwn_change_stream.is_none() {
            self.nvme_wwn_change_stream = other.nvme_wwn_change_stream;
        }

        if self.drive_change_stream.is_none() {
            self.drive_change_stream = other.drive_change_stream;
        }

        if self.ejectable_change_stream.is_none() {
            self.ejectable_change_stream = other.ejectable_change_stream;
        }

        if self.size_change_stream.is_none() {
            self.size_change_stream = other.size_change_stream;
        }

        if self.rotation_rate_change_stream.is_none() {
            self.rotation_rate_change_stream = other.rotation_rate_change_stream;
        }

        if self.serial_change_stream.is_none() {
            self.serial_change_stream = other.serial_change_stream;
        }

        if self.model_change_stream.is_none() {
            self.model_change_stream = other.model_change_stream;
        }

        if self.vendor_change_stream.is_none() {
            self.vendor_change_stream = other.vendor_change_stream;
        }

        self
    }
}

#[derive(Default)]
pub struct PartitionGrouper {
    pub devname: String,
    pub encrypted_devname: Option<String>,
    pub partition_object: Option<PartitionProxy<'static>>,
    pub partition_size_stream: Option<PropertyStream<'static, u64>>,
    pub filesystem_object: Option<FilesystemProxy<'static>>,
    pub filesystem_size_stream: Option<PropertyStream<'static, u64>>,
    pub block_object: Option<BlockProxy<'static>>,
    pub block_size_stream: Option<PropertyStream<'static, u64>>,
    // encrypted "partitions" backed by virtual devs should not be counted
    pub count_to_formatted: bool,
}

impl PartitionGrouper {
    pub fn update_partitions(&mut self, mounts_info: &[MountInfo]) -> Option<PartitionInfo> {
        let mount_info = mounts_info.first()?;

        let stat_path = if mount_info.mount_dir.is_empty() {
            mount_info.dev_name.clone().into()
        } else {
            mount_info.mount_dir.clone()
        };

        let usage = statfs(&stat_path).map(FsUsage::new).ok();

        // big thanks to https://github.com/uutils/coreutils/blob/6e422b728e902c263107cec5af31d70eba991ac8/src/uu/df/src/table.rs#L159
        let (size, used) = usage
            .map(|stat| {
                (
                    stat.blocks * stat.blocksize,
                    stat.blocksize * stat.blocks.saturating_sub(stat.bfree),
                )
            })
            .unzip();

        let mut mountpoints = mounts_info
            .iter()
            .map(|mi| mi.mount_dir.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        // shortest mountpoint is probably the most important. this is probably cheap for any reasonable disk setup.
        mountpoints.sort_by_key(String::len);

        Some(PartitionInfo {
            devname: self.devname.clone(),
            filesystem: Some(mount_info.fs_type.clone()),
            mountpoints,
            size,
            used,
        })
    }
}

#[derive(Default)]
pub struct DiskWrapper {
    pub disk_id: String,
    pub root_object: Option<Object>,
    pub partition_table: Option<PartitionTableProxy<'static>>,
    pub block_proxy: Option<BlockProxy<'static>>,
    pub drive_proxy: Option<DriveProxy<'static>>,
    pub drive_ata: Option<udisks2::ata::AtaProxy<'static>>,
    pub nvme_controller: Option<udisks2::nvme::controller::ControllerProxy<'static>>,
    pub nvme_namespace: Option<udisks2::nvme::namespace::NamespaceProxy<'static>>,
    pub partition_groups: Vec<PartitionGrouper>,
    pub is_system: bool,
    stream_listeners: StreamListener,

    pub disk: Disk,
    pub stats: Stats,
}

impl DiskWrapper {
    pub async fn new(disk_id: &str, udisks2: &Client, object: Object) -> Self {
        let mut wrapper = Self {
            disk_id: disk_id.to_string(),
            root_object: Some(object),
            partition_table: None,
            block_proxy: None,
            drive_proxy: None,
            drive_ata: None,
            nvme_controller: None,
            nvme_namespace: None,
            partition_groups: Vec::new(),
            is_system: false,
            stream_listeners: Default::default(),

            disk: Default::default(),
            stats: Default::default(),
        };

        // Initialize the wrapper with data from udisks2
        wrapper.initialize_from_udisks(udisks2).await;

        wrapper
    }

    async fn initialize_from_udisks(&mut self, udisks2: &Client) {
        if let Some(object) = self.root_object.as_ref() {
            // Get block proxy
            self.block_proxy = util::block(object, &self.disk_id).await;

            // Get drive proxy and ata data
            if let Some(block_proxy) = self.block_proxy.as_ref() {
                if let Some(drive_obj) = util::drive(udisks2, block_proxy, &self.disk_id).await {
                    let (drive_proxy, drive_ata, nvme_controller) = join!(
                        drive_obj.drive(),
                        drive_obj.drive_ata(),
                        drive_obj.nvme_controller()
                    );

                    if let Ok(drive_proxy) = drive_proxy {
                        self.drive_proxy = Some(drive_proxy);
                    }

                    if let Ok(drive_ata) = drive_ata {
                        self.drive_ata = Some(drive_ata);
                    }

                    // Get nvme controller if available
                    if let Ok(nvme_controller) = nvme_controller {
                        self.nvme_controller = Some(nvme_controller);
                    }
                }
            }

            let (nvme_namespace, partition_table) =
                join!(object.nvme_namespace(), object.partition_table());

            self.nvme_namespace = nvme_namespace.ok();
            self.partition_table = partition_table.ok();

            // Determine if this is a system disk
            self.is_system = false; //util::is_system(&self.disk_id, rt, &self.filesystems);
        }

        if let Some(partition_groups) = self.load_partition_groups(udisks2).await {
            self.partition_groups = partition_groups;

            'outer: for fs in self
                .partition_groups
                .iter()
                .filter_map(|group| group.filesystem_object.as_ref())
            {
                let Ok(mount_points) = fs.mount_points().await else {
                    continue;
                };

                for mount_point in mount_points {
                    let Ok(pt) = String::from_utf8(mount_point) else {
                        continue;
                    };

                    if pt == "/\0" {
                        self.is_system = true;
                        break 'outer;
                    }
                }
            }
        }

        let (atas, nvmes, drives, partitions, blocks) = join!(
            self.initialize_ata_listeners(),
            self.initialize_nvme_listeners(),
            self.initialize_drive_listeners(),
            self.initialize_partition_listeners(),
            self.initialize_block_proxy_listeners()
        );

        self.stream_listeners = atas;

        self.stream_listeners
            .join(nvmes)
            .join(drives)
            .join(partitions)
            .join(blocks);

        self.disk = self.create_disk_obj().await;
        self.stats = self.create_stats();
    }

    async fn get_filesystem_for_partition(
        partition: &Object,
        udisks2: &Client,
    ) -> Option<FilesystemProxy<'static>> {
        let mut object = partition.clone();

        if let Ok(encrypted) = object.encrypted().await {
            if let Ok(cleartext) = encrypted.cleartext_device().await {
                let Ok(obj) = udisks2.object(cleartext);

                object = obj;
            }
        }

        object.filesystem().await.ok()
    }

    async fn load_partition_groups(&mut self, udisks2: &Client) -> Option<Vec<PartitionGrouper>> {
        // already inited and no cheap way to look for changes, so we just wont.
        if !self.partition_groups.is_empty()
            && self
                .stream_listeners
                .partition_table_change_stream
                .is_none()
        {
            return None;
        }

        // do NOT use the nice macro. we DONT want a new value if there isnt a new value!

        if let Some(partition_table) = self.partition_table.as_ref() {
            if let Some(ref mut partition_listener) =
                self.stream_listeners.partition_table_change_stream
            {
                match stream_has_contents(partition_listener).await {
                    None => {
                        // no update, just quietly move on
                        None
                    }
                    Some(partition_result) => match partition_result {
                        Ok(mut partitions) => Some(
                            Self::extract_partition_groups(udisks2, &mut partitions, true).await,
                        ),
                        Err(_) => None,
                    },
                }
            } else if let Ok(mut partitions) = partition_table.partitions().await {
                Some(Self::extract_partition_groups(udisks2, &mut partitions, true).await)
            } else {
                None
            }
        } else {
            None
        }
    }

    async fn get_encrypted_block(obj: &Object, udisks2: &Client) -> Option<Object> {
        let enc = obj.encrypted().await.ok()?;

        let clear = enc.cleartext_device().await.ok()?;

        udisks2.object(clear).ok()
    }

    async fn extract_partition_groups_recursive(
        udisks2: &Client,
        partitions: &mut Vec<OwnedObjectPath>,
        count_to_formatted: bool,
        depth: usize,
    ) -> Vec<PartitionGrouper> {
        // random number, shouldnt happen
        if depth > 10 {
            log::error!("Maximum partition update depth exceeded");
            return Default::default();
        }

        let mut out = Vec::new();

        for partition in partitions.drain(..) {
            let partition_name = partition.to_string();

            let Some(obj) = udisks2.object(partition).ok() else {
                log::warn!("Failed to get partition object for {}", partition_name);
                continue;
            };

            let block_obj = obj.block().await.ok();
            let (block_resize, devname) = if let Some(block_obj) = block_obj.as_ref() {
                (
                    Some(block_obj.receive_size_changed().await),
                    Self::get_block_devname(block_obj).await,
                )
            } else {
                Default::default()
            };

            let enc_obj = Self::get_encrypted_block(&obj, udisks2).await;

            let enc_block = if let Some(obj) = enc_obj {
                obj.block().await.ok()
            } else {
                None
            };

            let encrypted_devname = if let Some(encrypted_block) = enc_block {
                Some(Self::get_block_devname(&encrypted_block).await)
            } else {
                None
            };

            let filesystem_obj = Self::get_filesystem_for_partition(&obj, udisks2).await;
            let filesystem_resize = if let Some(filesystem) = filesystem_obj.as_ref() {
                Some(filesystem.receive_size_changed().await)
            } else {
                None
            };

            let partition_obj = obj.partition().await.ok();
            let partition_resize = if let Some(partition) = partition_obj.as_ref() {
                Some(partition.receive_size_changed().await)
            } else {
                None
            };

            out.push(PartitionGrouper {
                devname: devname.to_string(),
                encrypted_devname: encrypted_devname.clone(),
                partition_object: partition_obj,
                partition_size_stream: partition_resize,
                block_object: block_obj,
                block_size_stream: block_resize,
                filesystem_object: filesystem_obj,
                filesystem_size_stream: filesystem_resize,
                count_to_formatted,
            });

            let Some(enc_dev) = encrypted_devname else {
                continue;
            };

            let Some(enc_dev) = Path::new(&enc_dev).file_name() else {
                continue;
            };

            // if we have an encrypted drive, we need to find if the virtual dev is holding any orphan devs
            let p = Path::new("/sys/block").join(enc_dev).join("holders");

            let Ok(holders) = p.read_dir() else {
                log::warn!(
                    "{} not found when recursing {}",
                    enc_dev.to_string_lossy(),
                    devname
                );
                continue;
            };

            let mut holders: Vec<OwnedObjectPath> = holders
                .filter_map(Result::ok)
                .map(|v| v.file_name().to_string_lossy().replace("-", "_2d"))
                .map(|p| format!("/org/freedesktop/UDisks2/block_devices/{p}"))
                .filter_map(|p| OwnedObjectPath::try_from(p).ok())
                .collect();

            out.extend(
                Box::pin(Self::extract_partition_groups_recursive(
                    udisks2,
                    &mut holders,
                    false,
                    depth + 1,
                ))
                .await,
            );
        }

        out
    }

    #[inline]
    async fn extract_partition_groups(
        udisks2: &Client,
        partitions: &mut Vec<OwnedObjectPath>,
        count_to_formatted: bool,
    ) -> Vec<PartitionGrouper> {
        Self::extract_partition_groups_recursive(udisks2, partitions, count_to_formatted, 0).await
    }

    async fn get_block_devname(encrypted_block: &BlockProxy<'_>) -> String {
        encrypted_block
            .device()
            .await
            .ok()
            .and_then(|v| String::from_utf8(v).ok())
            .unwrap_or_default()
            .trim_matches(char::from(0))
            .to_string()
    }

    async fn initialize_block_proxy_listeners(&self) -> StreamListener {
        let mut out = StreamListener::default();

        if let Some(block_proxy) = self.block_proxy.as_ref() {
            let (capacity_change_stream, drive_change_stream) = join!(
                block_proxy.receive_size_changed(),
                block_proxy.receive_drive_changed()
            );

            out.capacity_change_stream = Some(capacity_change_stream);
            out.drive_change_stream = Some(drive_change_stream);
        }

        out
    }

    async fn initialize_drive_listeners(&self) -> StreamListener {
        let mut out = StreamListener::default();

        if let Some(drive) = self.drive_proxy.as_ref() {
            let (
                drive_wwn_change_stream,
                ejectable_change_stream,
                size_change_stream,
                rotation_rate_change_stream,
                serial_change_stream,
                model_change_stream,
                vendor_change_stream,
            ) = join!(
                drive.receive_wwn_changed(),
                drive.receive_ejectable_changed(),
                drive.receive_size_changed(),
                drive.receive_rotation_rate_changed(),
                drive.receive_serial_changed(),
                drive.receive_model_changed(),
                drive.receive_vendor_changed()
            );

            out.drive_wwn_change_stream = Some(drive_wwn_change_stream);
            out.ejectable_change_stream = Some(ejectable_change_stream);
            out.size_change_stream = Some(size_change_stream);
            out.rotation_rate_change_stream = Some(rotation_rate_change_stream);
            out.serial_change_stream = Some(serial_change_stream);
            out.model_change_stream = Some(model_change_stream);
            out.vendor_change_stream = Some(vendor_change_stream);
        }

        out
    }

    async fn initialize_nvme_listeners(&self) -> StreamListener {
        let mut out = StreamListener::default();

        if let Some(nvme_namespace) = self.nvme_namespace.as_ref() {
            out.nvme_wwn_change_stream = Some(nvme_namespace.receive_wwn_changed().await);
        }

        out
    }

    async fn initialize_partition_listeners(&self) -> StreamListener {
        let mut out = StreamListener::default();

        if let Some(partition_table) = self.partition_table.as_ref() {
            out.partition_table_change_stream =
                Some(partition_table.receive_partitions_changed().await);
        }

        out
    }

    async fn initialize_ata_listeners(&self) -> StreamListener {
        let mut out = StreamListener::default();

        if let Some(drive_ata) = self.drive_ata.as_ref() {
            out.ata_temp_change_stream = Some(drive_ata.receive_smart_temperature_changed().await);
        }

        out
    }

    pub async fn get_wwn(&mut self) -> Option<String> {
        if let Some(wwn) = poll_wrapper!(
            self.nvme_namespace,
            self.stream_listeners.nvme_wwn_change_stream,
            cached_wwn,
            wwn
        ) {
            return Some(wwn);
        }

        poll_wrapper!(
            self.drive_proxy,
            self.stream_listeners.drive_wwn_change_stream,
            cached_wwn,
            wwn
        )
    }

    pub async fn get_serial(&mut self) -> Option<String> {
        poll_wrapper!(
            self.drive_proxy,
            self.stream_listeners.serial_change_stream,
            cached_serial,
            serial
        )
    }

    fn map_rotation_rate(rate: Option<RotationRate>) -> Option<u64> {
        rate.and_then(|r| match r {
            RotationRate::Unknown => None,
            // a special case where Rotation is known, but zero
            RotationRate::NonRotating => None,
            RotationRate::Rotating(rate) => Some(rate as u64),
        })
    }

    pub async fn get_rotation_rate(&mut self) -> Option<u64> {
        Self::map_rotation_rate(poll_wrapper!(
            self.drive_proxy,
            self.stream_listeners.rotation_rate_change_stream,
            cached_rotation_rate,
            rotation_rate
        ))
    }

    pub async fn get_formatted_bytes(&mut self) -> Option<u64> {
        let mut out = 0;

        for partition_group in self
            .partition_groups
            .iter_mut()
            .filter(|p| p.count_to_formatted)
        {
            let mut this_amt = 0;

            this_amt = this_amt.max(
                poll_wrapper!(
                    partition_group.partition_object,
                    partition_group.partition_size_stream,
                    cached_size,
                    size
                )
                .unwrap_or(0),
            );
            this_amt = this_amt.max(
                poll_wrapper!(
                    partition_group.filesystem_object,
                    partition_group.filesystem_size_stream,
                    cached_size,
                    size
                )
                .unwrap_or(0),
            );
            this_amt = this_amt.max(
                poll_wrapper!(
                    partition_group.block_object,
                    partition_group.block_size_stream,
                    cached_size,
                    size
                )
                .unwrap_or(0),
            );

            out += this_amt;
        }

        Some(out)
    }

    pub async fn get_temperature(&mut self) -> Option<u32> {
        let mut hwmon_dirs = ArrayString::<256>::new();
        let device_id = &self.disk_id;
        if let Err(e) = write!(
            &mut hwmon_dirs,
            "/sys/block/{device_id}/device/hwmon[0-9]*/temp[0-9]*_input"
        ) {
            log::warn!("Failed to format hwmon dirs: {e:?}");
            return None;
        };

        let glob = match glob(hwmon_dirs.as_str()) {
            Ok(glob) => glob,
            Err(e) => {
                log::warn!("Failed to glob hwmon dirs: {e:?}");
                return None;
            }
        };

        if let Some(temperature) = glob
            .filter_map(Result::ok)
            .filter_map(|f| read_i64(f.as_os_str().to_string_lossy().as_ref(), "temperature"))
            .map(|i| (i + util::MK_TO_0_C) as u32)
            .next()
        {
            return Some(temperature);
        }

        poll_wrapper!(
            self.drive_ata,
            self.stream_listeners.ata_temp_change_stream,
            cached_smart_temperature,
            smart_temperature
        )
        .map(|f| (f * 1000.) as u32)
        .and_then(|v| if v == 0 { None } else { Some(v) })
    }

    pub fn create_stats(&self) -> Stats {
        Stats::load(&self.disk_id)
    }

    pub fn update_stats(&mut self) {
        let mut new_stats = self.create_stats();

        new_stats.update(&self.stats, &mut self.disk);

        self.stats = new_stats;
    }

    pub fn update_partitions(&mut self, mount_info: &HashMap<String, Vec<MountInfo>>) {
        self.disk.partitions.clear();

        for pg in self.partition_groups.iter_mut() {
            if !pg.count_to_formatted {
                continue;
            }

            // Use the encrypted device name if available, since that is supposed to have mount points
            let lookup_key = pg.encrypted_devname.as_deref().unwrap_or(&pg.devname);
            let Some(mounts) = mount_info.get(lookup_key) else {
                continue;
            };

            let Some(partition_info) = pg.update_partitions(mounts) else {
                log::error!("Mount info for device '{}' is empty", pg.devname);
                continue;
            };

            if partition_info.mountpoints.iter().any(|m| m == "/") {
                self.disk.is_system = true;
            }

            // The physical device is always used as a key for the partition list
            self.disk
                .partitions
                .insert(pg.devname.clone(), partition_info);
        }
    }

    pub async fn update_disk_obj(&mut self, client: Option<&Client>) {
        self.disk.capacity_bytes = self.get_capacity().await.unwrap_or(0);
        self.disk.ejectable = self.get_ejectable().await;
        self.disk.temperature_milli_k = self.get_temperature().await;

        if let Some(client) = client {
            if let Some(partition_groups) = self.load_partition_groups(client).await {
                self.partition_groups = partition_groups;
            }
        }

        self.disk.formatted_bytes = self.get_formatted_bytes().await;
    }

    pub async fn create_disk_obj(&mut self) -> Disk {
        Disk {
            id: self.disk_id.clone(),
            model: util::model(&self.disk_id),
            kind: util::kind(&self.disk_id, self.drive_proxy.as_ref())
                .await
                .map(|kind| kind.into()),
            smart_interface: util::smart_interface(
                self.drive_ata.as_ref(),
                self.nvme_controller.as_ref(),
            )
            .map(|kind| kind.into()),
            capacity_bytes: self.get_capacity().await.unwrap_or(0),
            formatted_bytes: self.get_formatted_bytes().await,
            is_system: self.is_system,
            busy_percent: 0.0,
            response_time_ms: 0.0,
            rx_speed_bytes_ps: 0,
            rx_bytes_total: 0,
            tx_speed_bytes_ps: 0,
            tx_bytes_total: 0,
            ejectable: self.get_ejectable().await,
            temperature_milli_k: self.get_temperature().await,
            serial_number: self.get_serial().await,
            world_wide_name: self.get_wwn().await,
            rotation_rate: self.get_rotation_rate().await,
            sector_size: 512,

            partitions: Default::default(),
        }
    }

    async fn get_capacity(&mut self) -> Option<u64> {
        if let Some(cap) = poll_wrapper!(
            self.block_proxy,
            self.stream_listeners.capacity_change_stream,
            cached_size,
            size
        ) {
            return Some(cap);
        }

        //backup method

        let disk_id = &self.disk_id;
        let mut size_file = ArrayString::<256>::new();
        if let Err(e) = write!(&mut size_file, "/sys/block/{disk_id}/size") {
            log::warn!("Failed to format disk size file: {e:?}");
            return None;
        }

        read_u64(size_file.as_str(), "disk size").and_then(
            |cap| {
                if cap == 0 {
                    None
                } else {
                    Some(cap)
                }
            },
        )
    }

    async fn get_ejectable(&mut self) -> bool {
        poll_wrapper!(
            self.drive_proxy,
            self.stream_listeners.ejectable_change_stream,
            cached_ejectable,
            ejectable
        )
        .unwrap_or(false)
    }
}
