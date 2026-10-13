/* crates/mc-ui/src/dialogs.rs
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

//! Modal dialogs. Data they need from Magpie is requested when they open and arrives as
//! events; nothing here blocks.

use std::num::NonZeroU32;

use egui::{Context, Id, RichText};
use mc_core::format::{DataType, to_human_readable_nice};
use mc_core::i18n::i18n;
use mc_core::i18n::n_;
use mc_core::table::RowKind;
use mc_magpie::{About, ErrorEjectFailed, Event, MagpieClient, Request, SmartData};

use crate::cx::{Command, Cx, Dialog};
use crate::smart;
use crate::table::{TableView, row_icon};
use crate::win10::{dialog, palette, style};

#[derive(Default)]
pub struct Dialogs {
    open: Vec<Dialog>,
    about: Option<About>,
    service_logs: Option<(u64, String)>,
    smart: Option<(String, Option<SmartData>)>,
    eject: Option<(String, ErrorEjectFailed)>,
}

impl Dialogs {
    pub fn open(&mut self, dialog: Dialog, magpie: &MagpieClient) {
        match &dialog {
            Dialog::AboutSystem => {
                magpie.send(Request::AboutSystem);
            }
            Dialog::SmartData(disk_id) => {
                self.smart = None;
                magpie.send(Request::SmartData(disk_id.clone()));
                // Shown once data arrives
                return;
            }
            _ => {}
        }
        if !self.open.contains(&dialog) {
            self.open.push(dialog);
        }
    }

    fn request_logs(&mut self, magpie: &MagpieClient, service_id: u64, pid: u32) {
        self.service_logs = None;
        magpie.send(Request::ServiceLogs(service_id, NonZeroU32::new(pid)));
    }

    pub fn handle_event(&mut self, event: Event, _apps: &TableView, magpie: &MagpieClient) {
        match event {
            Event::About(about) => self.about = Some(about),
            Event::ServiceLogs { service_id, logs } => self.service_logs = Some((service_id, logs)),
            Event::SmartData { disk_id, data } => {
                self.smart = Some((disk_id.clone(), data));
                let dialog = Dialog::SmartData(disk_id);
                if !self.open.contains(&dialog) {
                    self.open.push(dialog);
                }
            }
            Event::EjectResult { disk_id, result } => match result {
                Ok(()) => self.eject = None,
                Err(e) => self.eject = Some((disk_id, e)),
            },
            Event::IconsReady(_)
            | Event::Fatal(_)
            | Event::HardwareAccess { .. }
            | Event::Spd { .. } => {}
        }
        let _ = magpie;
    }

    pub fn ui(&mut self, ctx: &Context, cx: &mut Cx, apps: &TableView, services: &TableView) {
        let mut close = vec![];
        for dialog in self.open.clone() {
            let keep = match &dialog {
                Dialog::KeyboardShortcuts => shortcuts(ctx),
                Dialog::About => about_app(ctx, cx),
                Dialog::AboutSystem => about_system(ctx, cx, self.about.as_ref()),
                Dialog::ProcessDetails(key) => {
                    process_details(ctx, cx, apps.node(key).or(services.node(key)))
                }
                Dialog::ServiceDetails(key) => self.service_details(ctx, cx, services, key),
                Dialog::SmartData(_) => match &self.smart {
                    Some((_, Some(data))) => smart::smart_dialog(ctx, data),
                    Some((_, None)) => smart::smart_failure(ctx),
                    None => true,
                },
            };
            if !keep {
                close.push(dialog);
            }
        }
        self.open.retain(|d| !close.contains(d));

        if let Some((disk_id, error)) = &self.eject {
            match smart::eject_failure(ctx, cx, disk_id, error, apps) {
                smart::EjectAction::None => {}
                smart::EjectAction::Close => self.eject = None,
                smart::EjectAction::Retry => {
                    cx.send(Command::Magpie(Request::EjectDisk(disk_id.clone())));
                }
                smart::EjectAction::Kill(pids) => {
                    cx.send(Command::Magpie(Request::KillProcesses(pids)));
                    cx.send(Command::Magpie(Request::EjectDisk(disk_id.clone())));
                }
            }
        }
    }

    fn service_details(
        &mut self,
        ctx: &Context,
        cx: &mut Cx,
        services: &TableView,
        key: &str,
    ) -> bool {
        let Some(node) = services.node(key) else {
            return false;
        };
        let Some(info) = node.service.clone() else {
            return false;
        };
        let wants_logs = match &self.service_logs {
            Some((id, _)) => *id != info.id,
            None => true,
        };
        if wants_logs
            && !ctx.memory(|m| {
                m.data
                    .get_temp::<bool>(Id::new(("logs-req", info.id)))
                    .unwrap_or(false)
            })
        {
            ctx.memory_mut(|m| m.data.insert_temp(Id::new(("logs-req", info.id)), true));
            cx.send(Command::Magpie(Request::ServiceLogs(
                info.id,
                NonZeroU32::new(node.pid),
            )));
        }

        let mut open = true;
        let snap = std::env::var_os("SNAP_CONTEXT").is_some();
        let response = egui::Modal::new(Id::new(("service-details", key))).show(ctx, |ui| {
            ui.set_width(420.);
            ui.vertical_centered(|ui| ui.heading(i18n("Service Details")));
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(i18n("State")).strong());
                    dialog::caption(ui, i18n("The current state of the service"));
                });
                if !snap {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("⟲")
                            .on_hover_text(i18n("Restart service"))
                            .clicked()
                        {
                            cx.send(Command::Magpie(Request::RestartService(info.id)));
                        }
                        if ui
                            .add_enabled(info.running, egui::Button::new("⏹"))
                            .on_hover_text(i18n("Stop service"))
                            .clicked()
                        {
                            cx.send(Command::Magpie(Request::StopService(info.id)));
                        }
                        if ui
                            .add_enabled(!info.running, egui::Button::new("▶"))
                            .on_hover_text(i18n("Start service"))
                            .clicked()
                        {
                            cx.send(Command::Magpie(Request::StartService(info.id)));
                        }
                    });
                }
            });
            let status = if info.running {
                i18n("Running")
            } else if info.failed {
                i18n("Failed")
            } else {
                i18n("Stopped")
            };
            egui::Grid::new("svc-state")
                .num_columns(2)
                .spacing([20., 8.])
                .show(ui, |ui| {
                    ui.label(i18n("Name"));
                    let response = ui.add(
                        egui::Label::new(
                            RichText::new(&node.name)
                                .color(palette::resolve(ui.ctx(), palette::LABEL)),
                        )
                        .truncate(),
                    );
                    crate::win10::paint::copy_value(ui, response.rect, &node.name);
                    ui.end_row();
                    ui.label(i18n("Description"));
                    let response = ui.add(
                        egui::Label::new(
                            RichText::new(&info.description)
                                .color(palette::resolve(ui.ctx(), palette::LABEL)),
                        )
                        .truncate(),
                    );
                    crate::win10::paint::copy_value(ui, response.rect, &info.description);
                    ui.end_row();
                    ui.label(i18n("Status"));
                    ui.label(
                        RichText::new(status).color(palette::resolve(ui.ctx(), palette::LABEL)),
                    );
                    ui.end_row();
                    ui.label(i18n("Enabled"));
                    let mut enabled = info.enabled;
                    ui.add_enabled_ui(!snap, |ui| {
                        if dialog::switch(ui, &mut enabled).changed() {
                            cx.send(Command::Magpie(if enabled {
                                Request::EnableService(info.id)
                            } else {
                                Request::DisableService(info.id)
                            }));
                        }
                    });
                    ui.end_row();
                });

            let na = i18n("N/A");
            let rows = [
                (
                    i18n("Process ID"),
                    if node.pid != 0 {
                        node.pid.to_string()
                    } else {
                        na.clone()
                    },
                ),
                (
                    i18n("User"),
                    if info.user.is_empty() {
                        na.clone()
                    } else {
                        info.user.clone()
                    },
                ),
                (
                    i18n("Group"),
                    if info.group.is_empty() {
                        na.clone()
                    } else {
                        info.group.clone()
                    },
                ),
                (
                    i18n("File Location"),
                    if info.file_path.is_empty() {
                        i18n("Unknown")
                    } else {
                        info.file_path.clone()
                    },
                ),
            ];
            let empty = node.pid == 0
                && info.user.is_empty()
                && info.group.is_empty()
                && info.file_path.is_empty();
            if !empty {
                ui.add_space(10.);
                ui.label(RichText::new(i18n("Process")).strong());
                dialog::caption(ui, i18n("Process and user information"));
                egui::Grid::new("svc-proc")
                    .num_columns(2)
                    .spacing([20., 8.])
                    .show(ui, |ui| {
                        for (l, v) in rows {
                            ui.label(l);
                            let response = ui.add(
                                egui::Label::new(
                                    RichText::new(&v)
                                        .color(palette::resolve(ui.ctx(), palette::LABEL)),
                                )
                                .truncate(),
                            );
                            crate::win10::paint::copy_value(ui, response.rect, &v);
                            ui.end_row();
                        }
                    });
            }

            if let Some((id, logs)) = &self.service_logs
                && *id == info.id
                && !logs.is_empty()
            {
                ui.add_space(10.);
                egui::CollapsingHeader::new(RichText::new(i18n("Logs")).strong())
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                if ui
                                    .button("🗐")
                                    .on_hover_text(i18n("Copy logs to clipboard"))
                                    .clicked()
                                {
                                    cx.send(Command::CopyText(logs.clone()));
                                }
                            });
                        });
                        egui::ScrollArea::vertical()
                            .max_height(260.)
                            .show(ui, |ui| {
                                ui.add(egui::Label::new(RichText::new(logs).monospace()).wrap());
                            });
                    });
            }
        });
        if response.should_close() {
            open = false;
            ctx.memory_mut(|m| m.data.remove::<bool>(Id::new(("logs-req", info.id))));
        }
        let _ = Self::request_logs;
        open
    }
}

pub fn fatal_dialog(ctx: &Context, message: &str, commands: &mut Vec<Command>) {
    egui::Modal::new(Id::new("fatal")).show(ctx, |ui| {
        ui.set_max_width(420.);
        ui.heading("A fatal error has occurred");
        ui.label(message);
        if ui
            .add(
                egui::Button::new(
                    RichText::new(mc_core::i18n::strip_mnemonic(&i18n("_Quit")))
                        .color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(0xe0, 0x1b, 0x24)),
            )
            .clicked()
        {
            commands.push(Command::Quit);
            std::process::exit(-1);
        }
    });
}

fn process_details(ctx: &Context, cx: &mut Cx, node: Option<&mc_core::table::Node>) -> bool {
    let Some(node) = node else {
        return false;
    };
    let h = |v: f32, t: DataType| to_human_readable_nice(v, &t);
    let s = &node.stats;
    let response = egui::Modal::new(Id::new(("process-details", &node.key))).show(ctx, |ui| {
        ui.set_width(380.);
        ui.horizontal(|ui| {
            row_icon(ui, cx, &node.icon, 48.);
            let response = ui.heading(&node.name);
            crate::win10::paint::copy_value(ui, response.rect, &node.name);
        });
        ui.add_space(8.);
        let kind = match node.kind {
            RowKind::SectionHeader => i18n("Section Header"),
            RowKind::Service => i18n("Service"),
            RowKind::App => i18n("App"),
            RowKind::Process => i18n("Process"),
        };
        let id_label = if node.kind == RowKind::App {
            i18n("App ID")
        } else {
            i18n("PID")
        };
        let id_value = match node.kind {
            RowKind::App => node.key.trim_start_matches("apps/a").to_owned(),
            _ => node.pid.to_string(),
        };
        egui::Grid::new("proc-details")
            .num_columns(2)
            .spacing([20., 8.])
            .show(ui, |ui| {
                ui.label(id_label);
                let response = ui.label(
                    RichText::new(&id_value).color(palette::resolve(ui.ctx(), palette::LABEL)),
                );
                crate::win10::paint::copy_value(ui, response.rect, &id_value);
                ui.end_row();
                ui.label(i18n("Type"));
                ui.label(RichText::new(kind).color(palette::resolve(ui.ctx(), palette::LABEL)));
                ui.end_row();
                if !node.command_line.is_empty() {
                    ui.label(i18n("Command Line"));
                    let response = ui.add(
                        egui::Label::new(
                            RichText::new(&node.command_line)
                                .color(palette::resolve(ui.ctx(), palette::LABEL)),
                        )
                        .wrap(),
                    );
                    crate::win10::paint::copy_value(ui, response.rect, &node.command_line);
                    ui.end_row();
                }
            });
        ui.add_space(10.);
        ui.label(RichText::new(i18n("Resource Usage")).strong());
        egui::Grid::new("proc-usage")
            .num_columns(2)
            .spacing([20., 6.])
            .show(ui, |ui| {
                for (label, value) in [
                    (i18n("CPU"), format!("{}%", s.cpu_usage.round() as u32)),
                    (
                        i18n("Memory"),
                        h(s.memory_usage as f32, DataType::MemoryBytes),
                    ),
                    (i18n("Swap"), h(s.swap_usage as f32, DataType::MemoryBytes)),
                    (
                        i18n("Drives"),
                        h(s.disk_usage, DataType::DriveBytesPerSecond),
                    ),
                    (
                        i18n("Network"),
                        h(s.network_usage, DataType::NetworkBytesPerSecond),
                    ),
                    (i18n("GPU"), node.gpu.usage.text()),
                    (i18n("GPU engine"), node.gpu.usage.engine_text()),
                    (
                        i18n("GPU Memory"),
                        node.gpu.memory.map_or_else(
                            || "—".into(),
                            |value| h(value as f32, DataType::MemoryBytes),
                        ),
                    ),
                ] {
                    ui.label(label);
                    ui.label(
                        RichText::new(value).color(palette::resolve(ui.ctx(), palette::LABEL)),
                    );
                    ui.end_row();
                }
            });
    });
    !response.should_close()
}

fn shortcuts(ctx: &Context) -> bool {
    let sections: [(&str, &[(&str, &str)]); 4] = [
        (
            n_("Global"),
            &[
                (n_("Open Menu"), "F10"),
                (n_("Close Window"), "Ctrl+Q, Ctrl+W"),
                (n_("Show Keyboard Shortcuts"), "Ctrl+?"),
                (n_("Show Performance Page"), "Alt+1, Ctrl+1"),
                (n_("Show Apps Page"), "Alt+2, Ctrl+2"),
                (n_("Show Services Page"), "Alt+3, Ctrl+3"),
                ("Show System Information", "Ctrl+A"),
                (n_("Toggle Sidebar"), "F9, Ctrl+T"),
                ("Pause Updates", "Ctrl"),
            ],
        ),
        (
            n_("Performance Page"),
            &[
                (n_("Copy Device Information"), "Ctrl+C"),
                (n_("Toggle Summary Mode"), "Ctrl+M"),
                (n_("Select a Device"), "F1...F8"),
            ],
        ),
        (
            n_("Apps Page"),
            &[
                (n_("Search"), "Ctrl+F"),
                (n_("Collapse All"), "Ctrl+L"),
                (n_("Stop App or Process"), "Ctrl+E"),
                (n_("Force Stop App or Process"), "Ctrl+X"),
                (n_("Show Details Dialog"), "Ctrl+I"),
            ],
        ),
        (
            n_("Services Page"),
            &[
                (n_("Search"), "Ctrl+F"),
                (n_("Collapse All"), "Ctrl+L"),
                (n_("Start Service"), "Ctrl+S"),
                (n_("Stop Service"), "Ctrl+E"),
                (n_("Restart Service"), "Ctrl+R"),
                (n_("Show Details Dialog"), "Ctrl+I"),
            ],
        ),
    ];
    let response = egui::Modal::new(Id::new("shortcuts")).show(ctx, |ui| {
        ui.set_width(460.);
        ui.vertical_centered(|ui| ui.heading(i18n("Keyboard Shortcuts")));
        egui::ScrollArea::vertical()
            .max_height(520.)
            .show(ui, |ui| {
                for (title, items) in sections {
                    ui.add_space(8.);
                    ui.label(RichText::new(i18n(title)).strong());
                    egui::Grid::new(title)
                        .num_columns(2)
                        .spacing([30., 6.])
                        .show(ui, |ui| {
                            for (label, keys) in items {
                                ui.vertical(|ui| {
                                    ui.label(i18n(label));
                                    if *label == "Pause Updates" {
                                        dialog::caption(ui, i18n("Hold to pause"));
                                    }
                                });
                                ui.label(RichText::new(*keys).monospace());
                                ui.end_row();
                            }
                        });
                }
            });
    });
    !response.should_close()
}

fn about_app(ctx: &Context, cx: &mut Cx) -> bool {
    let response = egui::Modal::new(Id::new("about-app")).show(ctx, |ui| {
        ui.set_width(400.);
        ui.vertical_centered(|ui| {
            if let Some(t) = cx.icons.app_logo(ui.ctx(), 96.) {
                ui.add(egui::Image::new((t.id(), egui::Vec2::splat(96.))));
            }
            ui.label(RichText::new(crate::APP_NAME).text_style(style::title1()));
            ui.label(RichText::new(crate::VERSION).color(palette::resolve(ui.ctx(), palette::BUTTON_HOVER_BORDER)));
            dialog::caption(ui, "GNU General Public License v3.0");
            let credits = i18n("translator-credits");
            if credits != n_("translator-credits") {
                dialog::caption(ui, credits);
            }

            // The statistics backend and much of the logic come from Mission Center
            ui.add_space(10.);
            ui.label(RichText::new(i18n("Based on Mission Center")).strong());
            dialog::caption(
                ui,
                i18n("Uses the Mission Center statistics backend (magpie) and code from the Mission Center project."),
            );
            dialog::caption(ui, "© 2023-2025 Mission Center Developers");
            dialog::caption(ui, "Romeo Calota, QwertyChouskie, jojo2357, Jan Luca");
            if ui.link("https://missioncenter.io").clicked() {
                cx.send(Command::OpenUrl("https://missioncenter.io".into()));
            }

            ui.add_space(8.);
            ui.label(RichText::new(i18n("Standing on the shoulders of giants")).strong());
            for line in [
                "egui https://github.com/emilk/egui",
                "NVTOP https://github.com/Syllo/nvtop",
                "Selawik (SIL OFL 1.1)",
                i18n("And many more... Thank you all!").as_str(),
            ] {
                dialog::caption(ui, line);
            }
        });
    });
    !response.should_close()
}

fn about_system(ctx: &Context, cx: &mut Cx, about: Option<&About>) -> bool {
    let response = egui::Modal::new(Id::new("about-system")).show(ctx, |ui| {
        ui.set_width(460.);
        let Some(about) = about else {
            ui.vertical_centered(|ui| ui.spinner());
            return;
        };
        let groups = crate::smart::about_groups(about);
        ui.horizontal(|ui| {
            ui.heading(i18n("System Information"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🗐").on_hover_text(i18n("Copy All")).clicked() {
                    cx.send(Command::CopyText(crate::smart::about_copy_all(&groups)));
                }
            });
        });
        for (title, rows) in &groups {
            if rows.iter().all(|(_, v)| v.is_none()) {
                continue;
            }
            ui.add_space(10.);
            ui.label(RichText::new(title).strong());
            for (label, value) in rows {
                let Some(value) = value else { continue };
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let response = ui.add(
                            egui::Label::new(
                                RichText::new(value)
                                    .color(palette::resolve(ui.ctx(), palette::LABEL)),
                            )
                            .truncate(),
                        );
                        crate::win10::paint::copy_value(ui, response.rect, value);
                    });
                });
            }
        }
    });
    !response.should_close()
}
