/* src/about.rs
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
use glob::glob;
use std::env;
use std::process::Command;
use trim_in_place::TrimInPlace;

use magpie_platform::about::{About, AboutDeInfo, AboutDeviceInfo, AboutOsInfo};

use crate::util::xdg_data_dirs;

pub struct AboutCache {
    about: About,
}

fn refresh_about(about: &mut About) {
    extract_os_info(&mut about.os_info);
    extract_de_info(&mut about.de_info);
    extract_device_info(&mut about.device_info);
}

fn extract_device_info(info: &mut AboutDeviceInfo) {
    info.hostname = match std::fs::read_to_string("/etc/hostname") {
        Ok(host) => Some(host.trim().to_string()),
        Err(e) => {
            log::error!("Failed to read about information: {e}");

            None
        }
    };
}

fn get_var(var: &str) -> Option<String> {
    match env::var(var) {
        Err(e) => {
            log::warn!("Failed to get environment variable {}: {}", var, e);

            None
        }
        Ok(sys) => Some(sys),
    }
}

fn get_command_output(command: &[&str]) -> Option<String> {
    let exec = Command::new(command[0]).args(&command[1..]).output();
    match exec {
        Err(e) => {
            log::error!("Failed to get command output: {:?}: {e}", command);
            None
        }
        Ok(v) => {
            let stdout = String::from_utf8_lossy(&v.stdout)
                .to_string()
                .trim()
                .to_string();

            if stdout.is_empty() {
                log::warn!(
                    "No output from command output. Stderr (may be helpful): '{}'",
                    String::from_utf8_lossy(&v.stderr).trim()
                );
            }

            Some(stdout)
        }
    }
}

fn get_file(file: &str) -> Option<String> {
    match std::fs::read_to_string(file) {
        Ok(host) => Some(host.trim().to_string()),
        Err(e) => {
            log::error!("Failed to read about information: failed to read {file}: {e}");

            None
        }
    }
}

macro_rules! get_simple_vers_output {
    ($executable: literal, $nice_name: literal) => {
        (
            get_command_output(&[$executable, "--version"]).and_then(|x| {
                match x.split_whitespace().last() {
                    None => {
                        log::warn!(
                            "Failed to parse command output for {} version: couldn't parse '{}'",
                            $nice_name,
                            x
                        );
                        None
                    }
                    Some(v) => Some(v.to_string()),
                }
            }),
            Some($nice_name.to_string()),
        )
    };
}

fn extract_de_info(info: &mut AboutDeInfo) {
    info.windowing_system = get_var("XDG_SESSION_TYPE");
    info.desktop_environment = get_var("XDG_CURRENT_DESKTOP");
    //info.session_id = get_var("XDG_SESSION_ID");
    //info.session_type = get_var("XDG_SESSION_CLASS");
    info.virtual_terminal = get_var("XDG_VTNR");

    (info.version, info.desktop_environment) = if let Some(de) = &info.desktop_environment {
        match de.as_str() {
            "XFCE" => get_simple_vers_output!("xfce4-panel", "XFCE"),
            "Budgie:GNOME" | "Budgie" => get_simple_vers_output!("budgie-desktop", "Budgie"),
            "ubuntu:GNOME" | "pop:GNOME" | "zorin:GNOME" | "GNOME" => {
                get_simple_vers_output!("gnome-shell", "GNOME")
            }
            "sway" => get_simple_vers_output!("sway", "sway"),
            "X-Cinnamon" | "CINNAMON" => get_simple_vers_output!("cinnamon", "Cinnamon"),
            "MATE" => get_simple_vers_output!("mate-about", "MATE"),
            "KDE" => get_simple_vers_output!("plasmashell", "KDE"),
            "LXQt" => get_simple_vers_output!("lxqt-about", "LXQt"),
            _ => {
                log::error!("Unknown DE: {}, please file an issue", &de);
                (None, info.desktop_environment.clone())
            }
        }
    } else {
        (None, None)
    }
}

fn extract_logo_bytes(logo_name: &str) -> Option<Vec<u8>> {
    fn find_logo_image(base_dir: &str, logo_name: &str) -> Option<String> {
        for pattern in [
            format!("{}/icons/**/{}.svg", base_dir, logo_name),
            format!("{}/pixmaps/**/{}.svg", base_dir, logo_name),
            format!("{}/icons/**/{}.png", base_dir, logo_name),
            format!("{}/pixmaps/**/{}.png", base_dir, logo_name),
            format!("{}/icons/**/{}.xpm", base_dir, logo_name),
            format!("{}/pixmaps/**/{}.xpm", base_dir, logo_name),
        ] {
            let Ok(globbed) = glob(&pattern) else {
                return None;
            };

            log::info!("Searching: {pattern}");

            for entry in globbed.filter_map(Result::ok) {
                if !entry.is_file() {
                    continue;
                }

                let logo_path = entry.display().to_string();
                log::info!("Found logo icon: {logo_path}");
                return Some(logo_path);
            }
        }
        None
    }

    for dir in xdg_data_dirs() {
        if let Some(logo_path) = find_logo_image(dir, logo_name) {
            if let Ok(bytes) = std::fs::read(logo_path) {
                return Some(bytes);
            }
        }
    }

    log::warn!("Failed to find distributor logo: {logo_name}");

    None
}

