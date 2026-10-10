/* crates/mc-ui/src/prefs.rs
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

//! Preferences dialog: General, Performance Page, Apps & Services, Units.

use egui::{Context, Id, Ui};
use mc_core::i18n::i18n;
use mc_core::i18n::n_;
use mc_magpie::INTERVAL_STEP;

use crate::cx::Cx;
use crate::win10::dialog;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PrefPage {
    #[default]
    General,
    Performance,
    AppsServices,
    Units,
}

fn switch_row(ui: &mut Ui, title: &str, subtitle: Option<&str>, value: &mut bool) {
    dialog::row(ui, &i18n(title), subtitle.map(i18n).as_deref(), |ui| {
        dialog::switch(ui, value);
    });
}

fn general(ui: &mut Ui, cx: &mut Cx) {
    dialog::group(ui, None, None, |ui| {
        let mut seconds = cx.config.app_update_interval as f64 * INTERVAL_STEP;
        dialog::row(
            ui,
            &i18n("Update Interval"),
            Some(&i18n("In seconds")),
            |ui| {
                let r = ui.add(
                    egui::DragValue::new(&mut seconds)
                        .range(0.5..=10.0)
                        .speed(0.05)
                        .fixed_decimals(2),
                );
                if r.changed() {
                    let ticks = (seconds / INTERVAL_STEP).round() as u64;
                    if (10..=200).contains(&ticks) {
                        cx.config.app_update_interval = ticks;
                    } else {
                        log::warn!(target: "TaskManager::Preferences", "Update interval out of bounds");
                    }
                }
            },
        );
    });
}

fn performance(ui: &mut Ui, cx: &mut Cx) {
    dialog::group(ui, Some(&i18n("Graph Settings")), None, |ui| {
        let mut points = cx.config.performance_page_data_points;
        dialog::row(ui, &i18n("Data Points"), None, |ui| {
            ui.add(egui::Slider::new(&mut points, 10..=600));
        });
        cx.config.performance_page_data_points = points;
        let mut smooth = cx.config.performance_smooth_graphs;
        switch_row(
            ui,
            n_("Smooth Graphs"),
            Some(n_("Draw graphs as smooth line instead of jagged line")),
            &mut smooth,
        );
        cx.config.performance_smooth_graphs = smooth;
    });
    performance_rest(ui, cx);
}

fn performance_rest(ui: &mut Ui, cx: &mut Cx) {
    let mut v = cx.config.clone();
    dialog::group(ui, None, None, |ui| {
        switch_row(
            ui,
            n_("Sliding Graphs"),
            Some(n_("Animate graphs (may negatively impact performance)")),
            &mut v.performance_sliding_graphs,
        );
        switch_row(
            ui,
            n_("Scale Network Graphs Dynamically"),
            Some(n_(
                "Set network graph to scale dynamically with actual usage or with channel max speed",
            )),
            &mut v.performance_page_network_dynamic_scaling,
        );
    });
    dialog::group(ui, Some(&i18n("Devices")), None, |ui| {
        switch_row(
            ui,
            n_("Show Disk Information"),
            Some(n_("Show disk information by default")),
            &mut v.performance_show_disks,
        );
        switch_row(
            ui,
            n_("Show Network Information"),
            Some(n_("Show network information by default")),
            &mut v.performance_show_network,
        );
        if v.performance_show_network {
            ui.indent("net-kinds", |ui| {
                switch_row(ui, n_("Wired"), None, &mut v.performance_show_network_wired);
                switch_row(
                    ui,
                    n_("Wireless"),
                    None,
                    &mut v.performance_show_network_wireless,
                );
                switch_row(ui, n_("VPN"), None, &mut v.performance_show_network_vpn);
                switch_row(
                    ui,
                    n_("Virtual"),
                    Some(n_("Docker, Multipass, Bridge")),
                    &mut v.performance_show_network_virtual,
                );
                switch_row(
                    ui,
                    n_("Other"),
                    Some(n_("Bluetooth, InfiniBand, WWAN")),
                    &mut v.performance_show_network_other,
                );
            });
        }
        switch_row(
            ui,
            n_("Show GPU Information"),
            Some(n_("Show GPU information by default")),
            &mut v.performance_show_gpus,
        );
        switch_row(
            ui,
            n_("Show Fan Information"),
            Some(n_("Show fan information by default")),
            &mut v.performance_show_fans,
        );
        switch_row(
            ui,
            n_("Show Battery Information"),
            Some(n_("Show battery information by default")),
            &mut v.performance_show_batteries,
        );
    });
    *cx.config = v;
}

fn apps_services(ui: &mut Ui, cx: &mut Cx) {
    let mut v = cx.config.clone();
    dialog::group(ui, None, None, |ui| {
        switch_row(
            ui,
            n_("Merge Process Stats"),
            Some(n_(
                "Parent and child process stats are shown individually or merged upwards",
            )),
            &mut v.apps_page_merged_process_stats,
        );
        switch_row(
            ui,
            n_("Remember Sorting"),
            Some(n_("Persist sorting by column across app restarts")),
            &mut v.apps_page_remember_sorting,
        );
        switch_row(
            ui,
            n_("Remember Column Order"),
            Some(n_("Persist the order of the columns, across app restarts")),
            &mut v.apps_page_remember_column_order,
        );
        switch_row(
            ui,
            n_("Scale CPU Usage to Core Count"),
            Some(n_("Show CPU usage scaled to the number of available cores")),
            &mut v.apps_page_core_count_affects_percentages,
        );
        switch_row(
            ui,
            n_("Show Column Separators"),
            Some(n_("Show a separator between each column")),
            &mut v.apps_page_show_column_separators,
        );
        switch_row(
            ui,
            n_("Grey Out Zeroes"),
            Some(n_("Gray out values that equal zero")),
            &mut v.apps_page_hide_is_zero,
        );
    });
    *cx.config = v;
}

fn unit_pair(
    ui: &mut Ui,
    title: &str,
    options: [(&str, &str); 2],
    truthy_first: bool,
    value: &mut bool,
) {
    let opts = [
        (i18n(options[0].0), i18n(options[0].1)),
        (i18n(options[1].0), i18n(options[1].1)),
    ];
    let refs: Vec<(&str, &str)> = opts.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let mut selected = if *value == truthy_first { 0 } else { 1 };
    dialog::row(ui, &i18n(title), None, |ui| {
        if dialog::toggle_group(ui, &refs, &mut selected) {
            *value = (selected == 0) == truthy_first;
        }
    });
}

fn units(ui: &mut Ui, cx: &mut Cx) {
    let mut v = cx.config.clone();
    dialog::group(ui, None, None, |ui| {
        unit_pair(
            ui,
            n_("Memory Usage Unit"),
            [
                (
                    n_("Bytes"),
                    n_("Use Bytes as a Unit of Measurement for Memory"),
                ),
                (
                    n_("Bits"),
                    n_("Use Bits as a Unit of Measurement for Memory"),
                ),
            ],
            true,
            &mut v.performance_page_memory2_use_bytes,
        );
        unit_pair(
            ui,
            n_("Memory Usage Base"),
            [
                (n_("Base 10"), n_("Use Base 10 for Memory")),
                (n_("Base 2"), n_("Use Base 2 for Memory")),
            ],
            false,
            &mut v.performance_page_memory2_use_base2,
        );
        unit_pair(
            ui,
            n_("Drive Usage Unit"),
            [
                (
                    n_("Bytes"),
                    n_("Use Bytes as a Unit of Measurement for Disk Drives"),
                ),
                (
                    n_("Bits"),
                    n_("Use Bits as a Unit of Measurement for Disk Drives"),
                ),
            ],
            true,
            &mut v.performance_page_drive_use_bytes,
        );
        unit_pair(
            ui,
            n_("Drive Usage Base"),
            [
                (n_("Base 10"), n_("Use Base 10 for Disk Drives")),
                (n_("Base 2"), n_("Use Base 2 for Disk Drives")),
            ],
            false,
            &mut v.performance_page_drive_use_base2,
        );
        unit_pair(
            ui,
            n_("Network Usage Unit"),
            [
                (
                    n_("Bytes"),
                    n_("Use Bytes as a Unit of Measurement for Network Interfaces"),
                ),
                (
                    n_("Bits"),
                    n_("Use Bits as a Unit of Measurement for Network Interfaces"),
                ),
            ],
            true,
            &mut v.performance_page_network_use_bytes,
        );
        unit_pair(
            ui,
            n_("Network Usage Base"),
            [
                (n_("Base 10"), n_("Use Base 10 for Network Interfaces")),
                (n_("Base 2"), n_("Use Base 2 for Network Interfaces")),
            ],
            false,
            &mut v.performance_page_network_use_base2,
        );
    });
    *cx.config = v;
}

/// Returns `false` once closed.
pub fn preferences(ctx: &Context, cx: &mut Cx) -> bool {
    let id = Id::new("preferences-page");
    let mut page: PrefPage = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let response = egui::Modal::new(Id::new("preferences")).show(ctx, |ui| {
        ui.set_width(560.);
        ui.horizontal(|ui| {
            for (p, label) in [
                (PrefPage::General, n_("General")),
                (PrefPage::Performance, n_("Performance Page")),
                (PrefPage::AppsServices, n_("Apps & Services")),
                (PrefPage::Units, n_("Units")),
            ] {
                if ui.selectable_label(page == p, i18n(label)).clicked() {
                    page = p;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(520.)
            .show(ui, |ui| match page {
                PrefPage::General => general(ui, cx),
                PrefPage::Performance => performance(ui, cx),
                PrefPage::AppsServices => apps_services(ui, cx),
                PrefPage::Units => units(ui, cx),
            });
    });
    ctx.data_mut(|d| d.insert_temp(id, page));
    !response.should_close()
}
