/* src/disks/smart_data.rs
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

use std::collections::HashMap;

use udisks2::ata::AtaProxy;
use udisks2::nvme::controller::ControllerProxy;
use udisks2::Object;
use zbus::zvariant::{Array, OwnedValue, Value};

use magpie_platform::disks::{
    DiskSmartAtaAttribute, DiskSmartData, DiskSmartDataAta, DiskSmartDataNvme,
    DiskSmartDataVariant, DiskSmartPrettyUnitKind, DiskSmartTestResult,
};

pub async fn smart_data(drive: &Object, disk_id: &str, automatic: bool) -> Option<DiskSmartData> {
    if let Ok(ata) = drive.drive_ata().await {
        return ata_smart_data(ata, disk_id, automatic).await;
    };

    if let Ok(nvme) = drive.nvme_controller().await {
        return nvme_smart_data(nvme, disk_id, automatic).await;
    };

    None
}

fn smart_options(automatic: bool) -> HashMap<&'static str, Value<'static>> {
    if automatic {
        HashMap::from([("auth.no_user_interaction", Value::from(true))])
    } else {
        HashMap::new()
    }
}

async fn ata_smart_data(
    ata: AtaProxy<'_>,
    disk_id: &str,
    automatic: bool,
) -> Option<DiskSmartData> {
    let attributes = match ata.smart_get_attributes(smart_options(automatic)).await {
        Ok(res) => res,
        Err(e) => {
            log::error!("Failed to find ata interface {disk_id}: {e}",);
            return None;
        }
    };

    let mut result = DiskSmartData::default();

    result.powered_on_seconds = ata.smart_power_on_seconds().await.unwrap_or(0);
    result.last_update_time = ata.smart_updated().await.unwrap_or(0);
    let test_result: Option<DiskSmartTestResult> = ata
        .smart_selftest_status()
        .await
        .ok()
        .and_then(|tr| tr.as_str().try_into().ok());
    result.test_result = test_result.map(|tr| tr as i32);

    let mut ata_smart_data = DiskSmartDataAta::default();
    for attr_set in attributes {
        ata_smart_data.attributes.push(DiskSmartAtaAttribute {
            id: attr_set.0 as u32,
            name: attr_set.1,
            flags: attr_set.2 as u32,
            value: attr_set.3,
            worst: attr_set.4,
            threshold: attr_set.5,
            pretty: attr_set.6,
            pretty_unit: if attr_set.7 < 0 || attr_set.7 > 4 {
                DiskSmartPrettyUnitKind::Unknown as i32
            } else {
                attr_set.7
            },
        });
    }
    result.data = Some(DiskSmartDataVariant::Ata(ata_smart_data));

    Some(result)
}

async fn nvme_smart_data<'a>(
    controller: ControllerProxy<'a>,
    disk_id: &str,
    automatic: bool,
) -> Option<DiskSmartData> {
    let attributes = match controller
        .smart_get_attributes(smart_options(automatic))
        .await
    {
        Ok(res) => res,
        Err(e) => {
            log::error!("Failed to get attributes for {disk_id}: {e}",);
            return None;
        }
    };

    let mut result = DiskSmartData::default();

    result.powered_on_seconds = controller.smart_power_on_hours().await.unwrap_or(0) * 3600;
    result.last_update_time = controller.smart_updated().await.unwrap_or(0);
    let test_result: Option<DiskSmartTestResult> = controller
        .smart_selftest_status()
        .await
        .ok()
        .and_then(|tr| tr.as_str().try_into().ok());
    result.test_result = test_result.map(|tr| tr as i32);

    let mut nvme_smart_data = DiskSmartDataNvme::default();
    let avail_spare: Option<u8> = attributes
        .get("avail_spare")
        .and_then(|v| v.try_into().ok());
    let spare_thresh: Option<u8> = attributes
        .get("spare_thresh")
        .and_then(|v| v.try_into().ok());
    let percent_used: Option<u8> = attributes
        .get("percent_used")
        .and_then(|v| v.try_into().ok());
    let total_data_read: Option<u64> = attributes
        .get("total_data_read")
        .and_then(|v| v.try_into().ok());
    let total_data_written: Option<u64> = attributes
        .get("total_data_written")
        .and_then(|v| v.try_into().ok());
    let ctrl_busy_minutes: Option<u64> = attributes
        .get("ctrl_busy_time")
        .and_then(|v| v.try_into().ok());
    let power_cycles: Option<u64> = attributes
        .get("power_cycles")
        .and_then(|v| v.try_into().ok());
    let unsafe_shutdowns: Option<u64> = attributes
        .get("unsafe_shutdowns")
        .and_then(|v| v.try_into().ok());
    let media_errors: Option<u64> = attributes
        .get("media_errors")
        .and_then(|v| v.try_into().ok());
    let num_err_log_entries: Option<u64> = attributes
        .get("num_err_log_entries")
        .and_then(|v| v.try_into().ok());
    let warn_composite_temp_thresh: Option<u16> =
        attributes.get("wctemp").and_then(|v| v.try_into().ok());
    let crit_composite_temp_thresh: Option<u16> =
        attributes.get("cctemp").and_then(|v| v.try_into().ok());
    let warn_composite_temp_time: Option<u32> = attributes
        .get("warning_temp_time")
        .and_then(|v| v.try_into().ok());
    let crit_composite_temp_time: Option<u32> = attributes
        .get("critical_temp_time")
        .and_then(|v| v.try_into().ok());

    if let Some(temp_sensors) = attributes.get("temp_sensors") {
        match <&OwnedValue as TryInto<&Array>>::try_into(temp_sensors) {
            Ok(temp_sensors) => {
                nvme_smart_data.temp_sensors = temp_sensors
                    .iter()
                    .filter_map(|v| v.try_into().ok())
                    .filter(|v| *v > 0)
                    .collect();
            }
            Err(e) => {
                log::error!("Failed to parse temp_sensors for {disk_id}: {e}",);
            }
        }
    }
    nvme_smart_data.avail_spare = avail_spare.map(|v| v as _);
    nvme_smart_data.spare_thresh = spare_thresh.map(|v| v as _);
    nvme_smart_data.percent_used = percent_used.map(|v| v as _);
    nvme_smart_data.total_data_read = total_data_read;
    nvme_smart_data.total_data_written = total_data_written;
    nvme_smart_data.ctrl_busy_minutes = ctrl_busy_minutes;
    nvme_smart_data.power_cycles = power_cycles;
    nvme_smart_data.unsafe_shutdowns = unsafe_shutdowns;
    nvme_smart_data.media_errors = media_errors;
    nvme_smart_data.num_err_log_entries = num_err_log_entries;
    nvme_smart_data.warn_composite_temp_thresh = warn_composite_temp_thresh.map(|v| v as _);
    nvme_smart_data.crit_composite_temp_thresh = crit_composite_temp_thresh.map(|v| v as _);
    nvme_smart_data.warn_composite_temp_time = warn_composite_temp_time;
    nvme_smart_data.crit_composite_temp_time = crit_composite_temp_time;

    result.data = Some(DiskSmartDataVariant::Nvme(nvme_smart_data));
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_automatic_smart_suppresses_authorization_prompts() {
        assert!(smart_options(false).is_empty());
        let options = smart_options(true);
        assert_eq!(options.len(), 1);
        assert_eq!(options["auth.no_user_interaction"], Value::from(true));
    }
}