fn extract_os_info(info: &mut AboutOsInfo) {
    let osinfo = match std::fs::read_to_string("/etc/os-release") {
        Ok(osinfo) => osinfo.trim().to_string(),
        Err(e) => {
            log::error!("Failed to read about information: {e}");
            return;
        }
    };

    for line in osinfo.trim().lines() {
        let mut split = line.split("=");
        let key = split.next().map_or("", |s| s);
        let value = split
            .collect::<Vec<_>>()
            .join("=")
            .trim_matches_in_place('"')
            .to_string();

        match key {
            "NAME" => info.name = Some(value),
            "PRETTY_NAME" => info.pretty_name = Some(value),
            "ID" => info.id = Some(value),
            "ID_LIKE" => info.id_like = Some(value),
            "BUILD_ID" => info.version_id = Some(value),

            "HOME_URL" => info.home_url = Some(value),
            "SUPPORT_URL" => info.support_url = Some(value),
            "BUG_REPORT_URL" => info.bug_report_url = Some(value),
            "PRIVACY_POLICY_URL" => info.privacy_policy_url = Some(value),
            "DOCUMENTATION_URL" => info.documentation_url = Some(value),

            "LOGO" => info.logo = extract_logo_bytes(&value),

            "ANSI_COLOR" => (), // TODO: figure out how to read this color
            _ => log::warn!("Unrecognized about information: {key}"),
        }
    }

    info.os_type = get_file("/proc/sys/kernel/ostype");
    info.kernel_release = get_file("/proc/sys/kernel/osrelease");
    info.kernel_version = get_file("/proc/sys/kernel/version");
    info.os_architecture = get_file("/proc/sys/kernel/arch");

    fn get_package_manager(id: &str) -> (Option<String>, Option<String>) {
        match id {
            "arch" => {
                // The output looks like it changes with every update, so add fallback
                fn get_pacman_version_1() -> Option<String> {
                    if let Some(output) = get_command_output(&["pacman", "--version"]) {
                        let line = match output.lines().next() {
                            Some(s) => s,
                            None => {
                                log::error!("Failed to parse command output for Pacman version");
                                return None;
                            }
                        };
                        let mut next = false;
                        for field in line.split_whitespace() {
                            if next {
                                return Some(field.to_string());
                            } else if field == "Pacman" {
                                next = true;
                            }
                        }
                    }
                    None
                }
                fn get_pacman_version_2() -> Option<String> {
                    if let Some(output) = get_command_output(&["pacman", "-Q", "pacman"]) {
                        match output.split_whitespace().last() {
                            None => {
                                log::warn!("failed to parse command output for Pacman version");
                                None
                            }
                            Some(v) => Some(v.to_string()),
                        }
                    } else {
                        None
                    }
                }
                (
                    get_pacman_version_1().or(get_pacman_version_2()),
                    Some("Pacman".into()),
                )
            }
            "ubuntu" | "debian" | "mint" => {
                let version = if let Some(output) = get_command_output(&["apt", "--version"]) {
                    match output.split_whitespace().nth(1) {
                        None => {
                            log::warn!("failed to parse command output for Apt version");
                            None
                        }
                        Some(v) => Some(v.to_string()),
                    }
                } else {
                    None
                };
                (version, Some("Apt".into()))
            }
            "fedora" => {
                get_simple_vers_output!("dnf", "dnf")
            }
            "opensuse" => {
                get_simple_vers_output!("zypper", "Zypper")
            }
            _ => {
                log::error!(
                    "Unknown Package manager for OS: {}, please file an issue",
                    id
                );
                (None, None)
            }
        }
    }
    // (info.package_manager_version, info.package_manager) = get_package_manager(&String::from("ubuntu"));
    if let Some(id_like) = &info.id_like {
        for id in id_like.split_whitespace() {
            if info.package_manager.is_none() {
                (info.package_manager_version, info.package_manager) = get_package_manager(id);
            } else {
                break;
            }
        }
    }
    if info.package_manager.is_none() {
        if let Some(id) = &info.id {
            (info.package_manager_version, info.package_manager) = get_package_manager(id.as_str());
        }
    }
}

impl magpie_platform::about::AboutCache for AboutCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let about = About::default();

        Self { about }
    }

    fn refresh(&mut self) {
        refresh_about(&mut self.about);
    }

    fn about(&self) -> &About {
        &self.about
    }
}
