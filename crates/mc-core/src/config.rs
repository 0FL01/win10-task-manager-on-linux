/* crates/mc-core/src/config.rs
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

//! User settings. Field names map 1:1 onto the keys of the GSettings schema of the Mission
//! Center GTK application this project is based on (`io.missioncenter.MissionCenter`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use crate::format::UnitSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
    #[default]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterfaceStyle {
    #[default]
    System,
    Light,
    Dark,
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Parses `id=width;…` (unknown or malformed entries are skipped).
pub fn parse_widths(raw: &str) -> std::collections::HashMap<String, f32> {
    raw.split(';')
        .filter_map(|entry| {
            let (id, width) = entry.split_once('=')?;
            let width: f32 = width.trim().parse().ok()?;
            (width.is_finite() && width > 0.).then(|| (id.trim().to_owned(), width))
        })
        .collect()
}

/// `id=width;…` in the given order.
pub fn serialize_widths<'a>(widths: impl IntoIterator<Item = (&'a str, f32)>) -> String {
    widths
        .into_iter()
        .map(|(id, w)| format!("{id}={}", w.round() as i32))
        .collect::<Vec<_>>()
        .join(";")
}

/// Version of the interface defaults applied by [`Config::migrate_layout`].
pub const LAYOUT_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub window_width: i32,
    pub window_height: i32,
    pub is_maximized: bool,
    pub window_selected_page: String,
    pub window_interface_style: InterfaceStyle,

    /// Refresh interval in multiples of 50 ms (10..=200).
    #[serde(rename = "app-update-interval-u64")]
    pub app_update_interval: u64,

    pub performance_page_data_points: i32,
    pub performance_page_memory2_use_bytes: bool,
    pub performance_page_memory2_use_base2: bool,
    pub performance_page_drive_use_bytes: bool,
    pub performance_page_drive_use_base2: bool,
    pub performance_page_network_use_bytes: bool,
    pub performance_page_network_use_base2: bool,
    pub performance_page_network_dynamic_scaling: bool,

    pub apps_page_merged_process_stats: bool,
    pub apps_page_remember_sorting: bool,
    pub apps_page_remember_column_order: bool,
    pub apps_page_core_count_affects_percentages: bool,
    pub apps_page_show_column_separators: bool,
    pub apps_page_hide_is_zero: bool,
    pub apps_page_sorting_column_name: String,
    pub apps_page_sorting_order: SortOrder,
    pub apps_page_column_order: String,
    /// Visible numeric columns of the Processes tab (`;` separated column ids)
    pub apps_page_visible_columns: String,
    /// View → Group by type
    pub apps_page_group_by_type: bool,
    /// Processes column widths, `id=width;…` (Name and every numeric column)
    pub apps_page_column_widths: String,
    /// Expanded/collapsed rows with stable keys (groups, apps), `;` separated
    pub apps_page_expanded: String,

    pub details_page_sorting_column_name: String,
    pub details_page_sorting_order: SortOrder,
    pub details_page_column_widths: String,
    pub details_page_column_order: String,
    pub services_page_column_widths: String,

    /// Options → Always on top
    pub window_always_on_top: bool,
    /// "Fewer details" (compact) view
    pub window_fewer_details: bool,
    /// View → Update speed → Paused
    pub app_update_paused: bool,

    pub services_page_sorting_column_name: String,
    pub services_page_sorting_order: SortOrder,
    pub services_page_column_order: String,

    /// 1: overall, 2: per core, 3: per socket? (kept as in GSettings, 1..=4)
    pub performance_page_cpu_graph: i32,
    pub performance_page_cpu_graph_bottom: i32,
    pub performance_page_kernel_times: bool,
    pub performance_selected_page: String,
    pub performance_smooth_graphs: bool,
    pub performance_sliding_graphs: bool,

    pub performance_show_cpu: bool,
    pub performance_show_memory: bool,
    pub performance_show_disks: bool,
    pub performance_show_network: bool,
    pub performance_show_network_wired: bool,
    pub performance_show_network_wireless: bool,
    pub performance_show_network_vpn: bool,
    pub performance_show_network_virtual: bool,
    pub performance_show_network_other: bool,
    pub performance_show_gpus: bool,
    pub performance_show_fans: bool,
    pub performance_show_batteries: bool,

    pub performance_page_memory_composition_visible: bool,
    pub performance_page_memory_swap_visible: bool,
    pub performance_page_gpu_graph_middle: i32,
    pub performance_page_gpu_graph_bottom: i32,

    pub performance_sidebar_order: String,
    /// Width of the Performance tab device list in points (draggable separator)
    pub performance_sidebar_width: i32,
    pub performance_sidebar_hidden_graphs: String,
    pub performance_sidebar_device_overrides: String,

    pub first_time_running: bool,

    /// Layout defaults already applied to this file (see [`Config::migrate_layout`]). Missing in
    /// files written before the Win10 interface, hence the field-level default of 0.
    #[serde(default)]
    pub layout_version: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_width: 1030,
            window_height: 705,
            is_maximized: false,
            window_selected_page: "performance-page".into(),
            window_interface_style: InterfaceStyle::System,
            app_update_interval: 20,
            performance_page_data_points: 60,
            performance_page_memory2_use_bytes: true,
            performance_page_memory2_use_base2: true,
            performance_page_drive_use_bytes: true,
            performance_page_drive_use_base2: true,
            performance_page_network_use_bytes: false,
            performance_page_network_use_base2: false,
            performance_page_network_dynamic_scaling: true,
            apps_page_merged_process_stats: false,
            apps_page_remember_sorting: true,
            apps_page_remember_column_order: true,
            apps_page_core_count_affects_percentages: false,
            apps_page_show_column_separators: false,
            apps_page_hide_is_zero: false,
            apps_page_sorting_column_name: String::new(),
            apps_page_sorting_order: SortOrder::None,
            apps_page_column_order: String::new(),
            apps_page_visible_columns: "cpu;memory;drive;network".into(),
            apps_page_group_by_type: true,
            apps_page_column_widths: String::new(),
            apps_page_expanded: String::new(),
            details_page_sorting_column_name: String::new(),
            details_page_sorting_order: SortOrder::Ascending,
            details_page_column_widths: String::new(),
            details_page_column_order: String::new(),
            services_page_column_widths: String::new(),
            window_always_on_top: false,
            window_fewer_details: false,
            app_update_paused: false,
            services_page_sorting_column_name: String::new(),
            services_page_sorting_order: SortOrder::None,
            services_page_column_order: String::new(),
            performance_page_cpu_graph: 2,
            performance_page_cpu_graph_bottom: 0,
            performance_page_kernel_times: false,
            performance_selected_page: "cpu".into(),
            performance_smooth_graphs: false,
            performance_sliding_graphs: false,
            performance_show_cpu: true,
            performance_show_memory: true,
            performance_show_disks: true,
            performance_show_network: true,
            performance_show_network_wired: true,
            performance_show_network_wireless: true,
            performance_show_network_vpn: true,
            performance_show_network_virtual: true,
            performance_show_network_other: true,
            performance_show_gpus: true,
            performance_show_fans: true,
            performance_show_batteries: true,
            performance_page_memory_composition_visible: true,
            performance_page_memory_swap_visible: false,
            performance_page_gpu_graph_middle: 1,
            performance_page_gpu_graph_bottom: 2,
            performance_sidebar_order: String::new(),
            performance_sidebar_width: 211,
            performance_sidebar_hidden_graphs: String::new(),
            performance_sidebar_device_overrides: String::new(),
            first_time_running: true,
            layout_version: LAYOUT_VERSION,
        }
    }
}

impl Config {
    /// Clamps numeric settings to the ranges declared by the former GSettings schema.
    pub fn sanitize(&mut self) {
        self.app_update_interval = self.app_update_interval.clamp(10, 200);
        self.performance_page_data_points = self.performance_page_data_points.clamp(10, 600);
        self.performance_page_cpu_graph = self.performance_page_cpu_graph.clamp(1, 4);
        self.performance_page_cpu_graph_bottom = self.performance_page_cpu_graph_bottom.clamp(0, 3);
        self.performance_page_gpu_graph_middle = self.performance_page_gpu_graph_middle.clamp(0, 5);
        self.performance_page_gpu_graph_bottom = self.performance_page_gpu_graph_bottom.clamp(0, 5);
        self.performance_sidebar_width = self.performance_sidebar_width.clamp(211, 1200);
        self.window_width = self.window_width.max(1);
        self.window_height = self.window_height.max(1);
    }

    pub fn units(&self) -> UnitSettings {
        UnitSettings {
            memory_use_bytes: self.performance_page_memory2_use_bytes,
            memory_use_base2: self.performance_page_memory2_use_base2,
            drive_use_bytes: self.performance_page_drive_use_bytes,
            drive_use_base2: self.performance_page_drive_use_base2,
            network_use_bytes: self.performance_page_network_use_bytes,
            network_use_base2: self.performance_page_network_use_base2,
        }
    }

    /// `$XDG_CONFIG_HOME/taskmanager/config.toml` (or `~/.config/...`).
    pub fn default_path() -> PathBuf {
        config_home().join("taskmanager").join("config.toml")
    }

    /// Copies the settings of the former Mission Center based build
    /// (`$XDG_CONFIG_HOME/missioncenter/config.toml`) to `path` if `path` does not exist yet.
    /// Returns `true` if they were copied.
    pub fn migrate_legacy_location(path: &Path) -> bool {
        let legacy = config_home().join("missioncenter").join("config.toml");
        Self::migrate_from(&legacy, path)
    }

    fn migrate_from(legacy: &Path, path: &Path) -> bool {
        if path.exists() || !legacy.exists() {
            return false;
        }
        if let Some(dir) = path.parent()
            && std::fs::create_dir_all(dir).is_err()
        {
            return false;
        }
        match std::fs::copy(legacy, path) {
            Ok(_) => {
                log::info!(target: "TaskManager::Config", "Settings copied from {}", legacy.display());
                true
            }
            Err(e) => {
                log::warn!(target: "TaskManager::Config", "Failed to copy {}: {e}", legacy.display());
                false
            }
        }
    }

    pub fn from_toml(text: &str) -> Result<Self, String> {
        let mut config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.sanitize();
        Ok(config)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    /// Loads the config file. Returns `None` if it does not exist; unreadable files yield the
    /// defaults (and a warning) so a broken file never prevents startup.
    /// Applies the Win10 interface defaults once to settings written by older versions: the
    /// reference window size when the stored one is smaller than `min_size`, per-processor CPU
    /// graphs without the bottom graph, swap graph hidden. Returns `true` if anything changed.
    pub fn migrate_layout(&mut self, min_size: [i32; 2]) -> bool {
        if self.layout_version >= LAYOUT_VERSION {
            return false;
        }
        let defaults = Self::default();
        if self.layout_version < 1 {
            if self.window_width < min_size[0] || self.window_height < min_size[1] {
                self.window_width = defaults.window_width;
                self.window_height = defaults.window_height;
                self.is_maximized = false;
            }
            self.performance_page_cpu_graph = defaults.performance_page_cpu_graph;
            self.performance_page_cpu_graph_bottom = defaults.performance_page_cpu_graph_bottom;
            self.performance_page_memory_swap_visible =
                defaults.performance_page_memory_swap_visible;
        }
        if self.layout_version < 2 {
            // The interface state (sorting, column order and widths) is kept by default
            self.apps_page_remember_sorting = true;
            self.apps_page_remember_column_order = true;
        }
        self.layout_version = LAYOUT_VERSION;
        true
    }

    pub fn load(path: &Path) -> Option<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                log::warn!(target: "TaskManager::Config", "Failed to read {}: {e}", path.display());
                return Some(Self::default());
            }
        };

        match Self::from_toml(&text) {
            Ok(config) => Some(config),
            Err(e) => {
                log::warn!(target: "TaskManager::Config", "Failed to parse {}: {e}", path.display());
                Some(Self::default())
            }
        }
    }

    /// Writes atomically (temporary file + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_toml())?;
        std::fs::rename(tmp, path)
    }
}

/// Persists configuration snapshots on a background thread so the UI thread never touches the
/// file system. Newer snapshots replace older, unwritten ones.
pub struct ConfigWriter {
    pending: Arc<Mutex<Option<Config>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ConfigWriter {
    pub fn new(path: PathBuf) -> Self {
        let pending = Arc::new(Mutex::new(None::<Config>));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let thread = {
            let pending = pending.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("mc-config-writer".into())
                .spawn(move || {
                    loop {
                        let next = pending.lock().ok().and_then(|mut p| p.take());
                        match next {
                            Some(config) => {
                                if let Err(e) = config.save(&path) {
                                    log::warn!(target: "TaskManager::Config", "Failed to save {}: {e}", path.display());
                                }
                                // Coalesce bursts (window resize, column drag); the latest
                                // pending settings are still written, also on drop
                                if !stop.load(std::sync::atomic::Ordering::Acquire) {
                                    std::thread::sleep(std::time::Duration::from_millis(200));
                                }
                            }
                            None if stop.load(std::sync::atomic::Ordering::Acquire) => break,
                            None => std::thread::park(),
                        }
                    }
                })
                .ok()
        };

        Self {
            pending,
            stop,
            thread,
        }
    }

    pub fn save(&self, config: &Config) {
        if let Ok(mut p) = self.pending.lock() {
            *p = Some(config.clone());
        }
        if let Some(t) = &self.thread {
            t.thread().unpark();
        }
    }
}

impl Drop for ConfigWriter {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_gsettings_schema() {
        let c = Config::default();
        assert_eq!(c.app_update_interval, 20);
        assert_eq!(c.performance_page_data_points, 60);
        assert!(!c.performance_page_network_use_bytes);
        assert_eq!(c.performance_selected_page, "cpu");
        assert_eq!(c.performance_page_gpu_graph_bottom, 2);
        assert!(c.first_time_running);
    }

    #[test]
    fn toml_roundtrip() {
        let mut c = Config::default();
        c.apps_page_sorting_order = SortOrder::Descending;
        c.window_interface_style = InterfaceStyle::Dark;
        c.performance_sidebar_order = "cpu;memory".into();
        let back = Config::from_toml(&c.to_toml()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn keys_use_the_gsettings_names() {
        let text = Config::default().to_toml();
        assert!(text.contains("performance-page-memory2-use-bytes = true"));
        assert!(text.contains("app-update-interval-u64 = 20"));
        assert!(text.contains("window-interface-style = \"system\""));
    }

    #[test]
    fn missing_keys_fall_back_to_defaults_and_ranges_are_clamped() {
        let c = Config::from_toml("performance-page-data-points = 5000\n").unwrap();
        assert_eq!(c.performance_page_data_points, 600);
        assert_eq!(c.app_update_interval, 20);
    }

    #[test]
    fn writer_persists_the_latest_snapshot() {
        let dir = std::env::temp_dir().join(format!("mc-config-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        {
            let writer = ConfigWriter::new(path.clone());
            let mut c = Config::default();
            for i in 10..20 {
                c.app_update_interval = i;
                writer.save(&c);
            }
        }
        let loaded = Config::load(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loaded.app_update_interval, 19);
    }

    #[test]
    fn layout_migration_applies_once() {
        // A file from the GNOME-style interface: small window, overall CPU graph + power graph
        let mut c = Config::from_toml(
            "window-width = 600\nwindow-height = 400\nperformance-page-cpu-graph = 1\nperformance-page-cpu-graph-bottom = 1\n",
        )
        .unwrap();
        assert_eq!(c.layout_version, 0);
        assert!(c.migrate_layout([760, 540]));
        assert_eq!((c.window_width, c.window_height), (1030, 705));
        assert_eq!(
            (
                c.performance_page_cpu_graph,
                c.performance_page_cpu_graph_bottom
            ),
            (2, 0)
        );
        assert!(c.apps_page_remember_sorting && c.apps_page_remember_column_order);
        assert!(!c.migrate_layout([760, 540]), "only once");

        // New files start migrated; a large stored window is kept
        assert!(!Config::default().migrate_layout([760, 540]));
        let mut big = Config::from_toml("window-width = 1600\nwindow-height = 1000\n").unwrap();
        big.migrate_layout([760, 540]);
        assert_eq!((big.window_width, big.window_height), (1600, 1000));
    }

    #[test]
    fn widths_round_trip() {
        let raw = serialize_widths([("name", 234.4), ("cpu", 80.)]);
        assert_eq!(raw, "name=234;cpu=80");
        let parsed = parse_widths(&format!("{raw};bad;x=-3;y=abc"));
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["cpu"], 80.);
    }

    #[test]
    fn ui_state_survives_toml() {
        let mut c = Config::default();
        c.apps_page_column_widths = "name=300".into();
        c.details_page_sorting_column_name = "memory".into();
        c.details_page_sorting_order = SortOrder::Descending;
        c.window_always_on_top = true;
        c.window_fewer_details = true;
        c.apps_page_expanded = "!processes;apps/aorg.example.App".into();
        let back = Config::from_toml(&c.to_toml()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn legacy_settings_are_copied_once() {
        let dir = std::env::temp_dir().join(format!("mc-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let legacy = dir.join("missioncenter/config.toml");
        let new = dir.join("taskmanager/config.toml");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "window-width = 1500\n").unwrap();
        assert!(Config::migrate_from(&legacy, &new));
        assert_eq!(Config::load(&new).unwrap().window_width, 1500);
        std::fs::write(&legacy, "window-width = 900\n").unwrap();
        assert!(
            !Config::migrate_from(&legacy, &new),
            "existing settings are kept"
        );
        assert_eq!(Config::load(&new).unwrap().window_width, 1500);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
