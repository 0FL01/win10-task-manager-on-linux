/* crates/mc-magpie/src/fixture.rs
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

//! Recorded readings on disk: `readings-NNNN.json` snapshots plus an optional `icons.json`.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::{Icon, Readings};

const ICONS_FILE: &str = "icons.json";

/// JSON cannot represent NaN; battery history uses NaN to mark gaps, so it is stored as this
/// sentinel and restored on load.
const NAN_SENTINEL: f32 = -1.0e30;

fn encode_nan(reading: &Readings) -> Readings {
    let mut r = reading.clone();
    for b in r.batteries.iter_mut() {
        for p in b.history.iter_mut() {
            if p.y.is_nan() {
                p.y = NAN_SENTINEL;
            }
            if p.x.is_nan() {
                p.x = NAN_SENTINEL;
            }
        }
    }
    r
}

fn decode_nan(r: &mut Readings) {
    for b in r.batteries.iter_mut() {
        for p in b.history.iter_mut() {
            if p.y == NAN_SENTINEL {
                p.y = f32::NAN;
            }
            if p.x == NAN_SENTINEL {
                p.x = f32::NAN;
            }
        }
    }
}

pub fn write_fixture_dir(
    dir: &Path,
    readings: &[Readings],
    icons: &HashMap<String, Icon>,
) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;

    for (index, reading) in readings.iter().enumerate() {
        let file = std::fs::File::create(dir.join(format!("readings-{index:04}.json")))?;
        serde_json::to_writer(io::BufWriter::new(file), &encode_nan(reading))
            .map_err(io::Error::other)?;
    }

    if !icons.is_empty() {
        let file = std::fs::File::create(dir.join(ICONS_FILE))?;
        serde_json::to_writer(io::BufWriter::new(file), icons).map_err(io::Error::other)?;
    }

    Ok(())
}

pub fn load_fixture_dir(dir: &Path) -> io::Result<(Vec<Readings>, HashMap<String, Icon>)> {
    let mut files = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("readings-") && n.ends_with(".json"))
        })
        .collect::<Vec<_>>();
    files.sort();

    let mut readings = Vec::with_capacity(files.len());
    for path in files {
        let file = std::fs::File::open(&path)?;
        let mut reading: Readings = serde_json::from_reader(io::BufReader::new(file))
            .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
        decode_nan(&mut reading);
        readings.push(reading);
    }

    let icons_path = dir.join(ICONS_FILE);
    let icons = if icons_path.exists() {
        let file = std::fs::File::open(&icons_path)?;
        serde_json::from_reader(io::BufReader::new(file)).map_err(io::Error::other)?
    } else {
        HashMap::new()
    };

    Ok((readings, icons))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_dir_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mc-fixture-test-{}", std::process::id()));
        let mut r = Readings::new();
        r.cpu.total_usage_percent = 12.5;
        r.batteries.push(crate::Battery {
            history: vec![
                magpie_types::battery::HistoryPoint { x: 1., y: 0.5 },
                magpie_types::battery::HistoryPoint { x: 2., y: f32::NAN },
            ],
            ..Default::default()
        });
        let mut icons = HashMap::new();
        icons.insert(
            "org.example.App".to_string(),
            Icon::Id("org.example.App".into()),
        );

        write_fixture_dir(&dir, &[r.clone(), r], &icons).unwrap();
        let (readings, loaded_icons) = load_fixture_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(readings.len(), 2);
        assert_eq!(readings[1].cpu.total_usage_percent, 12.5);
        assert!(readings[0].batteries[0].history[1].y.is_nan());
        assert_eq!(loaded_icons.len(), 1);
    }

    #[test]
    fn old_gpu_and_process_json_without_telemetry_fields_still_loads() {
        let mut json = serde_json::to_value(crate::synthetic_readings(3, 10)).unwrap();
        for gpu in json["gpus"].as_object_mut().unwrap().values_mut() {
            gpu.as_object_mut().unwrap().remove("telemetry");
        }
        for process in json["running_processes"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            process.as_object_mut().unwrap().remove("start_time_ticks");
        }
        let reading: Readings = serde_json::from_value(json).unwrap();
        assert!(reading.gpus.values().all(|gpu| gpu.telemetry.is_none()));
        assert!(reading
            .running_processes
            .values()
            .all(|p| p.start_time_ticks.is_none()));
    }
}
