/* table_view/models.rs
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

use std::collections::{HashMap, HashSet};

use gtk::gio;
use gtk::glib::g_critical;
use gtk::prelude::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;

use magpie_types::apps::App;
use magpie_types::processes::{Process, ProcessUsageStats};
use magpie_types::services::Service;

use crate::table_view::cached_icon::LightCachedIcon;
use crate::table_view::row_model::{ContentType, RowModel, RowModelBuilder, SectionType};

pub fn update_apps(
    app_map: &HashMap<String, App>,
    process_map: &HashMap<u32, Process>,
    process_model_map: &HashMap<u32, RowModel>,
    app_icons: &mut HashMap<u32, LightCachedIcon>,
    list: &gio::ListStore,
) {
    app_icons.clear();

    let mut has_died = HashSet::new();
    let mut does_exist = HashSet::new();

    list.iter::<RowModel>().flatten().for_each(|row_model| {
        let app_id = row_model.id();
        let app_id = app_id.to_string();
        if let Some(app) = app_map.get(&app_id) {
            update_app(app, process_map, process_model_map, app_icons, row_model);

            does_exist.insert(app_id);
        } else {
            has_died.insert(app_id);
        }
    });

    list.retain(|object| {
        object
            .downcast_ref::<RowModel>()
            .map(|rm| !has_died.contains(rm.id().as_str()))
            .unwrap_or(false)
    });

    for (_, app) in app_map
        .iter()
        .filter(|(id, _)| !does_exist.contains(id.as_str()))
    {
        let row_model = RowModelBuilder::new()
            .content_type(ContentType::App)
            .section_type(SectionType::FirstSection)
            .light_cached_icon(LightCachedIcon::AppCachedKey(app.id.clone(), 24))
            .id(&app.id)
            .name(&app.name)
            .build();
        list.append(&row_model);

        update_app(app, process_map, process_model_map, app_icons, row_model);
    }
}

pub fn update_processes(
    process_map: &HashMap<u32, Process>,
    pids: HashSet<u32>,
    list: &gio::ListStore,
    app_icons: &HashMap<u32, LightCachedIcon>,
    icon: &LightCachedIcon,
    use_merged_stats: bool,
    section_type: SectionType,
    parent_service: Option<&Service>,
    model_map: &mut HashMap<u32, RowModel>,
) {
    let mut does_exist = HashSet::new();
    let mut has_died = HashSet::new();

    list.iter::<RowModel>().flatten().for_each(|row_model| {
        let pid = row_model.pid();
        if pids.contains(&pid) {
            if let Some(process) = process_map.get(&pid) {
                update_process(
                    process_map,
                    &process,
                    row_model,
                    app_icons,
                    icon,
                    use_merged_stats,
                    section_type,
                    parent_service,
                    model_map,
                );

                does_exist.insert(pid);
            } else {
                has_died.insert(pid);
            }
        } else {
            has_died.insert(pid);
        }
    });

    list.retain(|object| {
        object
            .downcast_ref::<RowModel>()
            .map(|rm| !has_died.contains(&rm.pid()))
            .unwrap_or(false)
    });

    for process in pids
        .iter()
        .filter(|pid| !does_exist.contains(pid))
        .filter_map(|pid| process_map.get(&pid))
    {
        let command_line = process.cmd.join(" ");

        let pretty_name = if process.exe.is_empty() {
            if let Some(cmd) = process.cmd.first() {
                let mut cmd = cmd
                    .split_ascii_whitespace()
                    .next()
                    .and_then(|s| s.split('/').last())
                    .unwrap_or(&process.name);
                if let Some(s) = cmd.strip_suffix(':') {
                    cmd = s;
                }
                cmd.trim()
            } else {
                process.name.trim()
            }
        } else {
            let exe_name = process.exe.split('/').last().unwrap_or(&process.name);
            if exe_name.starts_with("wine") {
                if process.cmd.is_empty() {
                    process.name.trim()
                } else {
                    process.cmd[0]
                        .split("\\")
                        .last()
                        .unwrap_or(&process.name)
                        .split("/")
                        .last()
                        .unwrap_or(&process.name)
                        .trim()
                }
            } else {
                exe_name.trim()
            }
        };

        let row_model = RowModelBuilder::new()
            .content_type(ContentType::Process)
            .section_type(section_type)
            .id(&process.pid.to_string())
            .pid(process.pid)
            .name(pretty_name)
            .command_line(&command_line)
            .build();
        list.append(&row_model);

        update_process(
            process_map,
            &process,
            row_model,
            app_icons,
            icon,
            use_merged_stats,
            section_type,
            parent_service,
            model_map,
        );
    }
}

pub fn update_services(
    process_map: &HashMap<u32, Process>,
    services: &HashMap<u64, Service>,
    list: &gio::ListStore,
    app_icons: &HashMap<u32, LightCachedIcon>,
    icon: &LightCachedIcon,
    use_merged_stats: bool,
    section_type: SectionType,
) {
    let mut has_died = HashSet::new();
    let mut does_exist = HashSet::new();

    list.iter::<RowModel>().flatten().for_each(|row_model| {
        let service_id = row_model.service_id();
        if let Some(service) = services.get(&service_id) {
            update_service(
                process_map,
                &row_model,
                service,
                app_icons,
                icon,
                use_merged_stats,
            );

            does_exist.insert(service_id);
        } else {
            has_died.insert(service_id);
        }
    });

    list.retain(|object| {
        !has_died.contains(&object.downcast_ref::<RowModel>().unwrap().service_id())
    });

    let new_rows = services
        .values()
        .filter(|service| !does_exist.contains(&service.id))
        .map(|service| {
            let row_model = RowModelBuilder::new()
                .id(&service.id.to_string())
                .content_type(ContentType::Service)
                .section_type(section_type)
                .service_id(service.id)
                .name(&service.name)
                .file_path(&service.file_path())
                .user(&service.user.clone().unwrap_or("".to_string()))
                .group(&service.group.clone().unwrap_or("".to_string()))
                .light_cached_icon(LightCachedIcon::StringPayload(service_icon(service)))
                .build();

            update_service(
                process_map,
                &row_model,
                service,
                app_icons,
                icon,
                use_merged_stats,
            );

            row_model
        })
        .collect::<Vec<_>>();

    if !new_rows.is_empty() {
        list.splice(list.n_items(), 0, &new_rows);
    }
}

fn update_app(
    app: &App,
    process_map: &HashMap<u32, Process>,
    process_model_map: &HashMap<u32, RowModel>,
    app_icons: &mut HashMap<u32, LightCachedIcon>,
    row_model: RowModel,
) {
    let primary_processes = primary_processes(app, process_map);

    let list = row_model.children();

    // nothing to do/clear; it doesnt exist yet
    if primary_processes.is_empty() {
        list.remove_all();

        g_critical!(
            "MissionCenter::AppsPage",
            "Failed to find primary PID for app {}",
            app.name
        );
        return;
    }

    row_model
        .imp()
        .set_light_icon(LightCachedIcon::AppCachedKey(app.id.clone(), 24));

    let mut has_died = HashSet::new();
    let mut does_exist = HashSet::new();

    list.iter::<RowModel>().flatten().for_each(|row_model| {
        if primary_processes.contains(&row_model.pid()) {
            does_exist.insert(row_model.pid());
        } else {
            has_died.insert(row_model.pid());
        }
    });

    list.retain(|row_model| {
        row_model
            .downcast_ref::<RowModel>()
            .map(|rm| !has_died.contains(&rm.pid()))
            .unwrap_or(false)
    });

    let mut usage_stats = ProcessUsageStats::default();

    for process in primary_processes
        .iter()
        .filter_map(|pid| process_map.get(pid))
    {
        usage_stats.merge(&process.merged_usage_stats(&process_map));
        app_icons.insert(
            process.pid,
            LightCachedIcon::AppCachedKey(app.id.clone(), 16),
        );

        if !does_exist.contains(&process.pid) {
            if let Some(process_model) = process_model_map.get(&process.pid) {
                list.append(process_model);
            }
        }
    }

    set_stats(&row_model, &usage_stats);
}

fn update_process(
    process_map: &HashMap<u32, Process>,
    process: &Process,
    row_model: RowModel,
    app_icons: &HashMap<u32, LightCachedIcon>,
    icon: &LightCachedIcon,
    use_merged_stats: bool,
    section_type: SectionType,
    parent_service: Option<&Service>,
    model_map: &mut HashMap<u32, RowModel>,
) {
    let usage_stats = if use_merged_stats {
        &process.merged_usage_stats(&process_map)
    } else {
        &process.usage_stats
    };

    let icon = if let Some(icon) = app_icons.get(&process.pid) {
        icon
    } else {
        icon
    };

    row_model.imp().set_light_icon(icon.clone());

    set_stats(&row_model, usage_stats);
    if let Some(parent_service) = parent_service {
        set_service(&row_model, parent_service);
    }

    update_processes(
        process_map,
        process.children.clone().drain(..).collect(),
        &row_model.children(),
        app_icons,
        icon,
        use_merged_stats,
        section_type,
        parent_service,
        model_map,
    );

    model_map.insert(process.pid, row_model);
}

fn update_service(
    process_map: &HashMap<u32, Process>,
    row_model: &RowModel,
    service: &Service,
    app_icons: &HashMap<u32, LightCachedIcon>,
    icon: &LightCachedIcon,
    use_merged_stats: bool,
) {
    set_service(&row_model, service);

    row_model
        .imp()
        .set_light_icon(LightCachedIcon::StringPayload(service_icon(&service)));

    row_model.set_pid(service.pid.clone().unwrap_or_default());
    row_model.set_user(service.user.clone().unwrap_or_default());
    row_model.set_group(service.group.clone().unwrap_or_default());

    if let Some(pid) = service.pid {
        if let Some(process) = process_map.get(&pid) {
            let usage_stats = process.merged_usage_stats(&process_map);

            set_stats(&row_model, &usage_stats);
        } // else clear usage stats?

        let app_children = row_model.children();

        app_children.retain(|child| {
            child
                .downcast_ref::<RowModel>()
                .map(|rm| rm.pid() == pid)
                .unwrap_or(false)
        });

        update_processes(
            process_map,
            HashSet::from([pid]),
            &app_children,
            app_icons,
            icon,
            use_merged_stats,
            row_model.section_type(),
            Some(service),
            &mut HashMap::new(),
        );
    } else {
        row_model.children().remove_all();

        set_empty_stats(&row_model);
    }
}

#[inline]
fn set_empty_stats(row_model: &RowModel) {
    set_stats(&row_model, &Default::default());
}

fn set_stats(row_model: &RowModel, usage_stats: &ProcessUsageStats) {
    row_model.set_cpu_usage(usage_stats.cpu_usage);
    row_model.set_memory_usage(usage_stats.memory_usage);
    row_model.set_swap_usage(usage_stats.swap_usage);
    row_model.set_disk_usage(usage_stats.disk_usage);
    row_model.set_network_usage(usage_stats.network_usage);
    row_model.set_gpu_usage(usage_stats.gpu_usage);
    row_model.set_gpu_memory_usage(usage_stats.gpu_memory_usage);
}

fn service_icon(service: &Service) -> String {
    if service.running {
        "service-running".into()
    } else {
        if service.failed {
            "service-failed".into()
        } else if service.enabled {
            "service-stopped".into()
        } else {
            "service-disabled".into()
        }
    }
}

fn set_service(row_model: &RowModel, service: &Service) {
    row_model.set_service_running(service.running);
    row_model.set_service_enabled(service.enabled);
    row_model.set_service_failed(service.failed);
    row_model.set_service_stopped(!service.running && !service.failed && service.enabled);
}

fn primary_processes(app: &App, process_map: &HashMap<u32, Process>) -> HashSet<u32> {
    let mut secondary_processes = HashSet::new();
    for app_pid in app.pids.iter() {
        if let Some(process) = process_map.get(app_pid) {
            for child in &process.children {
                if app.pids.contains(child) {
                    secondary_processes.insert(*child);
                }
            }
        }
    }

    let mut primary_processes = HashSet::new();
    for app_pid in app.pids.iter() {
        if !secondary_processes.contains(&app_pid) {
            primary_processes.insert(*app_pid);
        }
    }

    if primary_processes.is_empty() {
        for (index, pid) in app.pids.iter().enumerate() {
            if let Some(process) = process_map.get(pid) {
                if process.children.len() > 0 || index == app.pids.len() - 1 {
                    primary_processes.insert(*pid);
                    break;
                }
            }
        }
    }

    primary_processes
}
