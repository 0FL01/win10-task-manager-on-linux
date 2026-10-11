/* crates/mc-ui/src/smart.rs
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

//! Disk dialogs (SMART data, SMART failure, eject failure) and System Information helpers.

use std::collections::BTreeMap;

use egui::{Context, Id, RichText};
use egui_extras::{Column, TableBuilder};
use mc_core::format::{DataType, to_human_readable_nice, to_long_human_readable_time};
use mc_core::i18n::n_;
use mc_core::i18n::{i18n, i18n_f};
use mc_core::table::RowKind;
use mc_magpie::magpie_types::disks::smart_data::{self, ata::PrettyUnit};
use mc_magpie::{About, ErrorEjectFailed, SmartData};

use crate::cx::Cx;
use crate::perf::MK_TO_0_C;
use crate::table::{TableView, row_icon};
use crate::win10::palette;

fn nvme_rows(data: &smart_data::Nvme) -> Vec<(String, String)> {
    let na = i18n("N/A");
    let o = |v: Option<String>| v.unwrap_or_else(|| na.clone());
    vec![
        (
            i18n("Percent Used"),
            o(data.percent_used.map(|v| format!("{v}%"))),
        ),
        (
            i18n("Available Spare"),
            o(data.avail_spare.map(|v| v.to_string())),
        ),
        (
            i18n("Spare Threshold"),
            o(data.spare_thresh.map(|v| v.to_string())),
        ),
        (
            i18n("Total Data Read"),
            o(data
                .total_data_read
                .map(|v| to_human_readable_nice(v as f32, &DataType::DriveBytes))),
        ),
        (
            i18n("Total Data Written"),
            o(data
                .total_data_written
                .map(|v| to_human_readable_nice(v as f32, &DataType::DriveBytes))),
        ),
        (
            i18n("Warning Temp Time"),
            o(data
                .warn_composite_temp_time
                .map(|v| to_long_human_readable_time(v as u64))),
        ),
        (
            i18n("Critical Temp Time"),
            o(data
                .crit_composite_temp_time
                .map(|v| to_long_human_readable_time(v as u64))),
        ),
        (
            i18n("Control Busy Time"),
            o(data.ctrl_busy_minutes.map(to_long_human_readable_time)),
        ),
        (
            i18n("Warning Temperature"),
            o(data
                .warn_composite_temp_thresh
                .map(|v| i18n_f("{} °C", &[&(v as i64 - 273).to_string()]))),
        ),
        (
            i18n("Critical Temperature"),
            o(data
                .crit_composite_temp_thresh
                .map(|v| i18n_f("{} °C", &[&(v as i64 - 273).to_string()]))),
        ),
        (i18n("Temps"), format!("{:?}", data.temp_sensors)),
        (
            i18n("Unsafe Shutdowns"),
            o(data.unsafe_shutdowns.map(|v| v.to_string())),
        ),
        (
            i18n("Media Errors"),
            o(data.media_errors.map(|v| v.to_string())),
        ),
        (
            i18n("Number Error Log Entries"),
            o(data.num_err_log_entries.map(|v| v.to_string())),
        ),
        (
            i18n("Power Cycles"),
            o(data.power_cycles.map(|v| v.to_string())),
        ),
    ]
}

fn ata_value(pretty: i64, unit: PrettyUnit) -> String {
    match unit {
        PrettyUnit::Unknown => i18n("N/A"),
        PrettyUnit::Milliseconds => to_long_human_readable_time(pretty as u64 / 1000),
        PrettyUnit::Sectors => i18n_f("{} sectors", &[&pretty.to_string()]),
        PrettyUnit::Millikelvin => i18n_f(
            "{} °C",
            &[&((pretty as i32 + MK_TO_0_C) / 1000).to_string()],
        ),
        PrettyUnit::Dimentionless => pretty.to_string(),
    }
}

/// Returns `false` once closed.
pub fn smart_dialog(ctx: &Context, data: &SmartData) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs());
    let last_updated = match now {
        Ok(now) => i18n_f(
            n_("{} ago"),
            &[&to_long_human_readable_time(
                now.saturating_sub(data.last_update_time),
            )],
        ),
        Err(_) => i18n("Unknown"),
    };
    let powered_on = to_long_human_readable_time(data.powered_on_seconds);
    let status = format!("{:?}", data.test_result());

    let response = egui::Modal::new(Id::new("smart-data")).show(ctx, |ui| {
        ui.set_width(720.);
        ui.vertical_centered(|ui| ui.heading(i18n("Smart Data")));
        egui::Grid::new("smart-common")
            .num_columns(4)
            .spacing([12., 6.])
            .show(ui, |ui| {
                ui.label(i18n("Last Updated:"));
                ui.label(&last_updated);
                ui.label(i18n("Powered On:"));
                ui.label(&powered_on);
                ui.end_row();
                ui.label(i18n("Drive Health:"));
                ui.label("");
                ui.label(i18n("SMART Test Status:"));
                ui.label(&status);
                ui.end_row();
            });
        ui.add_space(10.);
        match &data.data {
            Some(smart_data::Data::Nvme(nvme)) => {
                let rows = nvme_rows(nvme);
                TableBuilder::new(ui)
                    .id_salt("smart-nvme")
                    .column(Column::remainder())
                    .column(Column::auto().at_least(160.))
                    .max_scroll_height(420.)
                    .header(24., |mut h| {
                        h.col(|ui| {
                            ui.strong(i18n("Attribute"));
                        });
                        h.col(|ui| {
                            ui.strong(i18n("Value"));
                        });
                    })
                    .body(|body| {
                        body.rows(22., rows.len(), |mut row| {
                            let (l, v) = &rows[row.index()];
                            row.col(|ui| {
                                ui.label(l);
                            });
                            row.col(|ui| {
                                ui.label(v);
                            });
                        });
                    });
            }
            Some(smart_data::Data::Ata(ata)) => {
                let headers = [
                    n_("ID"),
                    n_("Attribute"),
                    n_("Value"),
                    n_("Normalized"),
                    n_("Threshold"),
                    n_("Worst"),
                    n_("Type"),
                    n_("Updates"),
                    n_("Assessment"),
                ];
                let mut builder = TableBuilder::new(ui)
                    .id_salt("smart-ata")
                    .max_scroll_height(420.);
                for (i, _) in headers.iter().enumerate() {
                    builder = builder.column(if i == 1 {
                        Column::remainder().at_least(160.)
                    } else {
                        Column::auto()
                    });
                }
                builder
                    .header(24., |mut h| {
                        for t in headers {
                            h.col(|ui| {
                                ui.strong(i18n(t));
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(22., ata.attributes.len(), |mut row| {
                            let a = &ata.attributes[row.index()];
                            let unit =
                                PrettyUnit::try_from(a.pretty_unit).unwrap_or(PrettyUnit::Unknown);
                            let kind = if a.flags & 0b1 == 1 {
                                i18n("Pre-Fail")
                            } else {
                                i18n("Old-Age")
                            };
                            // Same expression as upstream (precedence makes it `flags & 1`)
                            let updates = if a.flags & (0b10 >> 1) == 0 {
                                i18n("Online")
                            } else {
                                i18n("Offline")
                            };
                            let assessment =
                                if a.value > 0 && a.threshold > 0 && a.value <= a.threshold {
                                    i18n("FAILING")
                                } else if a.worst > 0 && a.threshold > 0 && a.worst <= a.threshold {
                                    i18n("Failed in the past")
                                } else {
                                    i18n("Ok")
                                };
                            for text in [
                                a.id.to_string(),
                                a.name.clone(),
                                ata_value(a.pretty, unit),
                                a.value.to_string(),
                                a.threshold.to_string(),
                                a.worst.to_string(),
                                kind,
                                updates,
                                assessment,
                            ] {
                                row.col(|ui| {
                                    ui.label(text);
                                });
                            }
                        });
                    });
            }
            None => {}
        }
    });
    !response.should_close()
}

pub fn smart_failure(ctx: &Context) -> bool {
    let mut keep = true;
    egui::Modal::new(Id::new("smart-failure")).show(ctx, |ui| {
        ui.set_max_width(400.);
        ui.heading(i18n("Failed To Get SMART Data"));
        ui.label(i18n(
            n_("There was a problem fetching SMART data. Try running from the command line to get more information."),
        ));
        if ui.button(i18n("Close")).clicked() {
            keep = false;
        }
    });
    keep
}

pub enum EjectAction {
    None,
    Close,
    Retry,
    Kill(Vec<u32>),
}

pub fn eject_failure(
    ctx: &Context,
    cx: &mut Cx,
    _disk_id: &str,
    error: &ErrorEjectFailed,
    apps: &TableView,
) -> EjectAction {
    let mut action = EjectAction::None;
    // Group blockers by owning app
    type Blockers = Vec<(u32, Vec<String>)>;
    let mut groups: BTreeMap<String, (Option<mc_core::table::RowIcon>, Blockers)> = BTreeMap::new();
    for blocker in &error.blockers {
        let app = apps
            .tree()
            .nodes
            .iter()
            .find(|n| n.kind == RowKind::App && n.app_pids.contains(&blocker.pid));
        let entry = groups
            .entry(app.map(|a| a.name.clone()).unwrap_or_default())
            .or_insert((app.map(|a| a.icon.clone()), vec![]));
        if !blocker.files.is_empty() {
            entry.1.push((blocker.pid, blocker.files.clone()));
        }
        if !blocker.dirs.is_empty() {
            entry.1.push((blocker.pid, blocker.dirs.clone()));
        }
    }

    egui::Modal::new(Id::new("eject-failure")).show(ctx, |ui| {
        ui.set_width(520.);
        ui.heading(i18n("Failed To Eject"));
        egui::ScrollArea::vertical()
            .max_height(360.)
            .show(ui, |ui| {
                for (name, (icon, rows)) in &groups {
                    for (pid, files) in rows {
                        ui.horizontal(|ui| {
                            if let Some(icon) = icon {
                                row_icon(ui, cx, icon, 48.);
                            }
                            ui.add_sized([50., 20.], egui::Label::new(pid.to_string()));
                            ui.add_sized([100., 20.], egui::Label::new(name));
                            ui.add(egui::Label::new(files.join("\n")).wrap());
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(i18n("Kill")).color(egui::Color32::WHITE),
                                    )
                                    .fill(palette::SUMMARY_ALERT),
                                )
                                .clicked()
                            {
                                action = EjectAction::Kill(vec![*pid]);
                            }
                        });
                        ui.separator();
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui.button(i18n("Close")).clicked() {
                action = EjectAction::Close;
            }
            if ui.button(i18n("Retry")).clicked() {
                action = EjectAction::Retry;
            }
            if ui
                .add(
                    egui::Button::new(RichText::new(i18n("Kill All")).color(egui::Color32::WHITE))
                        .fill(palette::SUMMARY_ALERT),
                )
                .clicked()
            {
                action = EjectAction::Kill(error.blockers.iter().map(|b| b.pid).collect());
            }
        });
    });
    action
}

pub type AboutGroups = Vec<(String, Vec<(String, Option<String>)>)>;

pub fn about_groups(about: &About) -> AboutGroups {
    let os = &about.os_info;
    let de = &about.de_info;
    let kernel_release = os.kernel_release.as_ref().map(|r| {
        format!(
            "{} {r}",
            os.os_type.clone().unwrap_or_else(|| n_("Unknown").into())
        )
    });
    vec![
        (
            i18n("Operating System Information"),
            vec![
                (i18n("Name"), os.pretty_name.clone().or(os.name.clone())),
                (
                    i18n("Version"),
                    os.version_id.clone().or(os.version.clone()),
                ),
                (i18n("Package Manager"), os.package_manager.clone()),
                (
                    i18n("Package Manager Version"),
                    os.package_manager_version.clone(),
                ),
            ],
        ),
        (
            i18n("Kernel Information"),
            vec![
                (i18n("Kernel Release"), kernel_release),
                (i18n("Version"), os.kernel_version.clone()),
            ],
        ),
        (
            i18n("Desktop Environment Information"),
            vec![
                (i18n("Name"), de.desktop_environment.clone()),
                (i18n("Version"), de.version.clone()),
                (i18n("Windowing System"), de.windowing_system.clone()),
                (i18n("Virtual Terminal"), de.virtual_terminal.clone()),
            ],
        ),
    ]
}

pub fn about_copy_all(groups: &AboutGroups) -> String {
    let v = |g: usize, r: usize| groups[g].1[r].1.clone().unwrap_or_default();
    format!(
        r#"System Information:
    Operating System Information
    Name:                    {}
    Version:                 {}
    Package Manager:         {}
    Package Manager Version: {}

    Kernel Information
    Kernel Release:          {}
    Version:                 {}

    Desktop Environment Information
    Name:                    {}
    Version:                 {}
    Windowing System:        {}
    Virtual Terminal:        {}"#,
        v(0, 0),
        v(0, 1),
        v(0, 2),
        v(0, 3),
        v(1, 0),
        v(1, 1),
        v(2, 0),
        v(2, 1),
        v(2, 2),
        v(2, 3),
    )
}
