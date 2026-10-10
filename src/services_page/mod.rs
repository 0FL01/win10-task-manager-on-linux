/* services_page/mod.rs
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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write;

use adw::prelude::*;
use glib::{g_critical, ParamSpec, Properties, Value, WeakRef};
use gtk::{gio, glib, subclass::prelude::*};

use crate::i18n::{i18n, ni18n_f};
use crate::table_view::{
    update_services, ContentType, ProcessActionBar, RowModel, RowModelBuilder, SectionType,
    ServiceActionBar, SettingsNamespace, TableView,
};
use crate::widgets::Placeholder;

pub mod actions;

mod imp {
    use super::*;

    #[derive(Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::ServicesPage)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/services_page/page.ui")]
    pub struct ServicesPage {
        #[property(get, set)]
        collapsed: RefCell<bool>,

        #[template_child]
        pub h1: TemplateChild<gtk::Label>,
        #[template_child]
        pub h2: TemplateChild<gtk::Label>,

        #[template_child]
        pub toggle_running: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub toggle_failed: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub toggle_stopped: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub toggle_disabled: TemplateChild<gtk::ToggleButton>,

        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub table_view: TemplateChild<TableView>,

        #[template_child]
        pub process_action_bar: TemplateChild<ProcessActionBar>,
        #[template_child]
        pub service_action_bar: TemplateChild<ServiceActionBar>,

        pub user_section: RowModel,
        pub system_section: RowModel,

        pub use_merged_stats: Cell<bool>,

        pub total_services: Cell<u32>,
        pub running_services: Cell<u32>,
        pub failed_services: Cell<u32>,
        pub stopped_services: Cell<u32>,
        pub disabled_services: Cell<u32>,

        pub has_loaded: Cell<bool>,
    }

    impl ServicesPage {
        pub fn update_headers(&self) {
            let mut fmt_buffer = arrayvec::ArrayString::<12>::new();

            let total = self.total_services.get();
            let running = self.running_services.get();
            let stopped = self.stopped_services.get();
            let failed = self.failed_services.get();
            let disabled = self.disabled_services.get();

            fmt_buffer.clear();
            let _ = write!(fmt_buffer, "{}", total);
            self.h1.set_label(&ni18n_f(
                "{} Total Service",
                "{} Total Services",
                total,
                &[fmt_buffer.as_str()],
            ));

            let mut types = String::with_capacity(50);
            let mut any_active = false;
            let mut filtered = 0;
            if self.toggle_running.is_active() {
                any_active = true;
                filtered += running;
                types.push_str(&i18n("Running"));
            }

            if self.toggle_failed.is_active() {
                any_active = true;
                filtered += failed;
                if !types.is_empty() {
                    types.push_str(", ");
                }
                types.push_str(&i18n("Failed"));
            }

            if self.toggle_stopped.is_active() {
                any_active = true;
                filtered += stopped;
                if !types.is_empty() {
                    types.push_str(", ");
                }
                types.push_str(&i18n("Stopped"));
            }

            if self.toggle_disabled.is_active() {
                any_active = true;
                filtered += disabled;
                if !types.is_empty() {
                    types.push_str(", ");
                }
                types.push_str(&i18n("Disabled"));
            }

            if filtered == 0 {
                if any_active {
                    self.h2
                        .set_label(&i18n("No services match the current filters"));
                } else {
                    self.h2.set_label(&i18n("No filters applied"));
                }
            } else {
                fmt_buffer.clear();
                let _ = write!(fmt_buffer, "{}", filtered);
                // TRANSLATORS: {0} is a number, {1} is a comma-separated list of service states, i.e. "Running", "Failed", "Stopped", "Disabled"
                self.h2.set_label(&ni18n_f(
                    "{} {} Service",
                    "{} {} Services",
                    filtered,
                    &[fmt_buffer.as_str(), &types],
                ));
            }
        }
    }

    impl Default for ServicesPage {
        fn default() -> Self {
            Self {
                collapsed: RefCell::new(false),

                h1: Default::default(),
                h2: Default::default(),

                toggle_running: Default::default(),
                toggle_failed: Default::default(),
                toggle_stopped: Default::default(),
                toggle_disabled: Default::default(),

                content_stack: Default::default(),
                table_view: Default::default(),

                process_action_bar: Default::default(),
                service_action_bar: Default::default(),

                user_section: RowModelBuilder::new()
                    .name(&i18n("User Services"))
                    .content_type(ContentType::SectionHeader)
                    .section_type(SectionType::FirstSection)
                    .build(),
                system_section: RowModelBuilder::new()
                    .name(&i18n("System Services"))
                    .content_type(ContentType::SectionHeader)
                    .section_type(SectionType::SecondSection)
                    .build(),

                use_merged_stats: Cell::new(false),

                total_services: Cell::new(0),
                running_services: Cell::new(0),
                failed_services: Cell::new(0),
                stopped_services: Cell::new(0),
                disabled_services: Cell::new(0),

                has_loaded: Cell::new(false),
            }
        }
    }

    impl ServicesPage {
        pub fn collapse(&self) {
            self.process_action_bar.imp().collapse();
            self.service_action_bar.imp().collapse();

            self.obj().set_collapsed(true);

            self.update_headers();
        }

        pub fn expand(&self) {
            self.process_action_bar.imp().expand();
            self.service_action_bar.imp().expand();

            self.obj().set_collapsed(false);

            self.update_headers();
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ServicesPage {
        const NAME: &'static str = "ServicesPage";
        type Type = super::ServicesPage;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            RowModel::ensure_type();
            Placeholder::ensure_type();

            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ServicesPage {
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

            let update_headers = |this: &WeakRef<super::ServicesPage>| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                this.imp().update_headers();
            };

            self.toggle_running.connect_toggled({
                let this = self.obj().downgrade();
                move |_| {
                    update_headers(&this);
                }
            });

            self.toggle_failed.connect_toggled({
                let this = self.obj().downgrade();
                move |_| {
                    update_headers(&this);
                }
            });

            self.toggle_stopped.connect_toggled({
                let this = self.obj().downgrade();
                move |_| {
                    update_headers(&this);
                }
            });

            self.toggle_disabled.connect_toggled({
                let this = self.obj().downgrade();
                move |_| {
                    update_headers(&this);
                }
            });

            let actions = gio::SimpleActionGroup::new();

            let action_collapse_all = gio::SimpleAction::new("collapse-all", None);
            actions.add_action(&action_collapse_all);
            action_collapse_all.connect_activate({
                let this = self.obj().downgrade();
                move |_action, _| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    let Some(selection_model) = imp
                        .table_view
                        .imp()
                        .column_view
                        .model()
                        .and_then(|model| model.downcast::<gtk::SingleSelection>().ok())
                    else {
                        g_critical!(
                            "MissionCenter::AppsPage",
                            "Failed to get model for `collapse-all` action"
                        );
                        return;
                    };

                    let mut count = 0;
                    for i in 0..selection_model.n_items() {
                        let Some(row) = selection_model
                            .item(i)
                            .and_then(|item| item.downcast::<gtk::TreeListRow>().ok())
                        else {
                            return;
                        };

                        let Some(row_model) =
                            row.item().and_then(|item| item.downcast::<RowModel>().ok())
                        else {
                            continue;
                        };

                        if row_model.content_type() != ContentType::SectionHeader {
                            continue;
                        }

                        row.set_expanded(false);
                        count += 1;

                        if count >= 2 {
                            break;
                        }
                    }
                }
            });

            let action_remove_filters = gio::SimpleAction::new("remove-filters", None);
            actions.add_action(&action_remove_filters);
            action_remove_filters.connect_activate({
                let this = self.obj().downgrade();
                move |_action, _| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    imp.toggle_running.set_active(false);
                    imp.toggle_failed.set_active(false);
                    imp.toggle_stopped.set_active(false);
                    imp.toggle_disabled.set_active(false);
                }
            });

            self.obj()
                .insert_action_group("services-page", Some(&actions));

            let service_actions = gio::SimpleActionGroup::new();
            service_actions.add_action(&actions::action_start(&self.table_view));
            service_actions.add_action(&actions::action_stop(&self.table_view));
            service_actions.add_action(&actions::action_restart(&self.table_view));
            service_actions.add_action(&actions::action_details(&self.table_view));
            self.obj()
                .insert_action_group("service", Some(&service_actions));

            let process_actions = gio::SimpleActionGroup::new();
            process_actions.add_action(&actions::apps::action_stop(&self.table_view));
            process_actions.add_action(&actions::apps::action_force_stop(&self.table_view));
            process_actions.add_action(&actions::apps::action_suspend(&self.table_view));
            process_actions.add_action(&actions::apps::action_continue(&self.table_view));
            process_actions.add_action(&actions::apps::action_hangup(&self.table_view));
            process_actions.add_action(&actions::apps::action_interrupt(&self.table_view));
            process_actions.add_action(&actions::apps::action_user_one(&self.table_view));
            process_actions.add_action(&actions::apps::action_user_two(&self.table_view));
            process_actions.add_action(&actions::apps::action_details(&self.table_view));
            self.obj()
                .insert_action_group("process", Some(&process_actions));

            let toggle_group = [
                self.toggle_running.downgrade(),
                self.toggle_failed.downgrade(),
                self.toggle_stopped.downgrade(),
                self.toggle_disabled.downgrade(),
            ];

            // Set up the models here since we need access to the main application window
            // which is not yet available in the constructor.
            self.table_view.imp().setup(
                SettingsNamespace::ServicesPage,
                &self.user_section,
                &self.system_section,
                Some(&self.process_action_bar),
                Some(&self.service_action_bar),
                Some(toggle_group),
            );
        }
    }

    impl WidgetImpl for ServicesPage {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl BoxImpl for ServicesPage {}
}

glib::wrapper! {
    pub struct ServicesPage(ObjectSubclass<imp::ServicesPage>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl ServicesPage {
    fn update_common(&self, readings: &mut crate::magpie_client::Readings) {
        let imp = self.imp();

        update_services(
            &readings.running_processes,
            &readings.user_services,
            &imp.user_section.children(),
            &HashMap::new(),
            &Default::default(),
            imp.table_view.imp().use_merged_stats.get(),
            SectionType::FirstSection,
        );

        update_services(
            &readings.running_processes,
            &readings.system_services,
            &imp.system_section.children(),
            &HashMap::new(),
            &Default::default(),
            imp.table_view.imp().use_merged_stats.get(),
            SectionType::SecondSection,
        );

        let mut services = readings.user_services.values().collect::<Vec<_>>();
        services.extend(readings.system_services.values());

        let total_services = services.len();
        let mut disabled_services = 0;
        let mut running_services = 0;
        let mut stopped_services = 0;
        let mut failed_services = 0;
        for service in services {
            if service.running {
                running_services += 1;
            } else if service.failed {
                failed_services += 1;
            } else if service.enabled {
                stopped_services += 1;
            } else {
                disabled_services += 1;
            }
        }

        imp.total_services.set(total_services as u32);
        imp.running_services.set(running_services);
        imp.stopped_services.set(stopped_services);
        imp.failed_services.set(failed_services);
        imp.disabled_services.set(disabled_services);

        imp.update_headers();
    }

    pub fn update_readings(&self, readings: &mut crate::magpie_client::Readings) -> bool {
        let imp = self.imp();

        self.update_common(readings);

        if !imp.has_loaded.get()
            && (!readings.system_services.is_empty() || !readings.user_services.is_empty())
        {
            glib::idle_add_local_once({
                let this = self.downgrade();
                move || {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    imp.content_stack.set_visible_child_name("content");
                }
            });
            imp.has_loaded.set(true);
        }

        if let Some(row_sorter) = imp.table_view.imp().row_sorter.get() {
            row_sorter.changed(gtk::SorterChange::Different)
        }

        if readings.network_stats_error.is_some() {
            imp.table_view
                .get()
                .imp()
                .network_usage_column
                .set_visible(false);
        }

        if readings.mem_info.swap_total == 0 {
            imp.table_view.get().imp().swap_column.set_visible(false);
        }

        true
    }

    #[inline]
    pub fn collapse(&self) {
        self.imp().collapse();
    }

    #[inline]
    pub fn expand(&self) {
        self.imp().expand();
    }

    pub fn activate_table_view_action(&self, name: &str) -> Result<(), glib::error::BoolError> {
        WidgetExt::activate_action(&*self.imp().table_view, name, None)
    }
}
