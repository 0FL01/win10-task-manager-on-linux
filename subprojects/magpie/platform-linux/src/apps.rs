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

use std::collections::HashMap;
use std::num::NonZero;
use std::path::PathBuf;

use freedesktop_icons::{default_theme_gtk, lookup};

use magpie_platform::apps::{App, Icon, IconKind};
use magpie_platform::processes::Process;
use magpie_platform::Mutex;

const APP_IGNORELIST: &[&str] = &[
    "guake-prefs",
    "org.codeberg.dnkl.foot-server",
    "org.codeberg.dnkl.footclient",
];

struct ProcessNewType(Process);

impl app_rummage::Process for ProcessNewType {
    fn pid(&self) -> NonZero<u32> {
        NonZero::new(self.0.pid).expect("PID must never be zero")
    }

    fn executable_path(&self) -> Option<PathBuf> {
        if self.0.exe.is_empty() {
            None
        } else {
            Some(PathBuf::from(&self.0.exe))
        }
    }

    fn name(&self) -> &str {
        &self.0.name
    }

    fn cmdline(&self) -> &[String] {
        &self.0.cmd
    }
}

pub struct AppCache {
    apps: Vec<App>,
    cached_icons: HashMap<String, Icon>,
    unresolved_icons: HashMap<String, String>,
}

impl magpie_platform::apps::AppCache for AppCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        Self {
            apps: Vec::new(),
            cached_icons: HashMap::new(),
            unresolved_icons: HashMap::new(),
        }
    }

    fn refresh(&mut self, processes: &Mutex<HashMap<u32, Process>>) {
        let start = std::time::Instant::now();

        let mut installed_apps = app_rummage::installed_apps();
        for app in APP_IGNORELIST {
            installed_apps.remove(*app);
        }

        let processes = processes
            .lock()
            .values()
            .map(|p| ProcessNewType(p.clone()))
            .collect::<Vec<_>>();

        let mut running_apps = app_rummage::running_apps(&installed_apps, &processes);

        for (running_app, _) in &running_apps {
            let Some(icon) = running_app.icon.as_ref() else {
                self.cached_icons
                    .insert(running_app.id.to_string(), Icon { icon: None });
                continue;
            };

            if !self.cached_icons.contains_key(running_app.id.as_ref())
                && !self.unresolved_icons.contains_key(running_app.id.as_ref())
            {
                self.unresolved_icons
                    .insert(running_app.id.to_string(), icon.to_string());
            }
        }

        self.apps = running_apps
            .drain(..)
            .map(|(app, mut pids)| App {
                id: app.id.as_ref().to_string(),
                name: app.name.as_ref().to_string(),
                command: app
                    .exec
                    .as_ref()
                    .map(|exec| exec.as_ref().to_string())
                    .unwrap_or(String::new()),
                pids: pids.drain(..).map(NonZero::<u32>::get).collect(),
            })
            .collect();

        log::debug!(
            "PERF: Refreshed process information in {:?}",
            start.elapsed()
        );

        if !self.unresolved_icons.is_empty() {
            self.refresh_icons();
        }
    }

    fn refresh_icons(&mut self) {
        let start = std::time::Instant::now();

        for (appid, icon) in self.unresolved_icons.drain() {
            // We can't access `/snap` when packaged as a Snap, we can go through the hostfs though.
            // So update the icon path to reflect this change.
            let icon = if crate::is_snap() && icon.starts_with("/snap") {
                format!("{}{}", "/var/lib/snapd/hostfs", icon)
            } else {
                icon.to_string()
            };

            let resolved_icon = if std::fs::exists(&icon).unwrap_or(false) {
                match std::fs::read(&icon) {
                    Ok(bytes) => Icon {
                        icon: Some(IconKind::Data(bytes)),
                    },
                    Err(e) => {
                        log::warn!("Failed to read icon {icon} for {appid} ({e})");
                        Icon {
                            icon: Some(IconKind::Id(icon)),
                        }
                    }
                }
            } else {
                let svgpath = lookup(&icon)
                    .force_svg()
                    .with_cache()
                    .with_size(64)
                    .with_theme(&default_theme_gtk().unwrap_or("hicolor".to_string()))
                    .find();

                let svgfile = svgpath.as_ref().and_then(|path| std::fs::read(path).ok());
                if let Some(bytes) = svgfile {
                    log::debug!("Using {:?} for {} ", svgpath, appid);

                    Icon {
                        icon: Some(IconKind::Data(bytes)),
                    }
                } else {
                    Icon {
                        icon: Some(IconKind::Id(icon)),
                    }
                }
            };

            self.cached_icons.insert(appid.to_string(), resolved_icon);
        }

        log::debug!("App Icon cache refreshed in {:?}", start.elapsed());
    }

    fn cached_entries(&self) -> &[App] {
        &self.apps
    }

    fn cached_icons(&self) -> HashMap<String, Icon> {
        self.cached_icons.clone()
    }
}
