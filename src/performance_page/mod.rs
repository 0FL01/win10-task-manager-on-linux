/* performance_page/view_models
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

use std::fmt::Write;
use std::marker::PhantomData;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
};

use adw::{prelude::*, subclass::prelude::*};
use arrayvec::ArrayString;
use glib::{ParamSpec, Properties, Value};
use gtk::{
    gdk, gio,
    glib::{self, g_critical, g_warning},
};

use magpie_types::battery::Battery;
use magpie_types::fan::Fan;
use magpie_types::gpus::Gpu;
use magpie_types::network::{Connection, ConnectionKind, ConnectionState};

use crate::i18n::*;
use crate::magpie_client::DiskKind;
use crate::performance_page::widgets::{
    AnimationFrame, DatasetGroup, FillingSettings, GraphWidget, GraphWidgetSettingsExt,
    RoundingSettings, ScalingSettings, SidebarDropHint,
};
use crate::widgets::Placeholder;
use crate::{settings, DataType};

use summary_graph::{
    decode_device_name, encode_device_name, parse_device_overrides, resolve_device_visibility,
    serialize_device_overrides, DeviceOverride, DeviceType, NetworkGroup,
};

mod battery;
mod cpu;
mod disk;
mod disk_details;
mod fan;
mod gpu;
mod gpu_details;
mod memory;
mod network;
mod summary_graph;
pub mod widgets;

type SummaryGraph = summary_graph::SummaryGraph;
type BatteryPage = battery::PerformancePageBattery;
type CpuPage = cpu::PerformancePageCpu;
type DiskPage = disk::PerformancePageDisk;
type MemoryPage = memory::PerformancePageMemory;
type NetworkPage = network::PerformancePageNetwork;
type GpuPage = gpu::PerformancePageGpu;
type GpuDetails = gpu_details::GpuDetails;
type FanPage = fan::PerformancePageFan;

trait PageExt {
    fn infobar_collapsed(&self);
    fn infobar_uncollapsed(&self);
}

const MK_TO_0_C: i32 = -273150;

const MAX_REMEMBERED_DEVICES: usize = 256;

fn prune_sidebar_order(order: &mut Vec<String>, present: &HashSet<String>, max: usize) {
    let mut excess = order.len().saturating_sub(max);
    let mut index = order.len();

    while excess > 0 && index > 0 {
        index -= 1;

        if !present.contains(&order[index]) {
            order.remove(index);
            excess -= 1;
        }
    }
}

mod imp {
    use super::*;

    // GNOME color palette: Blue 4
    const CPU_BASE_COLOR: [u8; 3] = [0x1c, 0x71, 0xd8];
    // GNOME color palette: Blue 2
    const MEMORY_BASE_COLOR: [u8; 3] = [0x62, 0xa0, 0xea];
    // GNOME color palette: Orange 2
    const DISK_BASE_COLOR: [u8; 3] = [0x26, 0xa2, 0x69];
    // GNOME color palette: Purple 1
    const NETWORK_BASE_COLOR: [u8; 3] = [0xdc, 0x8a, 0xdd];
    // GNOME color palette: Purple 4
    const FAN_BASE_COLOR: [u8; 3] = [0x81, 0x3d, 0x9c];
    // GNOME color palette: Red 1
    const GPU_BASE_COLOR: [u8; 3] = [0xf6, 0x61, 0x51];
    // GNOME color palette: Green 2
    const BATTERY_BASE_COLOR: [u8; 3] = [0x57, 0xe3, 0x89];

    enum Pages {
        Cpu((SummaryGraph, CpuPage)),
        Memory((SummaryGraph, MemoryPage)),
        Disk(HashMap<String, (SummaryGraph, DiskPage)>),
        Network(HashMap<String, (SummaryGraph, NetworkPage)>),
        Gpu(HashMap<String, (SummaryGraph, GpuPage)>),
        Fan(HashMap<String, (SummaryGraph, FanPage)>),
        Battery(HashMap<String, (SummaryGraph, BatteryPage)>),
    }

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePage)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/page.ui")]
    pub struct PerformancePage {
        #[template_child]
        pub breakpoint: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub page_content: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub page_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub info_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub info_bar: TemplateChild<adw::Bin>,

        #[property(get = Self::sidebar, set = Self::set_sidebar)]
        pub sidebar: RefCell<gtk::ListBox>,
        #[property(get, set = Self::set_sidebar_edit_mode)]
        pub sidebar_edit_mode: Cell<bool>,
        #[property(get, set)]
        summary_mode: Cell<bool>,
        #[property(name = "infobar-visible", get = Self::infobar_visible, set = Self::set_infobar_visible)]
        _infobar_visible: PhantomData<bool>,
        #[property(name = "info-button-visible", get = Self::info_button_visible)]
        _info_button_visible: PhantomData<bool>,

        breakpoint_applied: Cell<bool>,

        pages: Cell<Vec<Pages>>,
        pub summary_graphs: Cell<HashMap<SummaryGraph, gtk::DragSource>>,

        sidebar_rank: RefCell<HashMap<String, usize>>,
        rebuilding: Cell<bool>,

        context_menu_view_actions: Cell<HashMap<String, gio::SimpleAction>>,
        current_view_action: Cell<gio::SimpleAction>,
    }

    impl Default for PerformancePage {
        fn default() -> Self {
            Self {
                breakpoint: Default::default(),
                page_content: Default::default(),
                content_stack: Default::default(),
                page_stack: Default::default(),
                info_stack: Default::default(),
                info_bar: Default::default(),

                sidebar: RefCell::new(gtk::ListBox::new()),
                sidebar_edit_mode: Cell::new(false),
                summary_mode: Cell::new(false),
                _infobar_visible: PhantomData,
                _info_button_visible: PhantomData,

                breakpoint_applied: Cell::new(false),

                pages: Cell::new(Vec::new()),
                summary_graphs: Cell::new(HashMap::new()),

                sidebar_rank: RefCell::new(HashMap::new()),
                rebuilding: Cell::new(false),

                context_menu_view_actions: Cell::new(HashMap::new()),
                current_view_action: Cell::new(gio::SimpleAction::new("", None)),
            }
        }
    }

    impl PerformancePage {
        pub fn sidebar(&self) -> gtk::ListBox {
            self.sidebar.borrow().clone()
        }

        fn canonical_key(name: &str) -> (u8, &str) {
            const CATEGORIES: [&str; 7] = ["cpu", "memory", "disk", "net", "gpu", "fan", "battery"];

            let category = CATEGORIES
                .iter()
                .position(|prefix| name.starts_with(prefix))
                .unwrap_or(CATEGORIES.len()) as u8;

            (category, name)
        }

        fn view_action_name(page_name: &str) -> Option<&'static str> {
            Some(match page_name.split('-').next().unwrap_or_default() {
                "cpu" => "cpu",
                "memory" => "memory",
                "disk" => "disk",
                "net" => "network",
                "gpu" => "gpu",
                "fan" => "fan",
                "battery" => "battery",
                _ => return None,
            })
        }

        fn device_visible(&self, graph: &SummaryGraph) -> bool {
            let settings = settings!();

            let category_visible = match graph.device_type() {
                DeviceType::Disk => settings.boolean("performance-show-disks"),
                DeviceType::Network(group) => {
                    settings.boolean("performance-show-network")
                        && settings.boolean(group.settings_key())
                }
                DeviceType::Gpu => settings.boolean("performance-show-gpus"),
                DeviceType::Fan => settings.boolean("performance-show-fans"),
                DeviceType::Battery => settings.boolean("performance-show-batteries"),
                DeviceType::Cpu | DeviceType::Memory | DeviceType::Unspecified => true,
            };

            let overrides =
                parse_device_overrides(&settings.string("performance-sidebar-device-overrides"));

            resolve_device_visibility(graph.widget_name().as_str(), &overrides, category_visible)
        }

        fn graph_shown(&self, graph: &SummaryGraph) -> bool {
            self.sidebar_edit_mode.get() || self.device_visible(graph)
        }

        pub(super) fn row_shown(&self, row: &gtk::ListBoxRow) -> bool {
            match row
                .child()
                .and_then(|child| child.downcast::<SummaryGraph>().ok())
            {
                Some(graph) => self.graph_shown(&graph),
                None => true,
            }
        }

        fn load_sidebar_order(settings: &gio::Settings) -> Vec<String> {
            settings
                .string("performance-sidebar-order")
                .split(';')
                .filter(|entry| !entry.is_empty())
                .map(decode_device_name)
                .collect()
        }

        fn store_sidebar_order(settings: &gio::Settings, order: &[String]) {
            let encoded = order
                .iter()
                .map(|name| encode_device_name(name))
                .collect::<Vec<_>>()
                .join(";");

            if encoded.as_str() == settings.string("performance-sidebar-order").as_str() {
                return;
            }

            settings
                .set_string("performance-sidebar-order", &encoded)
                .unwrap_or_else(|_| {
                    g_warning!(
                        "MissionCenter::PerformancePage",
                        "Failed to set performance-sidebar-order setting"
                    );
                });
        }

        fn merge_new_devices(order: &mut Vec<String>, mut present: Vec<String>) {
            present.sort_unstable_by(|a, b| Self::canonical_key(a).cmp(&Self::canonical_key(b)));

            for name in present {
                if order.iter().any(|known| known == &name) {
                    continue;
                }

                let key = Self::canonical_key(&name);
                let at = order
                    .iter()
                    .rposition(|known| {
                        let known_key = Self::canonical_key(known);
                        known_key.0 == key.0 && known_key.1 < key.1
                    })
                    .or_else(|| {
                        order
                            .iter()
                            .rposition(|known| Self::canonical_key(known).0 == key.0)
                    })
                    .or_else(|| {
                        order
                            .iter()
                            .rposition(|known| Self::canonical_key(known) < key)
                    })
                    .map_or(0, |index| index + 1);

                order.insert(at, name);
            }
        }

        fn rebuild_sidebar_rank(&self) {
            if self.rebuilding.replace(true) {
                return;
            }

            let settings = settings!();
            let mut order = Self::load_sidebar_order(&settings);

            let summary_graphs = self.summary_graphs.take();
            let present = summary_graphs
                .keys()
                .map(|graph| graph.widget_name().to_string())
                .collect::<Vec<_>>();
            self.summary_graphs.set(summary_graphs);

            let present_set = present.iter().cloned().collect::<HashSet<_>>();

            Self::merge_new_devices(&mut order, present);
            prune_sidebar_order(&mut order, &present_set, MAX_REMEMBERED_DEVICES);
            Self::store_sidebar_order(&settings, &order);

            self.sidebar_rank.replace(
                order
                    .into_iter()
                    .enumerate()
                    .map(|(rank, name)| (name, rank))
                    .collect(),
            );

            self.sidebar().invalidate_sort();
            self.rebuilding.set(false);
        }

        fn move_in_saved_order(&self, dragged: &str, target: &str, after_target: bool) {
            if dragged == target {
                return;
            }

            let settings = settings!();

            let mut order = Self::load_sidebar_order(&settings);
            order.retain(|name| name != dragged);

            let at = match order.iter().position(|name| name == target) {
                Some(index) if after_target => index + 1,
                Some(index) => index,
                None => {
                    g_warning!(
                        "MissionCenter::PerformancePage",
                        "Drop target {} is not in the saved sidebar order",
                        target
                    );

                    return;
                }
            };
            order.insert(at, dragged.to_owned());

            Self::store_sidebar_order(&settings, &order);
        }

        fn set_sidebar(&self, lb: &gtk::ListBox) {
            let this = self.obj().as_ref().clone();

            lb.connect_row_selected(move |_, selected_row| {
                if let Some(row) = selected_row {
                    let child = match row.child() {
                        Some(child) => child,
                        None => {
                            g_critical!(
                                "MissionCenter::PerformancePage",
                                "Failed to get child of selected row"
                            );

                            return;
                        }
                    };

                    let child_name = child.widget_name();
                    let page_name = child_name.as_str();

                    let imp = this.imp();

                    let actions = imp.context_menu_view_actions.take();
                    if let Some(new_action) =
                        Self::view_action_name(page_name).and_then(|name| actions.get(name))
                    {
                        let prev_action = imp.current_view_action.replace(new_action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        new_action.set_state(&glib::Variant::from(true));
                    }

                    imp.context_menu_view_actions.set(actions);
                    imp.page_stack.set_visible_child_name(page_name);

                    settings!()
                        .set_string("performance-selected-page", page_name)
                        .unwrap_or_else(|_| {
                            g_warning!(
                                "MissionCenter::PerformancePage",
                                "Failed to set performance-selected-page setting"
                            );
                        });
                }
            });

            lb.set_sort_func({
                let this = self.obj().downgrade();
                move |row_a, row_b| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return gtk::Ordering::Equal,
                    };

                    let rank = this.imp().sidebar_rank.borrow();

                    let key = |row: &gtk::ListBoxRow| -> (u8, usize, u8, String) {
                        let name = row
                            .child()
                            .map(|child| child.widget_name().to_string())
                            .unwrap_or_default();

                        match rank.get(&name) {
                            Some(&index) => (0, index, 0, String::new()),
                            None => {
                                let category = Self::canonical_key(&name).0;
                                (1, 0, category, name)
                            }
                        }
                    };

                    key(row_a).cmp(&key(row_b)).into()
                }
            });

            lb.set_filter_func({
                let this = self.obj().downgrade();
                move |row| match this.upgrade() {
                    Some(this) => this.imp().row_shown(row),
                    None => true,
                }
            });

            let drop_target = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::all());
            drop_target.set_preload(true);
            drop_target.set_types(&[glib::Type::STRING]);
            drop_target.connect_motion({
                let this = self.obj().downgrade();
                move |_, _, y| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return gdk::DragAction::empty(),
                    };

                    let sidebar = this.imp().sidebar();

                    let summary_graphs = this.imp().summary_graphs.take();

                    for graph in summary_graphs.keys() {
                        graph.hide_drop_hint();
                    }

                    let mut drop_hint_bottom = false;
                    let row_count = summary_graphs.len() as i32;
                    let graph = match sidebar
                        .row_at_y(y as i32)
                        .and_then(|row| row.child())
                        .and_then(|child| child.downcast_ref::<SummaryGraph>().cloned())
                    {
                        Some(graph) => graph,
                        None => {
                            if y < 10. {
                                this.imp().summary_graphs.set(summary_graphs);
                                return gdk::DragAction::empty();
                            }

                            drop_hint_bottom = true;

                            let mut target_graph = None;

                            for i in (0..row_count).rev() {
                                let row = match sidebar.row_at_index(i) {
                                    Some(row) => row,
                                    None => continue,
                                };

                                if !row.is_visible() || !this.imp().row_shown(&row) {
                                    continue;
                                }

                                match row
                                    .child()
                                    .and_then(|child| child.downcast_ref::<SummaryGraph>().cloned())
                                {
                                    Some(graph) => {
                                        target_graph = Some(graph);
                                        break;
                                    }
                                    None => {
                                        this.imp().summary_graphs.set(summary_graphs);
                                        return gdk::DragAction::empty();
                                    }
                                }
                            }

                            match target_graph {
                                Some(graph) => graph,
                                None => {
                                    this.imp().summary_graphs.set(summary_graphs);
                                    return gdk::DragAction::empty();
                                }
                            }
                        }
                    };

                    if drop_hint_bottom {
                        graph.show_drop_hint_bottom();
                    } else {
                        graph.show_drop_hint_top();
                    }

                    this.imp().summary_graphs.set(summary_graphs);

                    gdk::DragAction::MOVE
                }
            });
            drop_target.connect_leave({
                let this = self.obj().downgrade();
                move |_| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    let summary_graphs = this.imp().summary_graphs.take();
                    for graph in summary_graphs.keys() {
                        graph.hide_drop_hint();
                    }
                    this.imp().summary_graphs.set(summary_graphs);
                }
            });
            drop_target.connect_drop({
                let this = self.obj().downgrade();
                move |_, value, _, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return false,
                    };

                    let dragged_name: String = match value.get() {
                        Ok(value) => value,
                        Err(_) => return false,
                    };

                    let summary_graphs = this.imp().summary_graphs.take();

                    let target = summary_graphs
                        .keys()
                        .find(|graph| graph.is_drop_hint_visible())
                        .map(|graph| (graph.widget_name(), graph.is_drop_hint_bottom()));

                    this.imp().summary_graphs.set(summary_graphs);

                    if let Some((target_name, after_target)) = target {
                        this.imp().move_in_saved_order(
                            dragged_name.as_str(),
                            target_name.as_str(),
                            after_target,
                        );
                    }

                    true
                }
            });
            lb.add_controller(drop_target);

            self.sidebar.replace(lb.clone());
        }

        /// Select the closest shown row to `index`, searching forward then backward.
        fn select_nearest_shown_row(&self, index: i32) {
            let sidebar = self.sidebar();

            let mut forward = index;
            let mut backward = index - 1;

            loop {
                let forward_row = sidebar.row_at_index(forward);
                let backward_row = if backward >= 0 {
                    sidebar.row_at_index(backward)
                } else {
                    None
                };

                if forward_row.is_none() && backward_row.is_none() {
                    return;
                }

                if let Some(row) = forward_row {
                    if self.row_shown(&row) {
                        sidebar.select_row(Some(&row));
                        return;
                    }
                    forward += 1;
                }

                if let Some(row) = backward_row {
                    if self.row_shown(&row) {
                        sidebar.select_row(Some(&row));
                        return;
                    }
                    backward -= 1;
                }
            }
        }

        /// Select the row for `page_name`, or the closest shown row if it is gone or hidden.
        fn select_page(&self, page_name: &str) {
            let sidebar = self.sidebar();

            let mut index = 0;
            while let Some(row) = sidebar.row_at_index(index) {
                let name = row.child().map(|child| child.widget_name());
                if name.as_deref() == Some(page_name) {
                    if self.row_shown(&row) {
                        sidebar.select_row(Some(&row));
                    } else {
                        self.select_nearest_shown_row(index);
                    }
                    return;
                }

                index += 1;
            }

            self.select_nearest_shown_row(0);
        }

        /// Select the first shown row belonging to `action_name`, in sidebar order.
        fn select_first_shown_in_category(&self, action_name: &str) -> bool {
            let sidebar = self.sidebar();

            let mut index = 0;
            while let Some(row) = sidebar.row_at_index(index) {
                let name = row
                    .child()
                    .map(|child| child.widget_name())
                    .unwrap_or_default();

                if Self::view_action_name(name.as_str()) == Some(action_name)
                    && self.row_shown(&row)
                {
                    sidebar.select_row(Some(&row));
                    return true;
                }

                index += 1;
            }

            false
        }

        fn set_sidebar_edit_mode(&self, edit_mode: bool) {
            self.sidebar_edit_mode.set(edit_mode);

            let summary_graphs = self.summary_graphs.take();
            for (graph, drag_source) in &summary_graphs {
                graph.set_edit_mode(edit_mode, self.device_visible(graph));

                if edit_mode {
                    drag_source.set_actions(gdk::DragAction::MOVE);
                } else {
                    drag_source.set_actions(gdk::DragAction::empty());
                }
            }
            self.summary_graphs.set(summary_graphs);

            self.sidebar().invalidate_filter();

            let active_page_name = self.page_stack.visible_child_name().unwrap_or_default();
            self.select_page(active_page_name.as_str());
        }

        fn infobar_visible(&self) -> bool {
            self.page_content.shows_sidebar()
        }

        fn set_infobar_visible(&self, v: bool) {
            self.page_content
                .set_show_sidebar(!self.page_content.is_collapsed() || v);
        }

        fn info_button_visible(&self) -> bool {
            self.page_content.is_collapsed()
        }
    }

    impl PerformancePage {
        fn configure_actions(&self) -> gio::SimpleActionGroup {
            let this = self.obj();
            let actions = gio::SimpleActionGroup::new();

            let mut view_actions = HashMap::new();

            let action = gio::SimpleAction::new_stateful(
                "summary",
                None,
                &glib::Variant::from(self.summary_mode.get()),
            );
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    let new_state = !this.summary_mode();
                    action.set_state(&glib::Variant::from(new_state));
                    this.set_summary_mode(new_state);
                    if !this.imp().breakpoint_applied.get() {
                        this.imp().page_content.set_show_sidebar(!new_state);
                    }
                }
            });
            actions.add_action(&action);

            let action = gio::SimpleAction::new_stateful("cpu", None, &glib::Variant::from(true));
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.select_first_shown_in_category("cpu") {
                        let prev_action = this.current_view_action.replace(action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        action.set_state(&glib::Variant::from(true));
                    }
                }
            });
            actions.add_action(&action);
            view_actions.insert("cpu".to_string(), action.clone());
            self.current_view_action.set(action);

            let action =
                gio::SimpleAction::new_stateful("memory", None, &glib::Variant::from(false));
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.select_first_shown_in_category("memory") {
                        let prev_action = this.current_view_action.replace(action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        action.set_state(&glib::Variant::from(true));
                    }
                }
            });
            actions.add_action(&action);
            view_actions.insert("memory".to_string(), action);

            let action = gio::SimpleAction::new_stateful("disk", None, &glib::Variant::from(false));
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.select_first_shown_in_category("disk") {
                        let prev_action = this.current_view_action.replace(action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        action.set_state(&glib::Variant::from(true));
                    }
                }
            });
            actions.add_action(&action);
            view_actions.insert("disk".to_string(), action);

            let action =
                gio::SimpleAction::new_stateful("network", None, &glib::Variant::from(false));
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.select_first_shown_in_category("network") {
                        let prev_action = this.current_view_action.replace(action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        action.set_state(&glib::Variant::from(true));
                    }
                }
            });
            actions.add_action(&action);
            view_actions.insert("network".to_string(), action);

            let action = gio::SimpleAction::new_stateful("gpu", None, &glib::Variant::from(false));
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.select_first_shown_in_category("gpu") {
                        let prev_action = this.current_view_action.replace(action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        action.set_state(&glib::Variant::from(true));
                    }
                }
            });
            actions.add_action(&action);
            view_actions.insert("gpu".to_string(), action);
            let action = gio::SimpleAction::new_stateful("fan", None, &glib::Variant::from(false));
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.select_first_shown_in_category("fan") {
                        let prev_action = this.current_view_action.replace(action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        action.set_state(&glib::Variant::from(true));
                    }
                }
            });
            actions.add_action(&action);
            view_actions.insert("fan".to_string(), action);
            let action =
                gio::SimpleAction::new_stateful("battery", None, &glib::Variant::from(false));
            action.connect_activate({
                let this = this.downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.select_first_shown_in_category("battery") {
                        let prev_action = this.current_view_action.replace(action.clone());
                        prev_action.set_state(&glib::Variant::from(false));
                        action.set_state(&glib::Variant::from(true));
                    }
                }
            });
            actions.add_action(&action);
            view_actions.insert("battery".to_string(), action);

            self.context_menu_view_actions.set(view_actions);

            actions
        }

        fn configure_page<P: PageExt + IsA<gtk::Widget>>(&self, page: &P) {
            self.page_content.connect_collapsed_notify({
                let page = page.downgrade();
                move |pc| {
                    if let Some(page) = page.upgrade() {
                        if pc.is_collapsed() {
                            page.infobar_collapsed();
                        } else {
                            page.infobar_uncollapsed();
                        }
                    }
                }
            });

            self.obj()
                .as_ref()
                .bind_property("summary-mode", page, "summary-mode")
                .flags(glib::BindingFlags::SYNC_CREATE)
                .build();
        }

        fn add_to_sidebar(&self, graph: &SummaryGraph) {
            let sidebar = self.sidebar();

            let drag_source = gtk::DragSource::builder()
                .actions(gdk::DragAction::empty())
                .build();

            let edit_mode = self.sidebar_edit_mode.get();
            graph.set_edit_mode(edit_mode, self.device_visible(graph));
            if edit_mode {
                drag_source.set_actions(gdk::DragAction::MOVE);
            }

            let mut summary_graphs = self.summary_graphs.take();
            summary_graphs.insert(graph.clone(), drag_source.clone());
            self.summary_graphs.set(summary_graphs);

            sidebar.append(graph);

            let row = match graph
                .parent()
                .and_then(|parent| parent.downcast::<gtk::ListBoxRow>().ok())
            {
                Some(row) => row,
                None => {
                    g_critical!(
                        "MissionCenter::PerformancePage",
                        "Sidebar row is missing for {}, it cannot be dragged",
                        graph.widget_name()
                    );

                    return;
                }
            };

            drag_source.connect_prepare({
                let graph = graph.downgrade();
                move |src, x, y| {
                    if !src.actions().contains(gdk::DragAction::MOVE) {
                        return None;
                    }

                    let graph = match graph.upgrade() {
                        Some(graph) => graph,
                        None => return None,
                    };

                    let row = match graph
                        .parent()
                        .and_then(|row| row.downcast_ref::<gtk::ListBoxRow>().cloned())
                    {
                        Some(row) => row,
                        None => return None,
                    };

                    src.set_icon(
                        Some(&gtk::WidgetPaintable::new(Some(&row)).current_image()),
                        x.round() as i32,
                        y.round() as i32,
                    );

                    Some(gdk::ContentProvider::for_value(&Value::from(
                        graph.widget_name().as_str(),
                    )))
                }
            });

            drag_source.connect_drag_begin({
                let this = self.obj().downgrade();
                let graph = graph.downgrade();
                move |_, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    let graph = match graph.upgrade() {
                        Some(graph) => graph,
                        None => return,
                    };

                    this.sidebar().unselect_all();

                    let summary_graphs = this.imp().summary_graphs.take();
                    for sg in summary_graphs.keys() {
                        if sg.as_ptr() != graph.as_ptr() {
                            if let Some(row) = sg.parent() {
                                row.set_sensitive(false);
                            }
                        }
                    }
                    this.imp().summary_graphs.set(summary_graphs);

                    if let Some(row) = graph.parent() {
                        row.set_visible(false);
                    }
                }
            });

            drag_source.connect_drag_end({
                let this = self.obj().downgrade();
                move |src, _, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    let summary_graphs = this.imp().summary_graphs.take();
                    for graph in summary_graphs.keys() {
                        if let Some(row) = graph.parent() {
                            row.set_sensitive(true);
                            row.set_visible(true);
                        }
                        graph.hide_drop_hint();
                    }
                    this.imp().summary_graphs.set(summary_graphs);

                    src.set_icon(None::<&gtk::WidgetPaintable>, 0, 0);
                    src.set_content(None::<&gdk::ContentProvider>);
                }
            });

            row.add_controller(drag_source);
        }

        fn set_up_cpu_page(
            &self,
            pages: &mut Vec<Pages>,
            readings: &crate::magpie_client::Readings,
        ) {
            let summary = SummaryGraph::new(DeviceType::Cpu);
            summary.set_widget_name("cpu");

            summary.set_heading(i18n("CPU"));
            summary.set_info1("0% 0.00 GHz");
            match readings.cpu.temperature_celsius.as_ref() {
                Some(v) => summary.set_info2(format!("{:.0} °C", *v)),
                _ => {}
            }

            summary.set_base_color(gdk::RGBA::new(
                CPU_BASE_COLOR[0] as f32 / 255.,
                CPU_BASE_COLOR[1] as f32 / 255.,
                CPU_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));

            let settings = settings!();

            let usage_group = DatasetGroup::new();

            summary.graph_widget().add_dataset(usage_group);
            summary.graph_widget().connect_to_settings(&settings!());

            let page = CpuPage::new(&settings);
            page.set_base_color(gdk::RGBA::new(
                CPU_BASE_COLOR[0] as f32 / 255.,
                CPU_BASE_COLOR[1] as f32 / 255.,
                CPU_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));
            page.set_static_information(readings);

            self.configure_page(&page);

            self.page_stack.add_named(&page, Some("cpu"));
            self.add_to_sidebar(&summary);

            pages.push(Pages::Cpu((summary, page)));
        }

        fn set_up_memory_page(
            &self,
            pages: &mut Vec<Pages>,
            readings: &crate::magpie_client::Readings,
        ) {
            let summary = SummaryGraph::new(DeviceType::Memory);
            summary.set_widget_name("memory");
            let mem_info = readings.mem_info;

            let settings = settings!();

            {
                let graph_widget = summary.graph_widget();

                let mut dataset_a = DatasetGroup::new();
                dataset_a.dataset_settings.fill = FillingSettings::None;
                dataset_a.dataset_settings.dashed = true;
                dataset_a.dataset_settings.high_watermark = mem_info.mem_total as f32;
                dataset_a.dataset_settings.scaling_settings = ScalingSettings::Fixed;
                let mut dataset_b = DatasetGroup::new();
                dataset_b.dataset_settings.high_watermark = mem_info.mem_total as f32;
                dataset_b.dataset_settings.scaling_settings = ScalingSettings::Fixed;

                graph_widget.add_dataset(dataset_a);
                graph_widget.add_dataset(dataset_b);

                // graph_widget.connect_datasets(0, 1);
                // graph_widget.connect_datasets(1, 0);

                graph_widget.connect_to_settings(&settings);
            }

            summary.set_heading(i18n("Memory"));
            summary.set_info1("0/0 GiB");
            summary.set_info2("0%");

            summary.set_base_color(gdk::RGBA::new(
                MEMORY_BASE_COLOR[0] as f32 / 255.,
                MEMORY_BASE_COLOR[1] as f32 / 255.,
                MEMORY_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));

            let page = MemoryPage::new(&settings);
            page.set_base_color(gdk::RGBA::new(
                MEMORY_BASE_COLOR[0] as f32 / 255.,
                MEMORY_BASE_COLOR[1] as f32 / 255.,
                MEMORY_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));
            page.set_memory_color(gdk::RGBA::new(
                DISK_BASE_COLOR[0] as f32 / 255.,
                DISK_BASE_COLOR[1] as f32 / 255.,
                DISK_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));
            page.set_static_information(readings);

            self.configure_page(&page);

            self.page_stack.add_named(&page, Some("memory"));
            self.add_to_sidebar(&summary);

            pages.push(Pages::Memory((summary, page)));
        }

        fn set_up_disk_pages(
            &self,
            pages: &mut Vec<Pages>,
            readings: &crate::magpie_client::Readings,
        ) {
            let mut disks = HashMap::new();
            let len = readings.disks_info.len();
            let hide_index = len == 1;
            for i in 0..len {
                let mut ret =
                    self.create_disk_page(readings, if hide_index { None } else { Some(i as i32) });
                disks.insert(std::mem::take(&mut ret.0), ret.1);
            }

            pages.push(Pages::Disk(disks));
        }

        pub fn update_disk_heading(
            &self,
            disk_graph: &SummaryGraph,
            kind: Option<DiskKind>,
            disk_id: &str,
            index: Option<i32>,
        ) {
            let kind = match kind {
                Some(DiskKind::Hdd) => i18n("HDD"),
                Some(DiskKind::Ssd) => i18n("SSD"),
                Some(DiskKind::NvMe) => i18n("NVMe"),
                Some(DiskKind::EMmc) => i18n("eMMC"),
                Some(DiskKind::Sd) => i18n("SD"),
                Some(DiskKind::IScsi) => i18n("iSCSI"),
                Some(DiskKind::Optical) => i18n("Optical"),
                Some(DiskKind::Floppy) => i18n("Floppy"),
                Some(DiskKind::ThumbDrive) => i18n("Thumb Drive"),
                None => i18n("Drive"),
            };

            if index.is_some() {
                disk_graph.set_heading(i18n_f(
                    "{} {} ({})",
                    &[
                        &format!("{}", kind),
                        &format!("{}", index.unwrap()),
                        &format!("{}", disk_id),
                    ],
                ));
            } else {
                disk_graph.set_heading(kind);
            }
        }

        fn disk_page_name(disk_id: &str) -> String {
            format!("disk-{}", disk_id)
        }

        pub fn create_disk_page(
            &self,
            readings: &crate::magpie_client::Readings,
            disk_id: Option<i32>,
        ) -> (String, (SummaryGraph, DiskPage)) {
            let disk = &readings.disks_info[disk_id.unwrap_or(0) as usize];

            let page_name = Self::disk_page_name(disk.id.as_ref());

            let summary = SummaryGraph::new(DeviceType::Disk);
            summary.set_widget_name(&page_name);

            self.update_disk_heading(
                &summary,
                disk.kind.and_then(|k| k.try_into().ok()),
                &disk.id,
                disk_id,
            );
            if let Some(model) = &disk.model {
                summary.set_info1(model.as_ref());
            }
            summary.set_info2(format!(
                "{:.0}%{}",
                disk.busy_percent,
                if let Some(temp_mk) = disk.temperature_milli_k {
                    format!(" ({:.0} °C)", (temp_mk as i32 + MK_TO_0_C) as f64 / 1000.)
                } else {
                    String::new()
                }
            ));

            summary.set_base_color(gdk::RGBA::new(
                DISK_BASE_COLOR[0] as f32 / 255.,
                DISK_BASE_COLOR[1] as f32 / 255.,
                DISK_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));

            let settings = settings!();

            let busy_pct = DatasetGroup::new();

            summary.graph_widget().add_dataset(busy_pct);
            summary.graph_widget().connect_to_settings(&settings);

            let page = DiskPage::new(&page_name, &settings);
            page.set_base_color(gdk::RGBA::new(
                DISK_BASE_COLOR[0] as f32 / 255.,
                DISK_BASE_COLOR[1] as f32 / 255.,
                DISK_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));
            page.set_static_information(disk_id, disk);

            self.configure_page(&page);

            self.page_stack.add_named(&page, Some(&page_name));
            self.add_to_sidebar(&summary);

            (page_name, (summary, page))
        }

        fn set_up_network_pages(
            &self,
            pages: &mut Vec<Pages>,
            readings: &crate::magpie_client::Readings,
        ) {
            let mut networks = HashMap::new();
            for (_, connection) in &readings.network_connections {
                let mut ret = self.create_network_page(connection);
                networks.insert(std::mem::take(&mut ret.0), ret.1);
            }

            pages.push(Pages::Network(networks));
        }

        fn network_page_name(if_name: &str) -> String {
            format!("net-{}", if_name)
        }

        fn create_network_page(
            &self,
            connection: &Connection,
        ) -> (String, (SummaryGraph, NetworkPage)) {
            let if_name = connection.id.as_str();
            let page_name = Self::network_page_name(if_name);

            let conn_kind: ConnectionKind =
                ConnectionKind::try_from(connection.kind).expect("Invalid connection type");
            let conn_type = conn_kind.as_str_name();

            let settings = settings!();

            let network_group = NetworkGroup::from_connection_kind(conn_kind);
            let summary = SummaryGraph::new(DeviceType::Network(network_group));
            summary.set_widget_name(&page_name);
            summary.set_heading(format!("{} ({})", conn_type, if_name));
            {
                let graph_widget = summary.graph_widget();

                let mut dataset_a = DatasetGroup::new();
                dataset_a.dataset_settings.fill = FillingSettings::None;
                dataset_a.dataset_settings.dashed = true;
                let mut dataset_b = DatasetGroup::new();
                dataset_a.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
                dataset_b.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
                dataset_a.dataset_settings.rounding_settings = RoundingSettings::Pow2;
                dataset_b.dataset_settings.rounding_settings = RoundingSettings::Pow2;

                graph_widget.add_dataset(dataset_a);
                graph_widget.add_dataset(dataset_b);

                graph_widget.connect_datasets(0, 1);
                graph_widget.connect_datasets(1, 0);

                graph_widget.set_base_color(gdk::RGBA::new(
                    NETWORK_BASE_COLOR[0] as f32 / 255.,
                    NETWORK_BASE_COLOR[1] as f32 / 255.,
                    NETWORK_BASE_COLOR[2] as f32 / 255.,
                    1.,
                ));

                graph_widget.connect_to_settings(&settings);
            }

            if let Some(max_speed) = connection.max_speed_bytes_ps {
                if !settings.boolean("performance-page-network-dynamic-scaling") {
                    summary
                        .graph_widget()
                        .set_dataset_scaling(0, ScalingSettings::Fixed);
                    summary
                        .graph_widget()
                        .set_dataset_max_scale(0, max_speed as f32);
                }
                settings.connect_changed(Some("performance-page-network-dynamic-scaling"), {
                    let graph = summary.graph_widget().downgrade();
                    move |settings, _| {
                        let graph = match graph.upgrade() {
                            Some(graph) => graph,
                            None => return,
                        };

                        let dynamic_scaling =
                            settings.boolean("performance-page-network-dynamic-scaling");

                        if dynamic_scaling {
                            graph.set_dataset_scaling(0, ScalingSettings::ScaleUp);
                        } else {
                            graph.set_dataset_scaling(0, ScalingSettings::Fixed);
                            graph.set_dataset_max_scale(0, max_speed as f32);
                        }
                    }
                });
            }

            let page = NetworkPage::new(if_name, conn_kind, &settings);
            page.set_base_color(gdk::RGBA::new(
                NETWORK_BASE_COLOR[0] as f32 / 255.,
                NETWORK_BASE_COLOR[1] as f32 / 255.,
                NETWORK_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));

            page.set_static_information(connection);
            self.configure_page(&page);

            self.page_stack.add_named(&page, Some(&page_name));
            self.add_to_sidebar(&summary);

            (page_name, (summary, page))
        }

        fn gpu_page_name(device_id: &str) -> String {
            format!("gpu-{}", device_id)
        }

        fn create_gpu_page(
            &self,
            gpu: &Gpu,
            index: Option<usize>,
        ) -> (String, (SummaryGraph, GpuPage)) {
            let page_name = Self::gpu_page_name(&gpu.id);

            let summary = SummaryGraph::new(DeviceType::Gpu);
            summary.set_widget_name(&page_name);

            let settings = settings!();

            let sumset = DatasetGroup::new();

            summary.graph_widget().add_dataset(sumset);

            summary.graph_widget().connect_to_settings(&settings);

            let page = GpuPage::new(gpu.device_name.as_ref().unwrap_or(&i18n("Unknown")));

            if let Some(index) = index {
                summary.set_heading(i18n_f("GPU {}", &[&format!("{}", index)]));
            } else {
                summary.set_heading(i18n_f("GPU", &[]));
            }
            summary.set_info1(
                gpu.device_name
                    .as_ref()
                    .unwrap_or(&i18n("Unknown"))
                    .as_str(),
            );

            let mut info2 = ArrayString::<256>::new();
            if let Some(v) = gpu.utilization_percent {
                let _ = write!(&mut info2, "{v}%");
            }
            if let Some(v) = gpu.temperature_c {
                let _ = write!(&mut info2, " ({v:.2}°C)");
            }
            summary.set_info2(info2.as_str());

            summary.set_base_color(gdk::RGBA::new(
                GPU_BASE_COLOR[0] as f32 / 255.,
                GPU_BASE_COLOR[1] as f32 / 255.,
                GPU_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));

            page.set_base_color(gdk::RGBA::new(
                GPU_BASE_COLOR[0] as f32 / 255.,
                GPU_BASE_COLOR[1] as f32 / 255.,
                GPU_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));
            page.set_static_information(index, gpu);

            self.configure_page(&page);

            self.page_stack.add_named(&page, Some(&page_name));
            self.add_to_sidebar(&summary);

            (page_name, (summary, page))
        }

        fn set_up_gpu_pages(
            &self,
            pages: &mut Vec<Pages>,
            readings: &crate::magpie_client::Readings,
        ) {
            let mut gpus = HashMap::new();

            let hide_index = readings.gpus.len() == 1;
            for (index, gpu) in readings.gpus.values().enumerate() {
                let (page_name, (summary, page)) =
                    self.create_gpu_page(gpu, if hide_index { None } else { Some(index) });
                gpus.insert(page_name, (summary, page));
            }

            pages.push(Pages::Gpu(gpus));
        }

        fn set_up_fan_pages(
            &self,
            pages: &mut Vec<Pages>,
            readings: &crate::magpie_client::Readings,
        ) {
            let mut fans = HashMap::new();
            let len = readings.fans.len();
            let hide_index = len == 1;
            for i in 0..len {
                let mut ret =
                    self.create_fan_page(readings, if hide_index { None } else { Some(i) });
                fans.insert(std::mem::take(&mut ret.0), ret.1);
            }

            pages.push(Pages::Fan(fans));
        }

        fn fan_page_name(fan_info: &Fan) -> String {
            format!("fan-{}-{}", fan_info.hwmon_index, fan_info.fan_index)
        }

        pub fn create_fan_page(
            &self,
            readings: &crate::magpie_client::Readings,
            index: Option<usize>,
        ) -> (String, (SummaryGraph, FanPage)) {
            let fan_static_info = &readings.fans[index.unwrap_or(0)];

            let page_name = Self::fan_page_name(fan_static_info);

            let summary = SummaryGraph::new(DeviceType::Fan);
            summary.set_widget_name(&page_name);

            if let Some(index) = index {
                summary.set_heading(i18n_f("Fan {}", &[&format!("{}", index)]));
            } else {
                summary.set_heading(i18n("Fan"));
            }
            summary.set_base_color(gdk::RGBA::new(
                FAN_BASE_COLOR[0] as f32 / 255.,
                FAN_BASE_COLOR[1] as f32 / 255.,
                FAN_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));

            let settings = settings!();

            summary.graph_widget().connect_to_settings(&settings);

            let mut speed_dataset = DatasetGroup::new();
            speed_dataset.dataset_settings.scaling_settings = ScalingSettings::StickyUp;

            summary.graph_widget().add_dataset(speed_dataset);

            let page = FanPage::new(&page_name, &settings);
            page.set_base_color(gdk::RGBA::new(
                FAN_BASE_COLOR[0] as f32 / 255.,
                FAN_BASE_COLOR[1] as f32 / 255.,
                FAN_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));
            page.set_static_information(fan_static_info);

            self.configure_page(&page);

            self.page_stack.add_named(&page, Some(&page_name));
            self.add_to_sidebar(&summary);

            (page_name, (summary, page))
        }

        fn set_up_battery_pages(
            &self,
            pages: &mut Vec<Pages>,
            readings: &crate::magpie_client::Readings,
        ) {
            let mut batteries = HashMap::new();
            let len = readings.batteries.len();
            let hide_index = len == 1;
            for i in 0..len {
                let mut ret =
                    self.create_battery_page(readings, if hide_index { None } else { Some(i) });
                batteries.insert(std::mem::take(&mut ret.0), ret.1);
            }

            pages.push(Pages::Battery(batteries));
        }

        fn battery_page_name(battery_info: &Battery) -> String {
            format!(
                "battery-{}-{}",
                battery_info.power_supply.map(|x| !x as u8).unwrap_or(2),
                battery_info.name
            )
        }

        pub fn create_battery_page(
            &self,
            readings: &crate::magpie_client::Readings,
            index: Option<usize>,
        ) -> (String, (SummaryGraph, BatteryPage)) {
            let battery_static_info = &readings.batteries[index.unwrap_or(0)];

            let page_name = Self::battery_page_name(battery_static_info);

            let summary = SummaryGraph::new(DeviceType::Battery);
            summary.set_widget_name(&page_name);

            if let Some(index) = index {
                summary.set_heading(i18n_f("Battery {}", &[&format!("{}", index)]));
            } else {
                summary.set_heading(i18n("Battery"));
            }
            summary.set_base_color(gdk::RGBA::new(
                BATTERY_BASE_COLOR[0] as f32 / 255.,
                BATTERY_BASE_COLOR[1] as f32 / 255.,
                BATTERY_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));

            let settings = settings!();

            summary.graph_widget().connect_to_settings(&settings);
            let speed_dataset = DatasetGroup::new();

            summary.graph_widget().add_dataset(speed_dataset);

            let page = BatteryPage::new(&page_name, &settings);
            page.set_base_color(gdk::RGBA::new(
                BATTERY_BASE_COLOR[0] as f32 / 255.,
                BATTERY_BASE_COLOR[1] as f32 / 255.,
                BATTERY_BASE_COLOR[2] as f32 / 255.,
                1.,
            ));
            page.set_static_information(battery_static_info);

            self.configure_page(&page);

            self.page_stack.add_named(&page, Some(&page_name));
            self.add_to_sidebar(&summary);

            (page_name, (summary, page))
        }
    }

    impl PerformancePage {
        fn refresh_device_visibility(&self) {
            let summary_graphs = self.summary_graphs.take();
            for graph in summary_graphs.keys() {
                graph.set_switch_active(self.device_visible(graph));
            }
            self.summary_graphs.set(summary_graphs);

            self.sidebar().invalidate_filter();
        }

        pub fn set_up_pages(
            this: &super::PerformancePage,
            readings: &crate::magpie_client::Readings,
        ) -> bool {
            let this = this.imp();

            let mut pages = vec![];
            this.set_up_cpu_page(&mut pages, &readings);
            this.set_up_memory_page(&mut pages, &readings);
            this.set_up_disk_pages(&mut pages, &readings);
            this.set_up_network_pages(&mut pages, &readings);
            this.set_up_gpu_pages(&mut pages, &readings);
            this.set_up_fan_pages(&mut pages, &readings);
            this.set_up_battery_pages(&mut pages, &readings);
            this.pages.set(pages);

            let settings = settings!();

            // Migrate from the deprecated performance-sidebar-hidden-graphs key
            let old_hidden = settings.string("performance-sidebar-hidden-graphs");
            if !old_hidden.is_empty() {
                let raw_overrides = settings.string("performance-sidebar-device-overrides");
                let mut overrides = parse_device_overrides(&raw_overrides);

                for name in old_hidden.split(';').filter(|s| !s.is_empty()) {
                    overrides
                        .entry(name.to_string())
                        .or_insert(DeviceOverride::Hide);
                }

                let _ = settings.set_string(
                    "performance-sidebar-device-overrides",
                    &serialize_device_overrides(&overrides),
                );
                let _ = settings.set_string("performance-sidebar-hidden-graphs", "");
            }

            this.rebuild_sidebar_rank();
            this.refresh_device_visibility();
            this.select_page(settings.string("performance-selected-page").as_str());

            let perf_page = this.obj().downgrade();
            let on_order_changed = move |_: &gio::Settings, _: &str| {
                if let Some(perf_page) = perf_page.upgrade() {
                    perf_page.imp().rebuild_sidebar_rank();
                }
            };
            settings.connect_changed(Some("performance-sidebar-order"), on_order_changed);

            let perf_page = this.obj().downgrade();
            let on_category_changed = move |_: &gio::Settings, _: &str| {
                if let Some(perf_page) = perf_page.upgrade() {
                    perf_page.imp().refresh_device_visibility();
                }
            };

            settings.connect_changed(
                Some("performance-sidebar-device-overrides"),
                on_category_changed.clone(),
            );
            settings.connect_changed(Some("performance-show-disks"), on_category_changed.clone());
            settings.connect_changed(
                Some("performance-show-network"),
                on_category_changed.clone(),
            );
            settings.connect_changed(
                Some("performance-show-network-wired"),
                on_category_changed.clone(),
            );
            settings.connect_changed(
                Some("performance-show-network-wireless"),
                on_category_changed.clone(),
            );
            settings.connect_changed(
                Some("performance-show-network-vpn"),
                on_category_changed.clone(),
            );
            settings.connect_changed(
                Some("performance-show-network-virtual"),
                on_category_changed.clone(),
            );
            settings.connect_changed(
                Some("performance-show-network-other"),
                on_category_changed.clone(),
            );
            settings.connect_changed(Some("performance-show-gpus"), on_category_changed.clone());
            settings.connect_changed(Some("performance-show-fans"), on_category_changed.clone());
            settings.connect_changed(Some("performance-show-batteries"), on_category_changed);

            true
        }

        pub fn update_readings(
            this: &super::PerformancePage,
            readings: &crate::magpie_client::Readings,
        ) -> bool {
            let mut pages = this.imp().pages.take();

            let mut pages_to_destroy = Vec::new();

            fn remove_pages<P: IsA<gtk::Widget>>(
                pages_to_destroy: &Vec<String>,
                pages: &mut HashMap<String, (SummaryGraph, P)>,
                summary_graphs: &mut HashMap<SummaryGraph, gtk::DragSource>,
                this: &PerformancePage,
            ) {
                let sidebar = this.sidebar();

                for page_name in pages_to_destroy {
                    if let Some((graph, page)) = pages.get(page_name).and_then(|v| Some(v.clone()))
                    {
                        summary_graphs.remove(&graph);
                        this.page_stack.remove(&page);
                        pages.remove(page_name);

                        let row = match graph
                            .parent()
                            .and_then(|parent| parent.downcast::<gtk::ListBoxRow>().ok())
                        {
                            Some(row) => row,
                            None => {
                                g_warning!(
                                    "MissionCenter::PerformancePage",
                                    "Failed to get parent of graph widget, is it not in the sidebar?"
                                );
                                continue;
                            }
                        };

                        let was_selected = sidebar
                            .selected_row()
                            .map_or(false, |selected| selected.eq(&row));
                        let index = row.index();

                        sidebar.remove(&row);

                        if was_selected {
                            this.select_nearest_shown_row(index);
                        }
                    }
                }
            }

            let mut summary_graphs = this.imp().summary_graphs.take();

            for page in &mut pages {
                match page {
                    Pages::Cpu(_) => {}    // not dynamic
                    Pages::Memory(_) => {} // not dynamic
                    Pages::Disk(ref mut disks_pages) => {
                        for disk_page_name in disks_pages.keys() {
                            if !readings.disks_info.iter().any(|disk| {
                                disk.capacity_bytes > 0
                                    && &Self::disk_page_name(disk.id.as_ref()) == disk_page_name
                            }) {
                                pages_to_destroy.push(disk_page_name.clone());
                            }
                        }

                        remove_pages(
                            &pages_to_destroy,
                            disks_pages,
                            &mut summary_graphs,
                            this.imp(),
                        );
                        pages_to_destroy.clear();
                    }
                    Pages::Network(net_pages) => {
                        for net_page_name in net_pages.keys() {
                            if !readings.network_connections.iter().any(|(_, device)| {
                                &Self::network_page_name(&device.id) == net_page_name
                            }) {
                                pages_to_destroy.push(net_page_name.clone());
                            }
                        }

                        remove_pages(
                            &pages_to_destroy,
                            net_pages,
                            &mut summary_graphs,
                            this.imp(),
                        );
                        pages_to_destroy.clear();
                    }
                    Pages::Gpu(gpu_pages) => {
                        for gpu_page_name in gpu_pages.keys() {
                            if !readings.gpus.contains_key(&gpu_page_name[4..]) {
                                pages_to_destroy.push(gpu_page_name.clone());
                            }
                        }

                        remove_pages(
                            &pages_to_destroy,
                            gpu_pages,
                            &mut summary_graphs,
                            this.imp(),
                        );
                        pages_to_destroy.clear();
                    }
                    Pages::Fan(fan_pages) => {
                        for fan_page_name in fan_pages.keys() {
                            if !readings
                                .fans
                                .iter()
                                .any(|fan| &Self::fan_page_name(&fan) == fan_page_name)
                            {
                                pages_to_destroy.push(fan_page_name.clone());
                            }
                        }

                        remove_pages(
                            &pages_to_destroy,
                            fan_pages,
                            &mut summary_graphs,
                            this.imp(),
                        );
                        pages_to_destroy.clear();
                    }
                    Pages::Battery(battery_pages) => {
                        for battery_page_name in battery_pages.keys() {
                            if !readings.batteries.iter().any(|battery| {
                                &Self::battery_page_name(&battery) == battery_page_name
                            }) {
                                pages_to_destroy.push(battery_page_name.clone());
                            }
                        }

                        remove_pages(
                            &pages_to_destroy,
                            battery_pages,
                            &mut summary_graphs,
                            this.imp(),
                        );
                        pages_to_destroy.clear();
                    }
                }
            }

            this.imp().summary_graphs.set(summary_graphs);

            let mut result = true;

            for page in &mut pages {
                match page {
                    Pages::Cpu((summary, page)) => {
                        let graph_widget = summary.graph_widget();

                        let mut info2 = ArrayString::<256>::new();
                        let _ = write!(&mut info2, "{}%", readings.cpu.total_usage_percent.round());
                        if let Some(temp) = readings.cpu.temperature_celsius.as_ref() {
                            let _ = write!(&mut info2, " ({:.0} °C)", temp);
                        }

                        graph_widget.add_data_point(vec![vec![readings.cpu.total_usage_percent]]);
                        if let Some(name) = readings.cpu.name.as_ref() {
                            summary.set_info1(name.as_str());
                            summary.set_info2(info2.as_str());
                        } else {
                            summary.set_info1(info2.as_str());
                        }

                        result &= page.update_readings(readings);
                    }
                    Pages::Memory((summary, page)) => {
                        let mem_info = &readings.mem_info;
                        let total_raw = mem_info.mem_total;
                        let total =
                            crate::to_human_readable_nice(total_raw as _, &DataType::MemoryBytes);

                        // https://gitlab.com/procps-ng/procps/-/blob/master/library/meminfo.c?ref_type=heads#L736
                        let mem_avail = if mem_info.mem_available > mem_info.mem_total {
                            mem_info.mem_free
                        } else {
                            mem_info.mem_available
                        };

                        let used_raw = total_raw.saturating_sub(mem_avail);
                        let graph_widget = summary.graph_widget();
                        graph_widget.add_data_point(vec![
                            vec![readings.mem_info.committed as _],
                            vec![used_raw as _],
                        ]);
                        let used =
                            crate::to_human_readable_nice(used_raw as _, &DataType::MemoryBytes);

                        summary.set_info1(format!("{} {}", used, total,));
                        summary.set_info2(format!(
                            "{}%",
                            ((used_raw as f32 / total_raw as f32) * 100.).round()
                        ));

                        result &= page.update_readings(readings);
                    }
                    Pages::Disk(pages) => {
                        let mut new_devices = Vec::new();
                        let hide_index = readings.disks_info.len() == 1;
                        for (index, disk) in readings.disks_info.iter().enumerate() {
                            if let Some((summary, page)) =
                                pages.get(&Self::disk_page_name(disk.id.as_ref()))
                            {
                                this.imp().update_disk_heading(
                                    summary,
                                    disk.kind.and_then(|k| k.try_into().ok()),
                                    disk.id.as_ref(),
                                    if hide_index { None } else { Some(index as i32) },
                                );

                                let graph_widget = summary.graph_widget();
                                graph_widget.add_data_point(vec![vec![disk.busy_percent]]);
                                if let Some(temp_mk) = disk.temperature_milli_k {
                                    summary.set_info2(format!(
                                        "{:.0}% ({:.0} °C)",
                                        disk.busy_percent,
                                        (temp_mk as i32 + MK_TO_0_C) as f64 / 1000.
                                    ));
                                } else {
                                    summary.set_info2(format!("{:.0}%", disk.busy_percent));
                                }

                                result &= page.update_readings(
                                    if hide_index { None } else { Some(index) },
                                    disk,
                                );
                            } else {
                                new_devices.push(index);
                            }
                        }

                        for new_device_index in new_devices {
                            if readings.disks_info[new_device_index].capacity_bytes == 0 {
                                continue;
                            }
                            let (disk_id, page) = this.imp().create_disk_page(
                                readings,
                                if hide_index {
                                    None
                                } else {
                                    Some(new_device_index as i32)
                                },
                            );

                            pages.insert(disk_id, page);
                        }
                    }
                    Pages::Network(pages) => {
                        let mut new_devices = Vec::new();
                        for (index, network_connection) in readings.network_connections.iter() {
                            if let Some((summary, page)) =
                                pages.get(&Self::network_page_name(&network_connection.id))
                            {
                                let graph_widget = summary.graph_widget();

                                graph_widget.add_data_point(vec![
                                    vec![network_connection.tx_rate_bytes_ps],
                                    vec![network_connection.rx_rate_bytes_ps],
                                ]);

                                let grey_out =
                                    network_connection.state() == ConnectionState::Unavailable;
                                summary.set_opacity(if grey_out { 0.6 } else { 1. });

                                if network_connection.state() != ConnectionState::Connected {
                                    summary.set_info1(i18n_f(
                                        "{}",
                                        &[network_connection.state().as_str_name()],
                                    ));
                                    summary.set_info2("");
                                } else {
                                    let send_speed = network_connection.tx_rate_bytes_ps;
                                    let rec_speed = network_connection.rx_rate_bytes_ps;

                                    let sent_speed = crate::to_human_readable_nice(
                                        send_speed,
                                        &DataType::NetworkBytesPerSecond,
                                    );
                                    let rect_speeed = crate::to_human_readable_nice(
                                        rec_speed,
                                        &DataType::NetworkBytesPerSecond,
                                    );

                                    summary.set_info1(i18n_f("{}: {}", &["S", &sent_speed]));
                                    summary.set_info2(i18n_f("{}: {}", &["R", &rect_speeed]));
                                }

                                result &= page.update_readings(network_connection);
                            } else {
                                new_devices.push(index);
                            }
                        }

                        for new_device_index in new_devices {
                            let (net_if_id, page) = this.imp().create_network_page(
                                &readings.network_connections[new_device_index],
                            );
                            pages.insert(net_if_id, page);
                        }
                    }
                    Pages::Gpu(pages) => {
                        let mut gpus = readings.gpus.iter().collect::<Vec<_>>();
                        gpus.sort_by(|(lhs, _), (rhs, _)| lhs.cmp(&rhs));

                        let hide_index = gpus.len() == 1;

                        let mut new_devices = Vec::new();
                        for (index, (id, gpu)) in gpus.drain(..).enumerate() {
                            let index = if hide_index { None } else { Some(index) };

                            if let Some((summary, page)) = pages.get(&Self::gpu_page_name(&gpu.id))
                            {
                                let graph_widget = summary.graph_widget();

                                if let Some(index) = index {
                                    summary.set_heading(i18n_f("GPU {}", &[&format!("{}", index)]));
                                } else {
                                    summary.set_heading(i18n("GPU"));
                                }

                                let mut info2 = ArrayString::<256>::new();
                                if let Some(v) = gpu.utilization_percent {
                                    graph_widget.add_data_point(vec![vec![v]]);
                                    let _ = write!(&mut info2, "{v}%");
                                }
                                if let Some(v) = gpu.temperature_c.map(|v| v.round() as u32) {
                                    let _ = write!(&mut info2, " ({v} °C)");
                                }
                                summary.set_info2(info2.as_str());

                                result &= page.update_readings(gpu, index);
                            } else {
                                new_devices.push((index, id.as_str()));
                            }
                        }

                        for (index, device_id) in new_devices {
                            let Some(gpu) = readings.gpus.get(device_id) else {
                                continue;
                            };

                            let (page_name, page) = this.imp().create_gpu_page(gpu, index);
                            pages.insert(page_name, page);
                        }
                    }
                    Pages::Fan(pages) => {
                        let hide_index = readings.fans.len() == 1;

                        let mut new_devices = Vec::new();
                        for (index, fan) in readings.fans.iter().enumerate() {
                            let index = if hide_index { None } else { Some(index) };

                            if let Some((summary, page)) = pages.get(&Self::fan_page_name(&fan)) {
                                let graph_widget = summary.graph_widget();
                                graph_widget.add_data_point(vec![vec![fan.rpm as f32]]);
                                if let Some(fan_name) = &fan.fan_label {
                                    summary.set_info1(fan_name.as_str());
                                } else if let Some(temp_name) = &fan.temp_name {
                                    summary.set_info1(temp_name.as_str());
                                }

                                if let Some(index) = index {
                                    summary.set_heading(i18n_f("Fan {}", &[&index.to_string()]));
                                } else {
                                    summary.set_heading(i18n("Fan"));
                                }

                                let temp_str = if let Some(temp_amount) = fan.temp_amount {
                                    format!(
                                        " ({:.0} °C)",
                                        (temp_amount as i32 + MK_TO_0_C) as f32 / 1000.0
                                    )
                                } else {
                                    String::new()
                                };

                                summary.set_info2(if let Some(pwm_percent) = fan.pwm_percent {
                                    format!("{:.0}%{}", pwm_percent * 100., temp_str)
                                } else {
                                    format!("{} RPM{}", fan.rpm, temp_str)
                                });
                                result &= page.update_readings(fan, index);
                            } else {
                                new_devices.push(index);
                            }
                        }

                        for index in new_devices {
                            let (page_name, page) = this.imp().create_fan_page(readings, index);
                            pages.insert(page_name, page);
                        }
                    }
                    Pages::Battery(pages) => {
                        let num_bat = readings.batteries.len();
                        let hide_index = num_bat == 1;

                        let mut new_devices = Vec::new();
                        for (index, battery) in readings.batteries.iter().enumerate() {
                            let index = if hide_index { None } else { Some(index) };

                            if let Some((summary, page)) =
                                pages.get(&Self::battery_page_name(&battery))
                            {
                                let graph_widget = summary.graph_widget();
                                graph_widget.add_data_point(vec![vec![battery.percentage * 100.]]);
                                summary.set_info1(
                                    battery.model.as_ref().unwrap_or(&String::new()).as_str(),
                                );
                                summary.set_info2(format!(
                                    "{:.0}%{}",
                                    battery.percentage * 100.,
                                    if let Some(temp) = battery.temp {
                                        format!(" ({} °C)", temp)
                                    } else {
                                        String::new()
                                    }
                                ));

                                if let Some(index) = index {
                                    summary
                                        .set_heading(i18n_f("Battery {}", &[&index.to_string()]));
                                } else {
                                    summary.set_heading(i18n("Battery"));
                                }

                                result &= page.update_readings(&battery, index);
                            } else {
                                new_devices.push(index);
                            }
                        }

                        for index in new_devices {
                            let (page_name, page) = this.imp().create_battery_page(readings, index);
                            pages.insert(page_name, page);
                        }
                    }
                }
            }

            this.imp().pages.set(pages);

            let summary_graphs = this.imp().summary_graphs.take();
            let unranked = {
                let rank = this.imp().sidebar_rank.borrow();
                summary_graphs
                    .keys()
                    .any(|graph| !rank.contains_key(graph.widget_name().as_str()))
            };
            this.imp().summary_graphs.set(summary_graphs);

            if unranked {
                this.imp().rebuild_sidebar_rank();
            }

            result
        }

        pub fn update_animations(this: &super::PerformancePage, ticks: AnimationFrame) -> bool {
            let mut pages = this.imp().pages.take();

            let mut result = true;

            for page in &mut pages {
                match page {
                    Pages::Cpu((summary, page)) => {
                        let graph_widget = summary.graph_widget();

                        result &= graph_widget.update_animation(ticks);
                        result &= page.update_animations(ticks);
                    }
                    Pages::Memory((summary, page)) => {
                        let graph_widget = summary.graph_widget();

                        result &= graph_widget.update_animation(ticks);
                        result &= page.update_animations(ticks);
                    }
                    Pages::Disk(pages) => {
                        for (summary, page) in pages.values() {
                            let graph_widget = summary.graph_widget();

                            result &= graph_widget.update_animation(ticks);
                            result &= page.update_animations(ticks);
                        }
                    }
                    Pages::Network(pages) => {
                        for (summary, page) in pages.values() {
                            let graph_widget = summary.graph_widget();

                            result &= graph_widget.update_animation(ticks);
                            result &= page.update_animations(ticks);
                        }
                    }
                    Pages::Gpu(pages) => {
                        for (summary, page) in pages.values() {
                            let graph_widget = summary.graph_widget();

                            result &= graph_widget.update_animation(ticks);
                            result &= page.update_animations(ticks);
                        }
                    }
                    Pages::Fan(pages) => {
                        for (summary, page) in pages.values() {
                            let graph_widget = summary.graph_widget();

                            result &= graph_widget.update_animation(ticks);
                            result &= page.update_animations(ticks);
                        }
                    }
                    Pages::Battery(pages) => {
                        for (summary, page) in pages.values() {
                            let graph_widget = summary.graph_widget();

                            result &= graph_widget.update_animation(ticks);
                            result &= page.update_animations(ticks);
                        }
                    }
                }
            }

            this.imp().pages.set(pages);

            result
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePage {
        const NAME: &'static str = "PerformancePage";
        type Type = super::PerformancePage;
        type ParentType = adw::BreakpointBin;

        fn class_init(klass: &mut Self::Class) {
            SummaryGraph::ensure_type();
            GraphWidget::ensure_type();
            CpuPage::ensure_type();
            NetworkPage::ensure_type();
            SidebarDropHint::ensure_type();
            Placeholder::ensure_type();

            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePage {
        fn properties() -> &'static [ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }

        fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
            self.derived_property(id, pspec)
        }

        fn constructed(&self) {
            self.parent_constructed();

            let this = self.obj().clone();

            let group = self.configure_actions();
            this.insert_action_group("graph", Some(&group));

            self.breakpoint.set_condition(Some(
                &adw::BreakpointCondition::parse("max-width: 570sp").unwrap(),
            ));
            self.breakpoint.connect_apply({
                let this = self.obj().downgrade();
                move |_| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    this.breakpoint_applied.set(true);
                    this.page_content.set_collapsed(true);
                    this.page_content.set_show_sidebar(false);
                }
            });
            self.breakpoint.connect_unapply({
                let this = self.obj().downgrade();
                move |_| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    this.breakpoint_applied.set(false);
                    this.page_content.set_collapsed(false);
                    if !this.summary_mode.get() {
                        this.page_content.set_show_sidebar(true);
                    } else {
                        this.page_content.set_show_sidebar(false);
                    }
                }
            });

            self.page_content
                .sidebar()
                .expect("Infobar is not set")
                .parent()
                .and_then(|p| Some(p.remove_css_class("sidebar-pane")));
            self.page_content.connect_collapsed_notify({
                let this = self.obj().downgrade();
                move |pc| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if !pc.is_collapsed() {
                        this.page_content
                            .sidebar()
                            .expect("Infobar is not set")
                            .parent()
                            .and_then(|p| Some(p.remove_css_class("sidebar-pane")));

                        this.info_bar.set_halign(gtk::Align::Fill);
                    } else {
                        this.info_bar.set_halign(gtk::Align::Center);
                    }
                    this.obj().notify_info_button_visible();
                }
            });

            self.page_content.connect_show_sidebar_notify({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        this.notify_infobar_visible();
                    }
                }
            });

            if let Some(child) = self.page_stack.visible_child() {
                let infobar_content = child.property::<Option<gtk::Widget>>("infobar-content");
                self.info_bar.set_child(infobar_content.as_ref());
            }
            self.page_stack.connect_visible_child_notify({
                let this = self.obj().downgrade();
                move |page_stack| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    if let Some(child) = page_stack.visible_child() {
                        let infobar_content =
                            child.property::<Option<gtk::Widget>>("infobar-content");
                        this.imp().info_bar.set_child(infobar_content.as_ref());
                    }
                }
            });
        }
    }

    impl WidgetImpl for PerformancePage {}

    impl BreakpointBinImpl for PerformancePage {}

    #[cfg(test)]
    mod tests {
        use super::*;

        fn names(list: &[&str]) -> Vec<String> {
            list.iter().map(|name| name.to_string()).collect()
        }

        fn present(list: &[&str]) -> HashSet<String> {
            list.iter().map(|name| name.to_string()).collect()
        }

        #[test]
        fn prune_leaves_a_short_order_alone() {
            let mut order = names(&["cpu", "memory", "disk-sda"]);
            prune_sidebar_order(&mut order, &present(&["cpu"]), 256);
            assert_eq!(order, names(&["cpu", "memory", "disk-sda"]));
        }

        #[test]
        fn prune_drops_absent_devices_from_the_end() {
            let mut order = names(&["a", "b", "c", "d", "e"]);
            prune_sidebar_order(&mut order, &HashSet::new(), 3);
            assert_eq!(order, names(&["a", "b", "c"]));
        }

        #[test]
        fn prune_never_drops_a_present_device() {
            let mut order = names(&["a", "b", "c", "d", "e"]);
            prune_sidebar_order(&mut order, &present(&["d", "e"]), 3);
            assert_eq!(order, names(&["a", "d", "e"]));
        }

        #[test]
        fn prune_keeps_every_present_device_even_above_the_cap() {
            let mut order = names(&["a", "b", "c", "d", "e"]);
            prune_sidebar_order(&mut order, &present(&["a", "b", "c", "d", "e"]), 2);
            assert_eq!(order, names(&["a", "b", "c", "d", "e"]));
        }

        #[test]
        fn prune_keeps_the_cap_when_there_are_repeated_changes() {
            let mut order = names(&["cpu", "memory"]);

            for index in 0..1000 {
                let arriving = format!("net-veth{}", index);
                order.push(arriving.clone());
                prune_sidebar_order(
                    &mut order,
                    &present(&["cpu", "memory", arriving.as_str()]),
                    MAX_REMEMBERED_DEVICES,
                );
            }

            assert_eq!(order.len(), MAX_REMEMBERED_DEVICES);
            assert!(order.contains(&"cpu".to_string()));
            assert!(order.contains(&"memory".to_string()));
            assert!(order.contains(&"net-veth999".to_string()));
        }

        fn merged(stored: &[&str], present: &[&str]) -> Vec<String> {
            let mut order = names(stored);
            PerformancePage::merge_new_devices(&mut order, names(present));
            order
        }

        #[test]
        fn merge_into_an_empty_order_gives_default_order() {
            assert_eq!(
                merged(
                    &[],
                    &["net-eth1", "cpu", "disk-sdb", "net-eth0", "disk-sda", "memory"]
                ),
                names(&["cpu", "memory", "disk-sda", "disk-sdb", "net-eth0", "net-eth1"])
            );
        }

        #[test]
        fn merge_joins_a_sibling_the_user_dragged_to_the_top() {
            assert_eq!(
                merged(
                    &["net-eth0", "cpu", "memory", "disk-sda"],
                    &["cpu", "memory", "disk-sda", "net-eth0", "net-eth1"]
                ),
                names(&["net-eth0", "net-eth1", "cpu", "memory", "disk-sda"])
            );
        }

        #[test]
        fn merge_leaves_the_top_row_alone() {
            assert_eq!(
                merged(
                    &["net-eth1", "cpu", "memory"],
                    &["cpu", "memory", "net-eth0", "net-eth1"]
                ),
                names(&["net-eth1", "net-eth0", "cpu", "memory"])
            );
        }

        #[test]
        fn merge_rejoins_a_group_a_drag_split_in_two() {
            assert_eq!(
                merged(
                    &["net-eth0", "cpu", "net-eth2", "memory"],
                    &["cpu", "memory", "net-eth0", "net-eth1", "net-eth2"]
                ),
                names(&["net-eth0", "net-eth1", "cpu", "net-eth2", "memory"])
            );
        }

        #[test]
        fn merge_uses_the_default_position_for_a_new_category() {
            assert_eq!(
                merged(&["net-eth0", "disk-sda"], &["cpu", "disk-sda", "net-eth0"]),
                names(&["cpu", "net-eth0", "disk-sda"])
            );
        }

        #[test]
        fn merge_keeps_siblings_arriving_together_contiguous() {
            assert_eq!(
                merged(
                    &["cpu", "net-eth5"],
                    &["cpu", "net-eth1", "net-eth2", "net-eth5"]
                ),
                names(&["cpu", "net-eth5", "net-eth1", "net-eth2"])
            );
        }

        #[test]
        fn merge_puts_a_newcomer_next_to_the_sibling_it_sorts_after() {
            assert_eq!(
                merged(
                    &["net-eth0", "cpu", "memory", "disk-sda", "net-wlan0"],
                    &[
                        "cpu",
                        "memory",
                        "disk-sda",
                        "net-eth0",
                        "net-wlan0",
                        "net-veth1a2b3c"
                    ]
                ),
                names(&[
                    "net-eth0",
                    "net-veth1a2b3c",
                    "cpu",
                    "memory",
                    "disk-sda",
                    "net-wlan0"
                ])
            );
        }

        #[test]
        fn merge_does_not_change_order_once_every_device_is_known() {
            let stored = ["net-eth0", "cpu", "memory", "disk-sda"];
            let present = ["cpu", "memory", "disk-sda", "net-eth0"];

            let once = merged(&stored, &present);
            let mut twice = once.clone();
            PerformancePage::merge_new_devices(&mut twice, names(&present));

            assert_eq!(once, names(&stored));
            assert_eq!(twice, once);
        }

        #[test]
        fn canonical_key_orders_categories() {
            let ordered = [
                "cpu",
                "memory",
                "disk-sda",
                "net-eth0",
                "gpu-0",
                "fan-0-0",
                "battery-BAT0",
            ];

            for pair in ordered.windows(2) {
                assert!(
                    PerformancePage::canonical_key(pair[0])
                        < PerformancePage::canonical_key(pair[1]),
                    "{} should sort before {}",
                    pair[0],
                    pair[1]
                );
            }
        }

        #[test]
        fn canonical_key_puts_unknown_names_last() {
            let unknown = PerformancePage::canonical_key("something-else");
            assert_eq!(unknown.0, 7);
            assert!(PerformancePage::canonical_key("battery-BAT0") < unknown);
        }

        #[test]
        fn canonical_key_groups_a_category_together() {
            let mut sorted = names(&["net-eth1", "cpu", "disk-sdb", "net-eth0", "disk-sda"]);
            sorted.sort_by(|a, b| {
                PerformancePage::canonical_key(a).cmp(&PerformancePage::canonical_key(b))
            });

            assert_eq!(
                sorted,
                names(&["cpu", "disk-sda", "disk-sdb", "net-eth0", "net-eth1"])
            );
        }

        #[test]
        fn view_action_name_maps_every_category() {
            let expected = [
                ("cpu", "cpu"),
                ("memory", "memory"),
                ("disk-sda", "disk"),
                ("net-eth0", "network"),
                ("gpu-0000:01:00.0", "gpu"),
                ("fan-0-1", "fan"),
                ("battery-0-BAT0", "battery"),
            ];

            for (page_name, action) in expected {
                assert_eq!(
                    PerformancePage::view_action_name(page_name),
                    Some(action),
                    "{} should use the {} action",
                    page_name,
                    action
                );
            }
        }

        #[test]
        fn view_action_name_rejects_names_outside_the_categories() {
            for page_name in ["", "cpufreq", "netlink-0", "something-else"] {
                assert_eq!(PerformancePage::view_action_name(page_name), None);
            }
        }
    }
}

glib::wrapper! {
    pub struct PerformancePage(ObjectSubclass<imp::PerformancePage>)
        @extends adw::BreakpointBin, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PerformancePage {
    pub fn set_initial_readings(&self, readings: &crate::magpie_client::Readings) -> bool {
        let mut ok = imp::PerformancePage::set_up_pages(self, readings);
        ok &= imp::PerformancePage::update_readings(self, readings);

        // Data has arrived; swap out the placeholders in favor of the real content
        glib::idle_add_local_once({
            let this = self.downgrade();
            move || {
                let Some(this) = this.upgrade() else {
                    return;
                };
                let imp = this.imp();

                imp.content_stack.set_visible_child_name("content");
                imp.info_stack.set_visible_child_name("content");
            }
        });

        ok
    }

    pub fn update_readings(&self, readings: &crate::magpie_client::Readings) -> bool {
        imp::PerformancePage::update_readings(self, readings)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePage::update_animations(self, ticks)
    }

    pub fn select_nth_shown_device(&self, nth: i32) {
        let this = self.imp();
        let sidebar = this.sidebar();

        let mut shown = 0;
        let mut index = 0;
        while let Some(row) = sidebar.row_at_index(index) {
            if this.row_shown(&row) {
                if shown == nth {
                    sidebar.select_row(Some(&row));
                    return;
                }
                shown += 1;
            }

            index += 1;
        }
    }

    fn set_all_enabled(&self, enabled: bool) {
        let this = self.imp();

        if !this.sidebar_edit_mode.get() {
            return;
        }

        let settings = settings!();
        let current =
            parse_device_overrides(&settings.string("performance-sidebar-device-overrides"));
        let mut overrides = current.clone();

        let state = if enabled {
            DeviceOverride::Show
        } else {
            DeviceOverride::Hide
        };

        let summary_graphs = this.summary_graphs.take();
        for graph in summary_graphs.keys() {
            overrides.insert(graph.widget_name().to_string(), state);
            graph.set_switch_active(enabled);
        }
        this.summary_graphs.set(summary_graphs);

        if overrides == current {
            return;
        }

        settings
            .set_string(
                "performance-sidebar-device-overrides",
                &serialize_device_overrides(&overrides),
            )
            .unwrap_or_else(|_| {
                g_warning!(
                    "MissionCenter::PerformancePage",
                    "Failed to set performance-sidebar-device-overrides setting"
                );
            });
    }

    pub fn sidebar_enable_all(&self) {
        self.set_all_enabled(true);
    }

    pub fn sidebar_disable_all(&self) {
        self.set_all_enabled(false);
    }

    pub fn sidebar_reset_to_default(&self) {
        let this = self.imp();

        if !this.sidebar_edit_mode.get() {
            return;
        }

        let settings = settings!();

        settings
            .set_string("performance-sidebar-order", "")
            .unwrap_or_else(|_| {
                g_warning!(
                    "MissionCenter::PerformancePage",
                    "Failed to set performance-sidebar-order setting"
                );
            });
        settings
            .set_string("performance-sidebar-device-overrides", "")
            .unwrap_or_else(|_| {
                g_warning!(
                    "MissionCenter::PerformancePage",
                    "Failed to set performance-sidebar-device-overrides setting"
                );
            });

        for key in [
            "performance-show-disks",
            "performance-show-network",
            "performance-show-network-wired",
            "performance-show-network-wireless",
            "performance-show-network-vpn",
            "performance-show-network-virtual",
            "performance-show-network-other",
            "performance-show-gpus",
            "performance-show-fans",
            "performance-show-batteries",
        ] {
            settings.reset(key);
        }
    }
}
