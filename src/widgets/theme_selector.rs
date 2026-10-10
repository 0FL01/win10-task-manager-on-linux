/* widgets/theme_selector.rs
 *
 * Copyright 2024 Mission Center Developers
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

use std::cell::Cell;

use glib::{ParamSpec, Properties, Value};
use gtk::{gdk::prelude::*, glib, prelude::WidgetExt, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::ThemeSelector)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/widgets/theme_selector.ui")]
    pub struct ThemeSelector {
        #[property(get = Self::action_name, set = Self::set_action_name, type = glib::GString)]
        action_name: Cell<glib::GString>,

        #[template_child]
        pub follow: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub light: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub dark: TemplateChild<gtk::CheckButton>,
    }

    impl Default for ThemeSelector {
        fn default() -> Self {
            Self {
                action_name: Cell::new("".into()),

                follow: TemplateChild::default(),
                light: TemplateChild::default(),
                dark: TemplateChild::default(),
            }
        }
    }

    impl ThemeSelector {
        fn action_name(&self) -> glib::GString {
            let action_name = self.action_name.take();
            self.action_name.set(action_name.clone());

            action_name
        }

        fn set_action_name(&self, action_name: &str) {
            let current_action_name = self.action_name.take();
            if current_action_name == action_name {
                self.action_name.set(current_action_name);
                return;
            }

            self.follow.set_action_name(Some(action_name));
            self.light.set_action_name(Some(action_name));
            self.dark.set_action_name(Some(action_name));

            self.action_name.set(action_name.into());
            self.obj().notify_action_name();
        }
    }

    impl ThemeSelector {
        fn on_system_supports_color_schemes_notify(&self, style_manager: &adw::StyleManager) {
            self.obj()
                .set_visible(style_manager.system_supports_color_schemes());
        }

        fn on_dark_notify(&self, style_manager: &adw::StyleManager) {
            let dark = style_manager.is_dark();
            if dark {
                self.obj().add_css_class("dark");
            } else {
                self.obj().remove_css_class("dark");
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ThemeSelector {
        const NAME: &'static str = "ThemeSelector";
        type Type = super::ThemeSelector;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.set_css_name("themeselector");
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ThemeSelector {
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

            let this = self.obj();
            let style_manager = adw::StyleManager::default();

            style_manager.connect_system_supports_color_schemes_notify({
                let this = this.downgrade();
                move |style_manager| {
                    if let Some(this) = this.upgrade() {
                        this.imp()
                            .on_system_supports_color_schemes_notify(style_manager);
                    }
                }
            });

            style_manager.connect_dark_notify({
                let this = this.downgrade();
                move |style_manager| {
                    if let Some(this) = this.upgrade() {
                        this.imp().on_dark_notify(style_manager);
                    }
                }
            });

            self.on_system_supports_color_schemes_notify(&style_manager);
            self.on_dark_notify(&style_manager);

            let dark = style_manager.is_dark();
            self.set_action_name(if dark { "dark" } else { "light" });
        }
    }

    impl WidgetImpl for ThemeSelector {}

    impl BoxImpl for ThemeSelector {}
}

glib::wrapper! {
    pub struct ThemeSelector(ObjectSubclass<imp::ThemeSelector>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl ThemeSelector {
    pub fn new(action_name: &str) -> Self {
        glib::Object::builder()
            .property("action-name", action_name)
            .build()
    }
}
