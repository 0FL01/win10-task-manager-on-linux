/* window.rs
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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use adw::{prelude::*, subclass::prelude::*};
use glib::{
    g_critical, idle_add_local_once, BindingFlags, ParamSpec, Propagation, Properties, Value,
};
use gtk::glib::{g_debug, ControlFlow};
use gtk::{gdk, gio, glib};

use crate::application::INTERVAL_STEP;
use crate::performance_page::widgets::AnimationFrame;
use crate::widgets::ListCell;
use crate::widgets::Placeholder;
use crate::widgets::ThemeSelector;
use crate::{app, magpie_client::Readings, settings};

fn special_shortcuts(
) -> &'static HashMap<gdk::ModifierType, HashMap<gdk::Key, fn(&MissionCenterWindow) -> bool>> {
    fn select_device(window: &MissionCenterWindow, index: i32) -> bool {
        let imp = window.imp();

        let result = window.performance_page_active();
        if result {
            imp.performance_page.select_nth_shown_device(index);
        }
        result
    }

    fn select_device_1(window: &MissionCenterWindow) -> bool {
        select_device(window, 0)
    }

    fn select_device_2(window: &MissionCenterWindow) -> bool {
        select_device(window, 1)
    }

    fn select_device_3(window: &MissionCenterWindow) -> bool {
        select_device(window, 2)
    }

    fn select_device_4(window: &MissionCenterWindow) -> bool {
        select_device(window, 3)
    }

    fn select_device_5(window: &MissionCenterWindow) -> bool {
        select_device(window, 4)
    }

    fn select_device_6(window: &MissionCenterWindow) -> bool {
        select_device(window, 5)
    }

    fn select_device_7(window: &MissionCenterWindow) -> bool {
        select_device(window, 6)
    }

    fn select_device_8(window: &MissionCenterWindow) -> bool {
        select_device(window, 7)
    }

    fn toggle_search(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();
        let result = imp.search_button.is_visible() && !imp.search_button.is_active();
        if result {
            let _ = WidgetExt::activate_action(window, "win.toggle-search", None);
        }

        result
    }

    fn graph_copy(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();

        let result = window.performance_page_active();
        if result {
            let Some(visible_child) = imp.performance_page.imp().page_stack.visible_child() else {
                return false;
            };

            let _ = WidgetExt::activate_action(&visible_child, "graph.copy", None);
        }
        result
    }

    fn graph_summary(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();

        let result = window.performance_page_active();
        if result {
            let _ = WidgetExt::activate_action(&*imp.performance_page, "graph.summary", None);
        }
        result
    }

    fn ctrl_l(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();
        if window.apps_page_active() {
            let _ = WidgetExt::activate_action(&*imp.apps_page, "apps-page.collapse-all", None);
            return true;
        } else if window.services_page_active() {
            let _ =
                WidgetExt::activate_action(&*imp.services_page, "services-page.collapse-all", None);
            return true;
        }

        false
    }

    fn services_start(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();

        let result = window.services_page_active();
        if result {
            let _ = imp
                .services_page
                .activate_table_view_action("service.start");
        }
        result
    }

    fn ctrl_e(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();

        if window.apps_page_active() {
            let _ = imp.apps_page.activate_table_view_action("process.stop");
            return true;
        } else if window.services_page_active() {
            let _ = imp.services_page.activate_table_view_action("process.stop");
            let _ = imp.services_page.activate_table_view_action("service.stop");
            return true;
        }

        false
    }

    fn force_stop(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();

        if window.apps_page_active() {
            let _ = imp
                .apps_page
                .activate_table_view_action("process.force-stop");
            return true;
        } else if window.services_page_active() {
            let _ = imp
                .services_page
                .activate_table_view_action("process.force-stop");
            let _ = imp.services_page.activate_table_view_action("service.stop");
            return true;
        }

        false
    }

    fn ctrl_i(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();

        if window.apps_page_active() {
            let _ = imp.apps_page.activate_table_view_action("process.details");
            return true;
        } else if window.services_page_active() {
            let _ = imp
                .services_page
                .activate_table_view_action("process.details");
            let _ = imp
                .services_page
                .activate_table_view_action("service.details");
            return true;
        }

        false
    }

    fn services_restart(window: &MissionCenterWindow) -> bool {
        let imp = window.imp();

        let result = window.services_page_active();
        if result {
            let _ = imp
                .services_page
                .activate_table_view_action("service.restart");
        }
        result
    }

    fn about_system(_window: &MissionCenterWindow) -> bool {
        app!().activate_action("system-about", None);

        false
    }

    static SHORTCUTS: OnceLock<
        HashMap<gdk::ModifierType, HashMap<gdk::Key, fn(&MissionCenterWindow) -> bool>>,
    > = OnceLock::new();
    SHORTCUTS.get_or_init(|| {
        let mut shortcuts = HashMap::new();

        let mut no_modifier_shortcuts =
            HashMap::<gdk::Key, fn(&MissionCenterWindow) -> bool>::new();
        no_modifier_shortcuts.insert(gdk::Key::F1, select_device_1);
        no_modifier_shortcuts.insert(gdk::Key::F2, select_device_2);
        no_modifier_shortcuts.insert(gdk::Key::F3, select_device_3);
        no_modifier_shortcuts.insert(gdk::Key::F4, select_device_4);
        no_modifier_shortcuts.insert(gdk::Key::F5, select_device_5);
        no_modifier_shortcuts.insert(gdk::Key::F6, select_device_6);
        no_modifier_shortcuts.insert(gdk::Key::F7, select_device_7);
        no_modifier_shortcuts.insert(gdk::Key::F8, select_device_8);
        shortcuts.insert(gdk::ModifierType::NO_MODIFIER_MASK, no_modifier_shortcuts);

        let mut ctrl_shortcuts = HashMap::<gdk::Key, fn(&MissionCenterWindow) -> bool>::new();
        ctrl_shortcuts.insert(gdk::Key::F, toggle_search);
        ctrl_shortcuts.insert(gdk::Key::f, toggle_search);
        ctrl_shortcuts.insert(gdk::Key::A, about_system);
        ctrl_shortcuts.insert(gdk::Key::a, about_system);
        ctrl_shortcuts.insert(gdk::Key::M, graph_summary);
        ctrl_shortcuts.insert(gdk::Key::m, graph_summary);
        ctrl_shortcuts.insert(gdk::Key::C, graph_copy);
        ctrl_shortcuts.insert(gdk::Key::c, graph_copy);
        ctrl_shortcuts.insert(gdk::Key::L, ctrl_l);
        ctrl_shortcuts.insert(gdk::Key::l, ctrl_l);
        ctrl_shortcuts.insert(gdk::Key::E, ctrl_e);
        ctrl_shortcuts.insert(gdk::Key::e, ctrl_e);
        ctrl_shortcuts.insert(gdk::Key::X, force_stop);
        ctrl_shortcuts.insert(gdk::Key::x, force_stop);
        ctrl_shortcuts.insert(gdk::Key::I, ctrl_i);
        ctrl_shortcuts.insert(gdk::Key::i, ctrl_i);
        ctrl_shortcuts.insert(gdk::Key::S, services_start);
        ctrl_shortcuts.insert(gdk::Key::s, services_start);
        ctrl_shortcuts.insert(gdk::Key::R, services_restart);
        ctrl_shortcuts.insert(gdk::Key::r, services_restart);
        shortcuts.insert(gdk::ModifierType::CONTROL_MASK, ctrl_shortcuts);

        shortcuts
    })
}

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::MissionCenterWindow)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/window.ui")]
    pub struct MissionCenterWindow {
        #[template_child]
        pub breakpoint: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub window_content: TemplateChild<adw::ToolbarView>,
        #[template_child]
        pub bottom_bar: TemplateChild<adw::ViewSwitcherBar>,
        #[template_child]
        pub sidebar_edit_mode_enable_all: TemplateChild<gtk::Button>,
        #[template_child]
        pub sidebar_edit_mode_disable_all: TemplateChild<gtk::Button>,
        #[template_child]
        pub sidebar_edit_mode_reset: TemplateChild<gtk::Button>,
        #[template_child]
        pub toggle_sidebar_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub sidebar: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub sidebar_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub performance_page: TemplateChild<crate::performance_page::PerformancePage>,
        #[template_child]
        pub apps_page: TemplateChild<crate::apps_page::AppsPage>,
        #[template_child]
        pub services_stack_page: TemplateChild<adw::ViewStackPage>,
        #[template_child]
        pub services_page: TemplateChild<crate::services_page::ServicesPage>,
        #[template_child]
        pub header_bar: TemplateChild<adw::HeaderBar>,
        #[template_child]
        pub header_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub header_tabs: TemplateChild<adw::ViewSwitcher>,
        #[template_child]
        pub header_search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub search_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub stack: TemplateChild<adw::ViewStack>,

        #[property(get)]
        performance_page_active: Cell<bool>,
        #[property(get)]
        apps_page_active: Cell<bool>,
        #[property(get)]
        services_page_active: Cell<bool>,
        #[property(get)]
        user_hid_sidebar: Cell<bool>,

        #[property(name = "info-button-visible", get = Self::info_button_visible, type = bool)]
        _info_button_visible: [u8; 0],
        #[property(name = "search-button-visible", get = Self::search_button_visible, type = bool)]
        _search_button_visible: [u8; 0],

        #[property(get, set)]
        summary_mode: Cell<bool>,
        #[property(get, set)]
        collapse_threshold: Cell<i32>,

        pub last_refresh: Cell<u128>,
        pub cached_refresh_ticks: Cell<u64>,
        pub global_scroll_ticks: Cell<u32>,

        pub stored_readings: RefCell<Option<Readings>>,
        pub is_refreshing_paused: Cell<bool>,
        pub(super) services_refresh_counter: Cell<u8>,
    }

    impl Default for MissionCenterWindow {
        fn default() -> Self {
            Self {
                breakpoint: TemplateChild::default(),
                split_view: TemplateChild::default(),
                window_content: TemplateChild::default(),
                menu_button: TemplateChild::default(),
                bottom_bar: TemplateChild::default(),
                sidebar_edit_mode_enable_all: TemplateChild::default(),
                sidebar_edit_mode_disable_all: TemplateChild::default(),
                sidebar_edit_mode_reset: TemplateChild::default(),
                toggle_sidebar_button: TemplateChild::default(),
                sidebar: TemplateChild::default(),
                sidebar_stack: TemplateChild::default(),
                performance_page: TemplateChild::default(),
                apps_page: TemplateChild::default(),
                services_stack_page: TemplateChild::default(),
                services_page: TemplateChild::default(),
                header_bar: TemplateChild::default(),
                header_stack: TemplateChild::default(),
                header_tabs: TemplateChild::default(),
                header_search_entry: TemplateChild::default(),
                search_button: TemplateChild::default(),
                stack: TemplateChild::default(),

                performance_page_active: Cell::new(true),
                apps_page_active: Cell::new(false),
                services_page_active: Cell::new(false),
                user_hid_sidebar: Cell::new(false),

                _info_button_visible: [0; 0],
                _search_button_visible: [0; 0],

                summary_mode: Cell::new(false),
                collapse_threshold: Cell::new(0),
                last_refresh: Cell::new(0),
                cached_refresh_ticks: Cell::new(1),
                global_scroll_ticks: Cell::new(1),

                stored_readings: RefCell::new(None),
                is_refreshing_paused: Cell::new(false),
                services_refresh_counter: Cell::new(0),
            }
        }
    }

    impl MissionCenterWindow {
        fn info_button_visible(&self) -> bool {
            if self.performance_page.is_bound() {
                self.performance_page_active.get() && self.performance_page.info_button_visible()
            } else {
                false
            }
        }

        fn search_button_visible(&self) -> bool {
            self.apps_page_active.get() || self.services_page_active.get()
        }
    }

    impl MissionCenterWindow {
        fn update_active_page(&self) {
            use glib::g_critical;

            let visible_child_name = self.stack.visible_child_name().unwrap_or("".into());

            if visible_child_name == "performance-page" {
                if self.performance_page_active.get() {
                    return;
                }

                self.performance_page_active.set(true);
                self.obj().notify_performance_page_active();

                self.apps_page_active.set(false);
                self.obj().notify_apps_page_active();

                self.services_page_active.set(false);
                self.obj().notify_services_page_active();
            }
            if visible_child_name == "apps-page" {
                if self.apps_page_active.get() {
                    return;
                }

                self.performance_page_active.set(false);
                self.obj().notify_performance_page_active();

                self.apps_page_active.set(true);
                self.obj().notify_apps_page_active();

                self.services_page_active.set(false);
                self.obj().notify_services_page_active();
            } else if visible_child_name == "services-page" {
                if self.services_page_active.get() {
                    return;
                }

                self.performance_page_active.set(false);
                self.obj().notify_performance_page_active();

                self.apps_page_active.set(false);
                self.obj().notify_apps_page_active();

                self.services_page_active.set(true);
                self.obj().notify_services_page_active();
            }

            self.obj().notify_info_button_visible();
            self.obj().notify_search_button_visible();

            settings!()
                .set_string("window-selected-page", &visible_child_name)
                .unwrap_or_else(|_| {
                    g_critical!(
                        "MissionCenter",
                        "Failed to set window-selected-page setting"
                    );
                });
        }

        pub(super) fn update_services(&self, readings: &mut Readings) -> bool {
            const SERVICES_REFRESH_COUNTER_MAX_FOR_EMPTY_LIST: u8 = 5;

            let mut result = true;

            self.services_refresh_counter
                .update(|c| c.saturating_add(1));
            let services_refresh_counter = self.services_refresh_counter.get();

            let should_show_services_page = !readings.system_services.is_empty()
                || !readings.user_services.is_empty()
                || services_refresh_counter < SERVICES_REFRESH_COUNTER_MAX_FOR_EMPTY_LIST;

            if should_show_services_page {
                self.services_stack_page.set_visible(true);
                result &= self.services_page.update_readings(readings);
            } else {
                self.services_stack_page.set_visible(false);
            }

            result
        }
    }

    impl MissionCenterWindow {
        fn configure_actions(&self) {
            let app = app!();

            let toggle_search =
                gio::SimpleAction::new_stateful("toggle-search", None, &false.to_variant());
            toggle_search.connect_activate({
                let this = self.obj().downgrade();
                move |action, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    let new_state = !action.state().and_then(|v| v.get::<bool>()).unwrap_or(true);
                    action.set_state(&new_state.to_variant());
                    this.search_button.set_active(new_state);

                    if new_state {
                        this.header_stack.set_visible_child_name("search-entry");
                        this.header_search_entry.grab_focus();
                        this.header_search_entry.select_region(-1, -1);

                        this.header_stack.set_visible(true);
                    } else {
                        if this.window_width_below_threshold() {
                            this.header_stack.set_visible(false);
                        }

                        this.header_search_entry.set_text("");
                        this.header_stack.set_visible_child_name("view-switcher");
                    }
                }
            });
            self.obj().add_action(&toggle_search);

            let ty = unsafe { glib::VariantTy::from_str_unchecked("s") };
            let interface_style =
                gio::SimpleAction::new_stateful("interface-style", Some(ty), &"default".into());
            interface_style.connect_activate(|action, param| {
                let interface_style = param.and_then(|v| v.get::<String>()).unwrap_or_default();

                let style_manager = adw::StyleManager::default();

                let _ = settings!().set_enum(
                    "window-interface-style",
                    match interface_style.as_str() {
                        "default" => {
                            style_manager.set_color_scheme(adw::ColorScheme::Default);
                            adw::ffi::ADW_COLOR_SCHEME_DEFAULT
                        }
                        "force-light" => {
                            style_manager.set_color_scheme(adw::ColorScheme::ForceLight);
                            adw::ffi::ADW_COLOR_SCHEME_FORCE_LIGHT
                        }
                        "force-dark" => {
                            style_manager.set_color_scheme(adw::ColorScheme::ForceDark);
                            adw::ffi::ADW_COLOR_SCHEME_FORCE_DARK
                        }
                        _ => {
                            g_critical!(
                                "MissionCenter",
                                "Invalid value for window-interface-style setting: {}",
                                interface_style
                            );
                            return;
                        }
                    },
                );
                action.set_state(&interface_style.to_variant());
            });
            self.obj().add_action(&interface_style);

            let action = gio::SimpleAction::new("select-tab-performance", None);
            action.connect_activate({
                let this = self.obj().downgrade();
                move |_, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();
                    this.stack.set_visible_child_name("performance-page");
                }
            });
            self.obj().add_action(&action);
            app.set_accels_for_action("win.select-tab-performance", &["<Control>1", "<Alt>1"]);

            let action = gio::SimpleAction::new("select-tab-apps", None);
            action.connect_activate({
                let this = self.obj().downgrade();
                move |_, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let imp = this.imp();
                    if imp.summary_mode.get() {
                        return;
                    }
                    imp.stack.set_visible_child_name("apps-page");
                }
            });
            self.obj().add_action(&action);
            app.set_accels_for_action("win.select-tab-apps", &["<Control>2", "<Alt>2"]);

            let action = gio::SimpleAction::new("select-tab-services", None);
            action.connect_activate({
                let this = self.obj().downgrade();
                move |_, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let imp = this.imp();
                    if imp.summary_mode.get() {
                        return;
                    }
                    imp.stack.set_visible_child_name("services-page");
                }
            });
            self.obj().add_action(&action);
            app.set_accels_for_action("win.select-tab-services", &["<Control>3", "<Alt>3"]);

            let action =
                gio::SimpleAction::new_stateful("toggle-sidebar", None, &true.to_variant());
            action.connect_activate({
                let this = self.obj().downgrade();
                move |action, _| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let imp = this.imp();

                    if imp.summary_mode.get() {
                        return;
                    }

                    let old_state = imp.split_view.shows_sidebar();

                    let new_state = !old_state;
                    action.set_state(&new_state.to_variant());
                    imp.toggle_sidebar_button.set_active(new_state);

                    if old_state != imp.user_hid_sidebar.get() {
                        if imp.performance_page_active.get() {
                            if !imp.window_width_below_threshold() {
                                imp.split_view.set_collapsed(false);
                            }
                        } else if new_state == false {
                            // If the use dismisses the siderbar using the keyboard shortcut, while
                            // not in the performance page, don't treat it as an intentional action.
                            return;
                        }

                        imp.user_hid_sidebar.set(old_state);
                        imp.obj().notify_user_hid_sidebar();
                    }
                }
            });
            self.obj().add_action(&action);
            app.set_accels_for_action("win.toggle-sidebar", &["<Control>T", "F9"]);

            let action = gio::SimpleAction::new("close", None);
            action.connect_activate({
                let this = self.obj().downgrade();
                move |_, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    this.close();
                }
            });
            self.obj().add_action(&action);
            app.set_accels_for_action("win.close", &["<Control>W"]);
        }

        fn configure_theme_selection(&self) {
            fn update_interface_style(this: &super::MissionCenterWindow, settings: &gio::Settings) {
                let Some(action) = this
                    .lookup_action("interface-style")
                    .and_then(|a| a.downcast::<gio::SimpleAction>().ok())
                else {
                    g_critical!(
                        "MissionCenter",
                        "Failed to get window-interface-style setting"
                    );
                    return;
                };

                let style_manager = adw::StyleManager::default();

                match settings.enum_("window-interface-style") {
                    adw::ffi::ADW_COLOR_SCHEME_DEFAULT => {
                        style_manager.set_color_scheme(adw::ColorScheme::Default);
                        action.set_state(&"default".to_variant());
                    }
                    adw::ffi::ADW_COLOR_SCHEME_FORCE_LIGHT => {
                        style_manager.set_color_scheme(adw::ColorScheme::ForceLight);
                        action.set_state(&"force-light".to_variant());
                    }
                    adw::ffi::ADW_COLOR_SCHEME_FORCE_DARK => {
                        style_manager.set_color_scheme(adw::ColorScheme::ForceDark);
                        action.set_state(&"force-dark".to_variant());
                    }
                    _ => {
                        unreachable!("Invalid value for window-interface-style setting");
                    }
                }
            }

            let settings = settings!();

            settings.connect_changed(Some("window-interface-style"), {
                let this = self.obj().downgrade();
                move |settings, _| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    update_interface_style(&this, settings);
                }
            });
            update_interface_style(&self.obj(), &settings);

            if let Some(popover) = self
                .menu_button
                .popover()
                .and_then(|p| p.downcast::<gtk::PopoverMenu>().ok())
            {
                popover.add_child(&ThemeSelector::new("win.interface-style"), "theme-selector");
            }
        }

        #[inline]
        fn window_width_below_threshold(&self) -> bool {
            let window_width =
                adw::LengthUnit::from_px(adw::LengthUnit::Sp, self.obj().width() as _, None);
            let collapse_threshold = self.collapse_threshold.get() as f64;

            window_width < collapse_threshold
        }

        #[inline]
        pub(crate) fn should_hide_sidebar(&self) -> bool {
            let performance_page_active = self.performance_page_active.get();
            let summary_mode = self.summary_mode.get();
            let user_hid_sidebar = self.user_hid_sidebar.get();

            !performance_page_active
                || user_hid_sidebar
                || summary_mode
                || self.window_width_below_threshold()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MissionCenterWindow {
        const NAME: &'static str = "MissionCenterWindow";
        type Type = super::MissionCenterWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            use crate::{
                apps_page::AppsPage, performance_page::PerformancePage, services_page::ServicesPage,
            };

            ListCell::ensure_type();
            Placeholder::ensure_type();

            PerformancePage::ensure_type();
            AppsPage::ensure_type();
            ServicesPage::ensure_type();

            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for MissionCenterWindow {
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

            self.configure_actions();
            self.configure_theme_selection();

            idle_add_local_once({
                let this = self.obj().downgrade();
                move || {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        this.update_active_page();
                    }
                }
            });

            self.sidebar.connect_row_activated({
                let this = self.obj().downgrade();
                move |_, _| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        let current_child = this.stack.visible_child_name().unwrap_or_default();
                        if current_child.as_str() != "performance-page" {
                            this.stack.set_visible_child_name("performance-page");
                        }
                    }
                }
            });

            self.sidebar_edit_mode_enable_all.connect_clicked({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        this.performance_page.sidebar_enable_all();
                    }
                }
            });

            self.sidebar_edit_mode_disable_all.connect_clicked({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        this.performance_page.sidebar_disable_all();
                    }
                }
            });

            self.sidebar_edit_mode_reset.connect_clicked({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        this.performance_page.sidebar_reset_to_default();
                    }
                }
            });

            self.stack.connect_visible_child_notify({
                let this = self.obj().downgrade();
                move |_| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.search_button.is_active() {
                        let _ = WidgetExt::activate_action(
                            this.obj().as_ref(),
                            "win.toggle-search",
                            None,
                        );
                    }
                    this.update_active_page();
                }
            });

            let evt_ctrl_key = gtk::EventControllerKey::new();
            evt_ctrl_key.connect_key_pressed({
                let this = self.obj().downgrade();
                move |controller, key, _, modifier| {
                    let Some(this) = this.upgrade() else {
                        return Propagation::Stop;
                    };
                    let imp = this.imp();

                    if key == gdk::Key::Control_L || key == gdk::Key::Control_R {
                        this.pause_refreshing();
                    }

                    let special_shortcuts = special_shortcuts();
                    if let Some(shortcut) = special_shortcuts.get(&modifier) {
                        if let Some(action) = shortcut.get(&key) {
                            if action(&this) {
                                return Propagation::Stop;
                            }
                        }
                    }

                    controller.forward(&imp.header_search_entry.get());
                    Propagation::Proceed
                }
            });
            evt_ctrl_key.connect_key_released({
                let this = self.obj().downgrade();
                move |_, key, _, _modifier| {
                    let Some(this) = this.upgrade() else { return };

                    if key == gdk::Key::Control_L || key == gdk::Key::Control_R {
                        this.continue_refreshing();
                    }
                }
            });
            self.obj().add_controller(evt_ctrl_key);

            // The key-released event is lost if focus leaves the window while
            // CTRL is held; resume updates on deactivation to avoid a stuck pause.
            self.obj().connect_is_active_notify(|window| {
                if !window.is_active() {
                    window.continue_refreshing();
                }
                window.sync_reduced_activity();
            });

            self.header_search_entry
                .set_key_capture_widget(Some(&self.header_search_entry.get()));

            self.header_search_entry.connect_search_started({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();

                        if this.apps_page_active.get() || this.services_page_active.get() {
                            let _ = WidgetExt::activate_action(
                                this.obj().as_ref(),
                                "win.toggle-search",
                                None,
                            );
                        }
                    }
                }
            });

            self.header_search_entry.connect_stop_search({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        let this = this.imp();
                        if this.apps_page_active.get() || this.services_page_active.get() {
                            let _ = WidgetExt::activate_action(
                                this.obj().as_ref(),
                                "win.toggle-search",
                                None,
                            );
                        }
                    }
                }
            });

            self.breakpoint.set_condition(Some(
                &adw::BreakpointCondition::parse(&format!(
                    "max-width: {}sp",
                    self.collapse_threshold.get()
                ))
                .unwrap(),
            ));
            self.breakpoint.connect_apply({
                let this = self.obj().downgrade();
                move |_| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    this.bottom_bar.set_reveal(true);
                    if !this.search_button.is_active() {
                        this.header_stack.set_visible(false);
                    }

                    this.apps_page.collapse();
                    this.services_page.collapse();

                    if this.performance_page_active.get() {
                        this.split_view.set_collapsed(this.should_hide_sidebar());
                    }
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

                    this.header_stack.set_visible(true);
                    this.bottom_bar.set_reveal(false);

                    this.apps_page.expand();
                    this.services_page.expand();

                    this.split_view.set_collapsed(this.should_hide_sidebar());
                }
            });

            self.obj().connect_performance_page_active_notify({
                let this = self.obj().downgrade();
                move |_| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.performance_page_active.get() {
                        let should_hide_sidebar = this.should_hide_sidebar();
                        this.split_view.set_collapsed(should_hide_sidebar);
                        this.split_view.set_show_sidebar(!should_hide_sidebar);
                    } else {
                        this.split_view.set_collapsed(true);
                        this.split_view.set_show_sidebar(false);
                    }
                }
            });

            self.obj().connect_summary_mode_notify({
                let this = self.obj().downgrade();
                move |_| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };
                    let this = this.imp();

                    if this.summary_mode.get() {
                        this.split_view.set_show_sidebar(false);
                    } else if !this.window_width_below_threshold() {
                        this.split_view.set_collapsed(false);
                        this.split_view
                            .set_show_sidebar(!this.user_hid_sidebar.get());
                    }
                }
            });

            self.performance_page.connect_info_button_visible_notify({
                let this = self.obj().downgrade();
                move |_| {
                    if let Some(this) = this.upgrade() {
                        this.notify_info_button_visible();
                    }
                }
            });
        }
    }

    impl WidgetImpl for MissionCenterWindow {
        fn realize(&self) {
            self.parent_realize();

            self.stack
                .set_visible_child_name(settings!().string("window-selected-page").as_str());
        }
    }

    impl WindowImpl for MissionCenterWindow {}

    impl ApplicationWindowImpl for MissionCenterWindow {}

    impl AdwApplicationWindowImpl for MissionCenterWindow {}
}

glib::wrapper! {
    pub struct MissionCenterWindow(ObjectSubclass<imp::MissionCenterWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible,
                    gtk::Buildable, gtk::ShortcutManager, gtk::Root, gtk::Native;
}

impl MissionCenterWindow {
    pub fn new<P: IsA<gtk::Application>>(
        application: &P,
        settings: &gio::Settings,
        sys_info: &crate::magpie_client::MagpieClient,
    ) -> Self {
        use gtk::glib::*;

        let this: Self = Object::builder()
            .property("application", application)
            .build();

        let interval = settings.uint64("app-update-interval-u64");
        sys_info.set_update_speed(interval);
        sys_info.set_core_count_affects_percentages(
            settings.boolean("apps-page-core-count-affects-percentages"),
        );

        this.imp().cached_refresh_ticks.set(interval);

        settings.connect_changed(Some("app-update-interval-u64"), |settings, _| {
            let update_speed = settings.uint64("app-update-interval-u64");
            let app = app!();
            match app.sys_info() {
                Ok(sys_info) => {
                    sys_info.set_update_speed(update_speed);
                }
                Err(e) => {
                    g_critical!(
                        "MissionCenter",
                        "Failed to get sys_info from MissionCenterApplication: {}",
                        e
                    );
                }
            };

            match app.window() {
                None => {
                    g_critical!(
                        "MissionCenter",
                        "Failed to get window from MissionCenterApplication",
                    );
                }
                Some(window) => {
                    window.imp().cached_refresh_ticks.set(update_speed);
                }
            }
        });

        sys_info.set_reduced_activity(this.reduced_activity());

        this.connect_suspended_notify(|window| {
            window.sync_reduced_activity();
        });

        this
    }

    #[inline]
    fn get_current_timestamp() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};

        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time before Unix epoch")
            .as_micros()
    }

    pub fn setup_animations(&self) {
        glib::timeout_add_local(Duration::from_millis(50), {
            let this = self.downgrade();

            move || {
                let Some(this) = this.upgrade() else {
                    return ControlFlow::Continue;
                };

                this.update_animations(AnimationFrame {
                    progress: ((Self::get_current_timestamp()
                        .saturating_sub(this.imp().last_refresh.get())
                        as f64
                        / 1_000_000.)
                        / (this.imp().cached_refresh_ticks.get() as f64 * INTERVAL_STEP))
                        .clamp(0., 1.) as f32,
                    grid_offset: this.imp().global_scroll_ticks.get(),
                });

                ControlFlow::Continue
            }
        });
    }

    pub fn set_initial_readings(&self, mut readings: Readings) {
        let imp = self.imp();

        self.add_css_class("mission-center-window");

        let ok = imp.performance_page.set_initial_readings(&readings);
        if !ok {
            g_critical!(
                "MissionCenter",
                "Failed to set initial readings for performance page"
            );
        }

        imp.sidebar_stack.set_visible_child_name("content");

        // Give services its own readings so the apps and services idles can both
        // be queued before continue_reading() below, keeping them ahead of the
        // first periodic refresh. Per-service process stats are skipped on this
        // first batch and filled in by the next refresh; mem_info is copied so the
        // swap column isn't left permanently hidden.
        let mut services_readings = Readings::new();
        services_readings.user_services = std::mem::take(&mut readings.user_services);
        services_readings.system_services = std::mem::take(&mut readings.system_services);
        services_readings.mem_info = readings.mem_info.clone();

        idle_add_local_once({
            let this = self.downgrade();
            move || {
                if let Some(this) = this.upgrade() {
                    let imp = this.imp();
                    imp.apps_page.update_readings(&mut readings);
                }
            }
        });

        idle_add_local_once({
            let this = self.downgrade();
            move || {
                if let Some(this) = this.upgrade() {
                    let imp = this.imp();
                    imp.update_services(&mut services_readings);
                }
            }
        });

        imp.performance_page
            .add_css_class("mission-center-performance-page");

        imp.apps_page.add_css_class("mission-center-apps-page");

        imp.services_page
            .add_css_class("mission-center-services-page");

        imp.header_bar.set_visible(true);
        imp.stack.set_visible(true);

        imp.bottom_bar.set_visible(true);
        self.bind_property("summary-mode", &imp.bottom_bar.get(), "visible")
            .flags(BindingFlags::INVERT_BOOLEAN)
            .build();

        imp.split_view.set_collapsed(imp.should_hide_sidebar());

        if let Ok(sys_info) = app!().sys_info() {
            sys_info.continue_reading();
        } else {
            g_critical!(
                "MissionCenter",
                "Failed to get sys_info from MissionCenterApplication"
            );
        }

        if settings!().boolean("first-time-running") {
            app!().show_first_run_dialog();
            settings!()
                .set_boolean("first-time-running", false)
                .unwrap_or_else(|_| {
                    g_critical!(
                        "MissionCenter::Application",
                        "Failed to save first-time-running setting"
                    )
                })
        }
    }

    pub fn update_readings(&self, readings: &mut Readings) -> bool {
        let mut result = true;

        let this = self.imp();

        if self.refreshing_paused() {
            this.stored_readings
                .replace(Some(std::mem::replace(readings, Readings::new())));
            return true;
        }

        result &= this.performance_page.update_readings(readings);

        if readings.includes_app_data {
            result &= this.apps_page.update_readings(readings);
            result &= this.update_services(readings);
        }

        this.last_refresh.set(Self::get_current_timestamp());

        let grid_offset = self.imp().global_scroll_ticks.get().wrapping_add(1);
        self.imp().global_scroll_ticks.set(grid_offset);

        self.update_animations(AnimationFrame {
            progress: 0.,
            grid_offset,
        });

        result
    }

    pub fn reduced_activity(&self) -> bool {
        self.is_suspended() && !self.is_active()
    }

    pub fn sync_reduced_activity(&self) {
        let reduced = self.reduced_activity();

        g_debug!("MissionCenter", "Reduced activity: {}", reduced);

        match app!().sys_info() {
            Ok(sys_info) => sys_info.set_reduced_activity(reduced),
            Err(e) => {
                g_critical!(
                    "MissionCenter",
                    "Failed to get sys_info from MissionCenterApplication: {}",
                    e
                );
            }
        }
    }

    pub fn pause_refreshing(&self) {
        self.imp().is_refreshing_paused.set(true);
    }

    pub fn continue_refreshing(&self) {
        let this = self.imp();

        if this.is_refreshing_paused.replace(false) {
            if let Some(mut readings) = this.stored_readings.take() {
                self.update_readings(&mut readings);
            }
        }
    }

    pub fn refreshing_paused(&self) -> bool {
        self.imp().is_refreshing_paused.get()
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        let mut result = true;

        let this = self.imp();

        g_debug!(
            "MissionCenter",
            "Updating with {} animation ticks",
            ticks.progress
        );

        result &= this.performance_page.update_animations(ticks);

        result
    }
}
