/* src/fan/mod.rs
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

use std::{fs::read_to_string, iter::zip, path::Path};

use glob::glob;
use magpie_platform::fan::Fan;

use crate::config_dir;

const MK_TO_0_C: u32 = 273150;

pub struct FanPath {
    base_path: Box<Path>,
    config_path: Option<Box<Path>>,
}

impl FanPath {
    fn read_to_string(&self, appendix: &str) -> Result<String, Box<dyn std::error::Error>> {
        Ok(match &self.config_path {
            Some(config_path) => {
                let path = config_path.join(appendix);
                if path.exists() {
                    log::info!(
                        "Found override for {}: {}",
                        &self.base_path.display(),
                        &path.display()
                    );
                    read_to_string(path)
                } else {
                    read_to_string(self.base_path.join(appendix))
                }
            }
            None => read_to_string(self.base_path.join(appendix)),
        }?)
    }
}

pub struct FanCache {
    fans: Vec<Fan>,
    fanpaths: Vec<FanPath>,
}

fn new_fan(path: &Path) -> Result<(Fan, FanPath), Box<dyn std::error::Error>> {
    // read the first glob result for hwmon location
    let parent_dir: Box<Path> = path.parent().unwrap().into();

    let hwmon_name = read_to_string(parent_dir.join("name"))?.trim().to_string();
    let config_path = config_dir::build_path(&format!("fans/{hwmon_name}"));

    let fanpath = FanPath {
        base_path: parent_dir,
        config_path,
    };

    let hwmon_idx = fanpath.base_path.file_name().unwrap().to_str().unwrap()[5..].parse::<u32>()?;

    // read the second glob result for fan index
    let findex = path.file_name().unwrap().to_str().unwrap();
    let findex = findex[3..(findex.len() - "_input".len())].parse::<u32>()?;

    // if rpm don't exist skip directory
    let _ = fanpath
        .read_to_string(&format!("fan{}_input", findex))?
        .trim()
        .parse::<u32>()?;

    // do not change the case here. we report how it is, not how it ought to be.
    let fan_label = fanpath
        .read_to_string(&format!("fan{}_label", findex))
        .map(|it| it.trim().to_owned())
        .ok()
        .and_then(|it| if it.is_empty() { None } else { Some(it) });

    let temp_label = fanpath
        .read_to_string(&format!("temp{}_label", findex))
        .map(|it| it.trim().to_owned())
        .ok()
        .and_then(|it| if it.is_empty() { None } else { Some(it) });

    let max_speed = match fanpath.read_to_string(&format!("fan{}_max", findex)) {
        Err(_) => None,
        Ok(it) => it.trim().parse::<u32>().ok(),
    };

    let fan = Fan {
        fan_index: findex,
        hwmon_index: hwmon_idx,
        fan_label,
        temp_name: temp_label,
        temp_amount: None,
        rpm: 0,
        pwm_percent: None,

        max_rpm: max_speed,
    };
    Ok((fan, fanpath))
}

fn update_fan((fan, fanpath): (&mut Fan, &FanPath)) {
    fan.pwm_percent = match fanpath.read_to_string(&format!("pwm{}", fan.fan_index)) {
        Err(_) => None,
        Ok(it) => it.trim().parse::<u64>().ok().map(|v| v as f32 / 255.),
    };

    fan.rpm = match fanpath.read_to_string(&format!("fan{}_input", fan.fan_index)) {
        Err(_) => None,
        Ok(it) => it.trim().parse::<u32>().ok(),
    }
    .unwrap_or_default();

    fan.temp_amount = match fanpath.read_to_string(&format!("temp{}_input", fan.fan_index)) {
        Err(_) => None,
        Ok(v) => v
            .trim()
            .parse::<i64>()
            .map(|v| (v + MK_TO_0_C as i64) as u32)
            .ok(),
    };
}

impl magpie_platform::fan::FanCache for FanCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let config_dir = config_dir::build_path("hwmon0");
        let mut fans = Vec::new();
        let mut fanpaths = Vec::new();

        let (search_str, error_str) = if let Some(config_dir) = &config_dir {
            (
                format!("{}/fan[0-9]*_input", config_dir.display()),
                "config files",
            )
        } else {
            (
                "/sys/class/hwmon/hwmon[0-9]*/fan[0-9]*_input".into(),
                "hwmon entry",
            )
        };
        match glob(&search_str) {
            Ok(globs) => {
                for entry in globs {
                    match entry {
                        Ok(path) => match new_fan(&path) {
                            Ok((fan, fanpath)) => {
                                fans.push(fan);
                                fanpaths.push(fanpath);
                            }
                            Err(e) => log::warn!("Failed to add fan from {}: {}", error_str, e),
                        },
                        Err(e) => {
                            log::warn!("Failed to read {}: {}", error_str, e)
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to read {}: {}", error_str, e)
            }
        };
        Self { fans, fanpaths }
    }

    fn refresh(&mut self) {
        for fan in zip(self.fans.iter_mut(), self.fanpaths.iter()) {
            update_fan(fan)
        }
    }

    fn cached_entries(&self) -> &[Fan] {
        &self.fans
    }
}

#[cfg(test)]
mod tests {
    use magpie_platform::fan::FanCache;

    #[test]
    fn test_disks_cache() {
        let mut cache = super::FanCache::new();
        cache.refresh();
    }
}
