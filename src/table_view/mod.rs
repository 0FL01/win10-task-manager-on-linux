/* table_view/mod.rs
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

use std::cell::RefCell;
use std::cell::{Cell, OnceCell};
use std::fmt::Write;

use adw::prelude::*;
use arrayvec::ArrayString;
use gtk::glib::{g_critical, g_debug, ParamSpec, Properties, Value};
use gtk::glib::{g_warning, VariantTy, WeakRef};
use gtk::ListScrollFlags;
use gtk::{gdk, gio, glib, subclass::prelude::*};
use textdistance::{Algorithm, Levenshtein};

use crate::i18n::i18n;
use crate::{app, settings, DataType};

use columns::*;
pub use models::*;
pub use process_action_bar::ProcessActionBar;
pub use process_details_dialog::ProcessDetailsDialog;
pub use row_model::{ContentType, RowModel, RowModelBuilder, SectionType};
pub use service_action_bar::ServiceActionBar;
pub use service_details_dialog::ServiceDetailsDialog;

pub mod cached_icon;
pub mod columns;
mod models;
mod process_action_bar;
mod process_details_dialog;
mod row_model;
mod service_action_bar;
mod service_details_dialog;
mod settings;

#[derive(Copy, Clone, Default)]
pub enum SettingsNamespace {
    #[default]
    AppsPage,
    ServicesPage,
}

impl SettingsNamespace {
    pub fn key_to_string(&self) -> &'static str {
        match self {
            SettingsNamespace::AppsPage => "apps-page",
            SettingsNamespace::ServicesPage => "services-page",
        }
    }

    #[inline]
    pub fn format_value(&self, value: &SettingsValues) -> String {
        format!("{}-{}", self.key_to_string(), value.key_to_string())
    }
}

// this only has settings that exist in all namespaces
#[derive(Copy, Clone, Default)]
pub enum SettingsValues {
    #[default]
    SortingColumnName,
    SortingOrder,
    ColumnOrder,
}

impl SettingsValues {
    pub fn key_to_string(&self) -> &'static str {
        match self {
            SettingsValues::SortingColumnName => "sorting-column-name",
            SettingsValues::SortingOrder => "sorting-order",
            SettingsValues::ColumnOrder => "column-order",
        }
    }
}

mod imp {
    use super::*;

    #[derive(Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::TableView)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/table_view/table_view.ui")]
    pub struct TableView {
        #[template_child]
        pub column_view: TemplateChild<gtk::ColumnView>,
        #[template_child]
        pub name_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub pid_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub cpu_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub memory_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub swap_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub drive_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub network_usage_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub gpu_usage_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub gpu_memory_column: TemplateChild<gtk::ColumnViewColumn>,
        #[template_child]
        pub context_menu: TemplateChild<gtk::PopoverMenu>,
        #[template_child]
        pub app_menu_model: TemplateChild<gio::MenuModel>,
        #[template_child]
        pub service_menu_model: TemplateChild<gio::MenuModel>,

        #[property(get, set)]
        pub show_column_separators: Cell<bool>,
        #[property(get)]
        pub selected_item: RefCell<RowModel>,
        #[property(get)]
        pub selected_item_running: Cell<bool>,
        #[property(get)]
        pub selected_item_enabled: Cell<bool>,

        pub row_sorter: OnceCell<gtk::TreeListRowSorter>,

        pub use_merged_stats: Cell<bool>,

        pub settings_namespace: Cell<SettingsNamespace>,

        service_state_connections: RefCell<[Option<glib::SignalHandlerId>; 2]>,

        filter: RefCell<gtk::CustomFilter>,

        pub filter_list_model: RefCell<Option<gtk::FilterListModel>>,
        pub sort_list_model: RefCell<Option<gtk::SortListModel>>,

        search_configured: Cell<bool>,
        search_setup_tries: Cell<u8>,
    }

    impl Default for TableView {
        fn default() -> Self {
            Self {
                column_view: Default::default(),
                name_column: Default::default(),
                pid_column: Default::default(),
                cpu_column: Default::default(),
                memory_column: Default::default(),
                swap_column: Default::default(),
                drive_column: Default::default(),
                network_usage_column: Default::default(),
                gpu_usage_column: Default::default(),
                gpu_memory_column: Default::default(),
                context_menu: Default::default(),
                app_menu_model: Default::default(),
                service_menu_model: Default::default(),

                show_column_separators: Cell::new(false),
                selected_item: RefCell::new(RowModelBuilder::new().build()),
                selected_item_running: Cell::new(false),
                selected_item_enabled: Cell::new(false),

                row_sorter: OnceCell::new(),

                use_merged_stats: Cell::new(false),

                settings_namespace: Cell::new(Default::default()),

                service_state_connections: RefCell::new([const { None }; 2]),

                filter: RefCell::new(gtk::CustomFilter::new(|_| true)),

                filter_list_model: RefCell::new(None),
                sort_list_model: RefCell::new(None),

                search_configured: Cell::new(false),
                search_setup_tries: Cell::new(1),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TableView {
        const NAME: &'static str = "TableView";
        type Type = super::TableView;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for TableView {
        fn properties() -> &'static [ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
            self.derived_set_property(id, value, pspec)
        }

        fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
            self.derived_property(id, pspec)
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.name_column
                .set_factory(Some(&name_list_item_factory()));
            self.name_column
                .set_sorter(Some(&name_sorter(&self.column_view)));

            self.pid_column.set_factory(Some(&pid_list_item_factory()));
            self.pid_column
                .set_sorter(Some(&pid_sorter(&self.column_view)));

            self.cpu_column.set_factory(Some(&cpu_list_item_factory()));
            self.cpu_column
                .set_sorter(Some(&cpu_sorter(&self.column_view)));

            self.memory_column
                .set_factory(Some(&memory_list_item_factory()));
            self.memory_column
                .set_sorter(Some(&memory_sorter(&self.column_view)));

            self.swap_column
                .set_factory(Some(&swap_list_item_factory()));
            self.swap_column
                .set_sorter(Some(&swap_sorter(&self.column_view)));

            self.drive_column
                .set_factory(Some(&drive_list_item_factory()));
            self.drive_column
                .set_sorter(Some(&drive_sorter(&self.column_view)));

            self.network_usage_column
                .set_factory(Some(&network_list_item_factory()));
            self.network_usage_column
                .set_sorter(Some(&network_sorter(&self.column_view)));

            self.gpu_usage_column
                .set_factory(Some(&gpu_list_item_factory()));
            self.gpu_usage_column
                .set_sorter(Some(&gpu_sorter(&self.column_view)));

            self.gpu_memory_column
                .set_factory(Some(&gpu_memory_list_item_factory()));
            self.gpu_memory_column
                .set_sorter(Some(&gpu_memory_sorter(&self.column_view)));

            let action_group = gio::SimpleActionGroup::new();

            let action_show_context_menu =
                gio::SimpleAction::new("show-context-menu", Some(VariantTy::TUPLE));
            action_show_context_menu.connect_activate({
                let this = self.obj().downgrade();
                move |_action, entry| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    let Some(model) = imp.column_view.model() else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to get model for `show-context-menu` action"
                        );
                        return;
                    };

                    let Some((id, x, y, w, h)) =
                        entry.and_then(|s| s.get::<(String, f64, f64, f64, f64)>())
                    else {
                        g_critical!(
                            "MissionCenter::TableView",
                            "Failed to get item id and anchor rect from show-context-menu action"
                        );
                        return;
                    };

                    if select_item(&model, &id) {
                        let context_menu = &imp.context_menu;

                        match imp.selected_item.borrow().content_type() {
                            ContentType::Process | ContentType::App => {
                                context_menu.set_menu_model(Some(&imp.app_menu_model.get()))
                            }
                            ContentType::Service => {
                                context_menu.set_menu_model(Some(&imp.service_menu_model.get()))
                            }
                            _ => {
                                return;
                            }
                        }

                        let anchor = calculate_anchor_point(&this, x, y, w, h);
                        context_menu.set_pointing_to(Some(&anchor));
                        context_menu.popup();
                    }
                }
            });

            action_group.add_action(&action_show_context_menu);

            let expand_row_action = gio::SimpleAction::new("tableview-expand-row", None);
            expand_row_action.connect_activate({
                let this = self.obj().downgrade();
                move |_action, _| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    let Some(model) = imp
                        .column_view
                        .model()
                        .and_then(|model| model.downcast::<gtk::SingleSelection>().ok())
                    else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to get model for `tableview-expand-row` action"
                        );
                        return;
                    };

                    let Some(selected_item) = model.selected_item() else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to get selection for `tableview-expand-row` action"
                        );
                        return;
                    };

                    let Some(row) = selected_item.downcast_ref::<gtk::TreeListRow>() else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to downcast selection for `tableview-expand-row` action"
                        );
                        return;
                    };

                    row.set_expanded(true);
                }
            });
            action_group.add_action(&expand_row_action);

            let collapse_row_action = gio::SimpleAction::new("tableview-collapse-row", None);
            collapse_row_action.connect_activate({
                let this = self.obj().downgrade();
                move |_action, _| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    let Some(model) = imp
                        .column_view
                        .model()
                        .and_then(|model| model.downcast::<gtk::SingleSelection>().ok())
                    else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to get model for `tableview-expand-row` action"
                        );
                        return;
                    };

                    let Some(selected_item) = model.selected_item() else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to get selection for `tableview-collapse-row` action"
                        );
                        return;
                    };

                    let Some(row) = selected_item.downcast_ref::<gtk::TreeListRow>() else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to downcast selection for `tableview-collapse-row` action"
                        );
                        return;
                    };

                    if row.is_expanded() && row.children().map(|c| c.n_items() > 0).unwrap_or(true)
                    {
                        row.set_expanded(false);
                    } else if row.depth() > 0 {
                        let parent = row.parent().unwrap();

                        let mut parent_index = (0..model.n_items())
                            .filter_map(|i| model.item(i).map(|item| (i, item)))
                            .filter_map(|(index, item)| {
                                item.downcast::<gtk::TreeListRow>()
                                    .map(|item| (index, item))
                                    .ok()
                            })
                            .filter(|(_, item)| item.eq(&parent))
                            .map(|(index, _)| index);

                        if let Some(parent_index) = parent_index.next() {
                            imp.column_view.scroll_to(
                                parent_index,
                                None,
                                ListScrollFlags::SELECT | ListScrollFlags::FOCUS,
                                None,
                            );
                        }
                    }
                }
            });
            action_group.add_action(&collapse_row_action);

            self.column_view.connect_activate({
                let this = self.obj().downgrade();

                move |_click, _n_press| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    let Some(model) = imp
                        .column_view
                        .model()
                        .and_then(|model| model.downcast::<gtk::SingleSelection>().ok())
                    else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to get model for `tableview-expand-row` action"
                        );
                        return;
                    };

                    let Some(selected_item) = model.selected_item() else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to get selection for `tableview-collapse-row` action"
                        );
                        return;
                    };

                    let Some(row) = selected_item.downcast_ref::<gtk::TreeListRow>() else {
                        g_critical!(
                            "MissionCenter::ProcessActionBar",
                            "Failed to downcast selection for `tableview-collapse-row` action"
                        );
                        return;
                    };

                    row.set_expanded(!row.is_expanded());
                }
            });

            self.obj()
                .insert_action_group("column-view", Some(&action_group));
        }
    }

    impl WidgetImpl for TableView {
        fn realize(&self) {
            self.parent_realize();

            // Delay connecting to the search field until Widget::realize to make reasonably sure
            // that the window exists.
            if !self.configure_search() {
                const MAX_SEARCH_SETUP_TRIES: u8 = 5;

                glib::timeout_add_local(std::time::Duration::from_millis(100), {
                    let this = self.obj().downgrade();
                    move || {
                        let Some(this) = this.upgrade() else {
                            return glib::ControlFlow::Break;
                        };
                        let imp = this.imp();

                        imp.search_setup_tries.update(|tries| tries + 1);
                        if imp.search_setup_tries.get() > MAX_SEARCH_SETUP_TRIES {
                            g_warning!(
                                "MissionCenter::TableView",
                                "Failed to configure search after {} tries, giving up",
                                MAX_SEARCH_SETUP_TRIES
                            );
                            return glib::ControlFlow::Break;
                        }

                        if imp.configure_search() {
                            return glib::ControlFlow::Break;
                        }

                        g_critical!(
                            "MissionCenter::TableView",
                            "Failed to configure search, retrying..."
                        );

                        glib::ControlFlow::Continue
                    }
                });

                g_debug!(
                    "MissionCenter::TableView",
                    "Failed to configure search on first try, retrying..."
                );
            }

            let column_view_title = self.column_view.first_child();
            adjust_view_header_alignment(column_view_title);
        }
    }

    impl BoxImpl for TableView {}

    impl TableView {
        pub fn setup<const TOGGLE_COUNT: usize>(
            &self,
            settings_namespace: SettingsNamespace,
            section_item_1: &RowModel,
            section_item_2: &RowModel,
            process_action_bar: Option<&ProcessActionBar>,
            service_action_bar: Option<&ServiceActionBar>,
            service_toggle_group: Option<[WeakRef<gtk::ToggleButton>; TOGGLE_COUNT]>,
        ) {
            self.settings_namespace.set(settings_namespace);

            self.update_column_order();

            let model = gio::ListStore::new::<RowModel>();
            model.append(section_item_1);
            model.append(section_item_2);

            let tree_model = Self::create_tree_model(model);
            let filter_list_model = self.configure_filter(tree_model, service_toggle_group);
            self.filter_list_model
                .replace(Some(filter_list_model.clone()));
            let (sort_list_model, row_sorter) = self.setup_filter_model(filter_list_model);
            self.sort_list_model.replace(Some(sort_list_model.clone()));
            let selection_model = self.setup_selection_model(sort_list_model);
            self.column_view.set_model(Some(&selection_model));

            let _ = self.row_sorter.set(row_sorter);

            selection_model.set_selected(0);

            if let Some(process_action_bar) = process_action_bar {
                process_action_bar.set_column_view(&self.obj());
            }

            if let Some(service_action_bar) = service_action_bar {
                service_action_bar.set_column_view(&self.obj());
            }

            settings::configure(&self.obj());
        }

        fn create_tree_model(model: impl IsA<gio::ListModel>) -> gtk::TreeListModel {
            gtk::TreeListModel::new(model, false, true, move |model_entry| {
                let Some(row_model) = model_entry.downcast_ref::<RowModel>() else {
                    return None;
                };
                Some(row_model.children().clone().into())
            })
        }

        fn configure_filter<const TOGGLE_COUNT: usize>(
            &self,
            tree_list_model: impl IsA<gio::ListModel>,
            group: Option<[WeakRef<gtk::ToggleButton>; TOGGLE_COUNT]>,
        ) -> gtk::FilterListModel {
            let group_clone = group.clone();
            let filter = gtk::CustomFilter::new(move |obj| {
                let Some(row_model) = obj
                    .downcast_ref::<gtk::TreeListRow>()
                    .and_then(|row| row.item())
                    .and_then(|item| item.downcast::<RowModel>().ok())
                else {
                    return false;
                };

                let Some(window) = app!().window() else {
                    g_debug!(
                        "MissionCenter::ProcessTree",
                        "Failed to get window after initiating search. Where did the search come from then?"
                    );
                    return true;
                };

                let search = || {
                    let window = window.imp();

                    if !window.search_button.is_active() {
                        return true;
                    }

                    if window.header_search_entry.text().is_empty() {
                        return true;
                    }

                    if row_model.content_type() == ContentType::SectionHeader {
                        return true;
                    }

                    let entry_name = row_model.name().to_lowercase();
                    let pid = row_model.pid().to_string();
                    let search_query = window.header_search_entry.text().to_lowercase();

                    if entry_name.contains(&search_query) || pid.contains(&search_query) {
                        return true;
                    }

                    if search_query.contains(&entry_name) || search_query.contains(&pid) {
                        return true;
                    }

                    let str_distance = Levenshtein::default()
                        .for_str(&entry_name, &search_query)
                        .ndist();
                    if str_distance <= 0.6 {
                        return true;
                    }

                    false
                };

                let group = group_clone.clone();
                let row_model_clone = row_model.clone();
                let filter = move || {
                    let Some(group) = group else {
                        return true;
                    };

                    if row_model_clone.content_type() == ContentType::SectionHeader {
                        return true;
                    }

                    if group.iter().all(|toggle| {
                        toggle
                            .upgrade()
                            .map(|toggle| !toggle.is_active())
                            .unwrap_or(true)
                    }) {
                        return true;
                    }

                    let mut visible = [false; TOGGLE_COUNT];
                    for (i, toggle) in group.iter().enumerate() {
                        if let Some(toggle) = toggle.upgrade() {
                            let name = toggle.widget_name();
                            match name.as_str() {
                                "toggle_running" => {
                                    visible[i] =
                                        toggle.is_active() && row_model_clone.service_running()
                                }
                                "toggle_failed" => {
                                    visible[i] =
                                        toggle.is_active() && row_model_clone.service_failed()
                                }
                                "toggle_stopped" => {
                                    visible[i] =
                                        toggle.is_active() && row_model_clone.service_stopped()
                                }
                                "toggle_disabled" => {
                                    visible[i] = toggle.is_active()
                                        && !row_model_clone.service_enabled()
                                        && !row_model_clone.service_running()
                                        && !row_model_clone.service_failed();
                                }
                                _ => {
                                    g_warning!(
                                        "MissionCenter::TableView",
                                        "Unknown toggle button: {}",
                                        name
                                    );
                                }
                            };
                        }
                    }

                    visible.iter().any(|b| *b)
                };

                search() && filter()
            });

            if let Some(group) = group {
                for toggle in &group {
                    if let Some(toggle) = toggle.upgrade() {
                        toggle.connect_toggled({
                            let filter = filter.downgrade();
                            move |_| {
                                if let Some(filter) = filter.upgrade() {
                                    filter.changed(gtk::FilterChange::Different);
                                }
                            }
                        });
                    }
                }
            }

            self.filter.replace(filter.clone());

            let model = gtk::FilterListModel::new(Some(tree_list_model), Some(filter));
            model
        }

        fn setup_filter_model(
            &self,
            filter_list_model: impl IsA<gio::ListModel>,
        ) -> (gtk::SortListModel, gtk::TreeListRowSorter) {
            let column_view_sorter = self.column_view.sorter();

            let sorting_settings_key = self.format_settings_key(&SettingsValues::SortingColumnName);
            let sorting_order_settings_key =
                self.format_settings_key(&SettingsValues::SortingOrder);

            if let Some(column_view_sorter) = column_view_sorter.as_ref() {
                column_view_sorter.connect_changed({
                    move |sorter, _| {
                        let settings = settings!();

                        let Some(sorter) = sorter.downcast_ref::<gtk::ColumnViewSorter>() else {
                            return;
                        };

                        let Some(sorted_column) = sorter.primary_sort_column() else {
                            return;
                        };

                        let Some(sorted_column_id) = sorted_column.id() else {
                            return;
                        };
                        let _ =
                            settings.set_string(&sorting_settings_key, sorted_column_id.as_str());

                        let sort_order = sorter.primary_sort_order();
                        let _ = settings.set_enum(
                            &sorting_order_settings_key,
                            match sort_order {
                                gtk::SortType::Ascending => gtk::ffi::GTK_SORT_ASCENDING,
                                gtk::SortType::Descending => gtk::ffi::GTK_SORT_DESCENDING,
                                _ => gtk::ffi::GTK_SORT_ASCENDING,
                            },
                        );
                    }
                });
            }

            let tree_list_sorter = gtk::TreeListRowSorter::new(column_view_sorter);
            let model =
                gtk::SortListModel::new(Some(filter_list_model), Some(tree_list_sorter.clone()));
            (model, tree_list_sorter)
        }

        fn setup_selection_model(
            &self,
            sort_list_model: impl IsA<gio::ListModel>,
        ) -> gtk::SingleSelection {
            let selection_model = gtk::SingleSelection::new(Some(sort_list_model));
            selection_model.set_autoselect(true);

            let this = self.obj().downgrade();

            selection_model.connect_selected_item_notify({
                move |model| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };

                    let imp = this.imp();

                    let Some(row_model) = model
                        .selected_item()
                        .and_then(|item| item.downcast::<gtk::TreeListRow>().ok())
                        .and_then(|row| row.item())
                        .and_then(|obj| obj.downcast::<RowModel>().ok())
                    else {
                        return;
                    };

                    {
                        let mut service_state_connections =
                            imp.service_state_connections.borrow_mut();

                        for conn in &mut *service_state_connections {
                            if let Some(conn) = conn.take() {
                                imp.selected_item.borrow().disconnect(conn);
                            }
                        }

                        if row_model.content_type() == ContentType::Service {
                            service_state_connections[0] =
                                Some(row_model.connect_service_running_notify({
                                    let this = this.downgrade();
                                    move |row_model| {
                                        let Some(this) = this.upgrade() else {
                                            return;
                                        };

                                        let imp = this.imp();
                                        imp.selected_item_running.set(row_model.service_running());
                                        this.notify_selected_item_running();
                                    }
                                }));
                            service_state_connections[1] =
                                Some(row_model.connect_service_enabled_notify({
                                    let this = this.downgrade();
                                    move |row_model| {
                                        let Some(this) = this.upgrade() else {
                                            return;
                                        };

                                        let imp = this.imp();
                                        imp.selected_item_enabled.set(row_model.service_enabled());
                                        this.notify_selected_item_enabled();
                                    }
                                }));

                            imp.selected_item_running.set(row_model.service_running());
                            imp.selected_item_enabled.set(row_model.service_enabled());
                        } else {
                            imp.selected_item_running.set(false);
                            imp.selected_item_enabled.set(false);
                        }
                    }

                    imp.selected_item.replace(row_model);
                    this.notify_selected_item();
                    this.notify_selected_item_running();
                    this.notify_selected_item_enabled();
                }
            });

            selection_model
        }

        pub fn update_column_titles(&self, readings: &crate::magpie_client::Readings) {
            let mut buffer = ArrayString::<128>::new();

            let cpu_usage = readings.cpu.total_usage_percent.round() as u32;
            let _ = write!(&mut buffer, "{}\n{}%", i18n("CPU"), cpu_usage);
            self.cpu_column.set_title(Some(buffer.as_str()));

            buffer.clear();

            let mem_total = if readings.mem_info.mem_total > 0 {
                readings.mem_info.mem_total
            } else {
                1
            };

            // https://gitlab.com/procps-ng/procps/-/blob/master/library/meminfo.c?ref_type=heads#L736
            let mem_avail = if readings.mem_info.mem_available > readings.mem_info.mem_total {
                readings.mem_info.mem_free
            } else {
                readings.mem_info.mem_available
            };

            let memory_used = mem_total.saturating_sub(mem_avail);
            let memory_usage = memory_used as f32 * 100. / mem_total as f32;
            let memory_usage = memory_usage.round() as u32;
            let _ = write!(&mut buffer, "{}\n{}%", i18n("Memory"), memory_usage);
            self.memory_column.set_title(Some(buffer.as_str()));

            buffer.clear();
            let drive_usage = readings
                .disks_info
                .iter()
                .filter(|disk| disk.capacity_bytes > 0)
                .map(|disk| disk.busy_percent)
                .fold(0., f32::max);
            let drive_usage = drive_usage.round() as u32;
            let _ = write!(&mut buffer, "{}\n{}%", i18n("Drive"), drive_usage);
            self.drive_column.set_title(Some(buffer.as_str()));

            buffer.clear();
            if readings.mem_info.swap_total != 0 {
                let _ = write!(
                    &mut buffer,
                    "{}\n{:.0}%",
                    i18n("Swap"),
                    100. - (readings.mem_info.swap_free as f64 * 100.)
                        / (readings.mem_info.swap_total as f64)
                );
            } else {
                let _ = write!(&mut buffer, "{}\n0%", i18n("Swap"));
            }
            self.swap_column.set_title(Some(buffer.as_str()));

            buffer.clear();
            if readings.running_processes.is_empty() {
                let _ = write!(&mut buffer, "{}\n0", i18n("Network"));
            } else {
                let mut sum = 0.;
                for proc in readings.running_processes.values() {
                    sum += proc.usage_stats.network_usage.round();
                }

                let label = crate::to_human_readable_nice(sum, &DataType::NetworkBytesPerSecond);

                let _ = write!(&mut buffer, "{}\n{}", i18n("Network"), label);
            }
            self.network_usage_column.set_title(Some(buffer.as_str()));

            buffer.clear();
            if readings.gpus.is_empty() {
                let _ = write!(&mut buffer, "{}\n0%", i18n("GPU"));
                self.gpu_usage_column.set_title(Some(buffer.as_str()));

                buffer.clear();
                let _ = write!(&mut buffer, "{}\n0%", i18n("GPU Memory"));
                self.gpu_memory_column.set_title(Some(buffer.as_str()));
            } else {
                let mut sum_util = 0.;
                let mut sum_mem_used = 0.;
                let mut sum_mem_total = 0.;
                for gpu in readings.gpus.values() {
                    sum_util += gpu.utilization_percent.unwrap_or(0.);
                    sum_mem_used += gpu.used_memory.unwrap_or(0) as f32;
                    sum_mem_total += gpu.total_memory.unwrap_or(0) as f32;
                }
                let gpu_usage = sum_util / readings.gpus.len() as f32;
                let gpu_usage = gpu_usage.round() as u32;
                let _ = write!(&mut buffer, "{}\n{}%", i18n("GPU"), gpu_usage);
                self.gpu_usage_column.set_title(Some(buffer.as_str()));

                buffer.clear();
                let gpu_mem_usage = sum_mem_used * 100. / sum_mem_total;
                let gpu_mem_usage = gpu_mem_usage.round() as u32;
                let _ = write!(&mut buffer, "{}\n{}%", i18n("GPU Memory"), gpu_mem_usage);
                self.gpu_memory_column.set_title(Some(buffer.as_str()));
            }
        }

        pub fn update_column_order(&self) {
            let column_view = &self.column_view;

            let settings = settings!();

            let order_key = &self.format_settings_key(&SettingsValues::ColumnOrder);

            if settings.boolean("apps-page-remember-column-order") {
                let columns = column_view.columns();
                let mut all_columns = Vec::new();
                for i in 0..columns.n_items() {
                    let Some(column) = columns
                        .item(i)
                        .and_then(|c| c.downcast::<gtk::ColumnViewColumn>().ok())
                    else {
                        continue;
                    };
                    all_columns.push(column);
                }
                for column in &all_columns {
                    column_view.remove_column(column);
                }

                let setting_column_order = settings.string(order_key);
                for column_id in setting_column_order.split(';') {
                    let Some((index, column)) = all_columns
                        .iter()
                        .enumerate()
                        .find(|(_, c)| {
                            let Some(id) = c.id() else {
                                return false;
                            };
                            id == column_id
                        })
                        .map(|(index, c)| (index, c.clone()))
                    else {
                        continue;
                    };
                    all_columns.remove(index);
                    column_view.append_column(&column);
                }

                for column in all_columns.drain(..) {
                    column_view.append_column(&column);
                }
            } else {
                let _ = settings.set_string(order_key, "");
            }

            column_view.columns().connect_items_changed({
                let order_key = order_key.clone();
                move |model, _, _, _| {
                    let settings = settings!();

                    let mut order = String::new();
                    for i in 0..model.n_items() {
                        let Some(id) = model
                            .item(i)
                            .and_then(|c| c.downcast::<gtk::ColumnViewColumn>().ok())
                            .and_then(|c| c.id())
                        else {
                            continue;
                        };

                        order.push_str(id.as_str());
                        order.push(';');
                    }
                    order.pop();

                    let _ = settings.set_string(&order_key, order.as_str());
                }
            });
        }

        #[inline]
        pub fn format_settings_key(&self, key: &SettingsValues) -> String {
            self.settings_namespace.get().format_value(key)
        }

        fn configure_search(&self) -> bool {
            if self.search_configured.get() {
                return true;
            }

            let Some(window) = app!().window() else {
                return false;
            };

            let filter = self.filter.borrow().downgrade();
            window
                .imp()
                .header_search_entry
                .connect_search_changed(move |_| {
                    if let Some(filter) = filter.upgrade() {
                        filter.changed(gtk::FilterChange::Different);
                    }
                });

            self.search_configured.set(true);
            true
        }
    }
}

glib::wrapper! {
    pub struct TableView(ObjectSubclass<imp::TableView>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl TableView {
    pub fn set_use_merged_stats(&self, use_merged: bool) {
        self.imp().use_merged_stats.set(use_merged);
    }

    pub fn column_view(&self) -> &gtk::ColumnView {
        &self.imp().column_view
    }

    #[inline]
    pub fn format_settings_key(&self, key: &SettingsValues) -> String {
        self.imp().format_settings_key(key)
    }
}

// The anchor rect arrives in root coordinates (see `ListCell`); translate both of its corners
// into the menu parent's coordinate space so any transform between the two is honored.
fn calculate_anchor_point(
    menu_parent: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> gdk::Rectangle {
    let Some(root) = menu_parent.as_ref().root() else {
        g_warning!(
            "MissionCenter::TableView",
            "Failed to get root widget, popup will display in an arbitrary location"
        );
        return gdk::Rectangle::new(0, 0, 0, 0);
    };

    let p1 = root.compute_point(menu_parent, &gtk::graphene::Point::new(x as _, y as _));
    let p2 = root.compute_point(
        menu_parent,
        &gtk::graphene::Point::new((x + w) as _, (y + h) as _),
    );

    match (p1, p2) {
        (Some(p1), Some(p2)) => gdk::Rectangle::new(
            p1.x().round() as i32,
            p1.y().round() as i32,
            (p2.x() - p1.x()).round() as i32,
            (p2.y() - p1.y()).round() as i32,
        ),
        _ => {
            g_critical!(
                "MissionCenter::TableView",
                "Failed to compute_point, context menu will not be anchored to mouse position"
            );
            gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                w.round() as i32,
                h.round() as i32,
            )
        }
    }
}

fn select_item(model: &gtk::SelectionModel, id: &str) -> bool {
    for i in 0..model.n_items() {
        if let Some(item) = model
            .item(i)
            .and_then(|i| i.downcast::<gtk::TreeListRow>().ok())
            .and_then(|row| row.item())
            .and_then(|obj| obj.downcast::<RowModel>().ok())
        {
            if item.content_type() != ContentType::SectionHeader && item.id() == id {
                model.select_item(i, false);
                return true;
            }
        }
    }

    false
}
