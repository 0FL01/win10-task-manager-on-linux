/* about_system_dialog.rs
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

use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, prelude::WidgetExt};

use magpie_types::about::{about::OsInfo, About};

use crate::table_view::cached_icon::CachedIcon;

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate, Default)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/about_system_dialog.ui")]
    pub struct AboutSystemDialog {
        // OS info
        #[template_child]
        os_info_box: TemplateChild<adw::PreferencesGroup>,

        #[template_child]
        os_name_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        os_name_label: TemplateChild<gtk::Label>,
        #[template_child]
        os_name: TemplateChild<gtk::Label>,

        #[template_child]
        version_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        version_label: TemplateChild<gtk::Label>,
        #[template_child]
        version: TemplateChild<gtk::Label>,

        #[template_child]
        package_manager_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        package_manager_label: TemplateChild<gtk::Label>,
        #[template_child]
        package_manager: TemplateChild<gtk::Label>,

        #[template_child]
        package_manager_version_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        package_manager_version_label: TemplateChild<gtk::Label>,
        #[template_child]
        package_manager_version: TemplateChild<gtk::Label>,

        // kernel info
        #[template_child]
        kernel_info_box: TemplateChild<adw::PreferencesGroup>,

        #[template_child]
        kernel_release_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        kernel_release_label: TemplateChild<gtk::Label>,
        #[template_child]
        kernel_release: TemplateChild<gtk::Label>,

        #[template_child]
        kernel_version_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        kernel_version_label: TemplateChild<gtk::Label>,
        #[template_child]
        kernel_version: TemplateChild<gtk::Label>,

        // DE info
        #[template_child]
        de_info_box: TemplateChild<adw::PreferencesGroup>,

        #[template_child]
        desktop_environment_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        desktop_environment_label: TemplateChild<gtk::Label>,
        #[template_child]
        desktop_environment: TemplateChild<gtk::Label>,

        #[template_child]
        desktop_environment_version_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        desktop_environment_version_label: TemplateChild<gtk::Label>,
        #[template_child]
        desktop_environment_version: TemplateChild<gtk::Label>,

        #[template_child]
        windowing_system_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        windowing_system_label: TemplateChild<gtk::Label>,
        #[template_child]
        windowing_system: TemplateChild<gtk::Label>,

        #[template_child]
        virtual_terminal_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        virtual_terminal_label: TemplateChild<gtk::Label>,
        #[template_child]
        virtual_terminal: TemplateChild<gtk::Label>,

        // other
        #[template_child]
        logo: TemplateChild<gtk::Image>,
    }

    impl AboutSystemDialog {
        fn bind_text(label: &TemplateChild<gtk::Label>, text: &Option<String>) -> bool {
            if let Some(text) = text {
                label.set_text(text);
                label.set_visible(true);

                true
            } else {
                label.set_visible(false);

                false
            }
        }

        fn bind_copy_on_activate(row: &adw::ActionRow, name: &gtk::Label, value: &gtk::Label) {
            let value = value.clone();
            let name = name.clone();

            row.connect_activated(move |_| {
                let value = value.text();
                let name = name.text();
                let text = format!("{}: {}", name, value);

                if let Some(display) = gtk::gdk::Display::default() {
                    let clipboard = display.clipboard();
                    clipboard.set_text(&text);
                }
            });
        }

        fn format_kernel_release_string(os_info: &OsInfo) -> Option<String> {
            match (os_info.os_type.clone(), os_info.kernel_release.clone()) {
                (Some(kernel), Some(release)) => Some(format!("{kernel} {release}")),
                (None, Some(release)) => Some(format!("Unknown {release}")),
                (Some(kernel), None) => Some(kernel),
                (None, None) => None,
            }
        }

        pub fn setup(&self, about: About) {
            let os_info = about.os_info;

            let _ = Self::bind_text(&self.os_name, &os_info.pretty_name)
                || Self::bind_text(&self.os_name, &os_info.name);
            let _ = Self::bind_text(&self.version, &os_info.version_id)
                || Self::bind_text(&self.version, &os_info.version);

            let _ = Self::bind_text(
                &self.kernel_release,
                &Self::format_kernel_release_string(&os_info),
            );
            let _ = Self::bind_text(&self.kernel_version, &os_info.kernel_version);

            let _ = Self::bind_text(&self.package_manager, &os_info.package_manager);
            let _ = Self::bind_text(
                &self.package_manager_version,
                &os_info.package_manager_version,
            );

            let de_info = about.de_info;

            let _ = Self::bind_text(&self.desktop_environment, &de_info.desktop_environment);
            let _ = Self::bind_text(&self.desktop_environment_version, &de_info.version);
            let _ = Self::bind_text(&self.windowing_system, &de_info.windowing_system);
            let _ = Self::bind_text(&self.virtual_terminal, &de_info.virtual_terminal);

            if os_info
                .logo
                .map(|img| CachedIcon::from(img).apply_to_image(&self.logo, 192))
                .unwrap_or(false)
            {
                self.logo.set_visible(true);
            } else {
                self.logo.set_visible(false);
            }

            Self::bind_copy_on_activate(&self.os_name_row, &self.os_name_label, &self.os_name);
            Self::bind_copy_on_activate(&self.version_row, &self.version_label, &self.version);
            Self::bind_copy_on_activate(
                &self.package_manager_row,
                &self.package_manager_label,
                &self.package_manager,
            );
            Self::bind_copy_on_activate(
                &self.package_manager_version_row,
                &self.package_manager_version_label,
                &self.package_manager_version,
            );

            Self::bind_copy_on_activate(
                &self.kernel_release_row,
                &self.kernel_release_label,
                &self.kernel_release,
            );
            Self::bind_copy_on_activate(
                &self.kernel_version_row,
                &self.kernel_version_label,
                &self.kernel_version,
            );

            Self::bind_copy_on_activate(
                &self.desktop_environment_row,
                &self.desktop_environment_label,
                &self.desktop_environment,
            );
            Self::bind_copy_on_activate(
                &self.desktop_environment_version_row,
                &self.desktop_environment_version_label,
                &self.desktop_environment_version,
            );
            Self::bind_copy_on_activate(
                &self.windowing_system_row,
                &self.windowing_system_label,
                &self.windowing_system,
            );
            Self::bind_copy_on_activate(
                &self.virtual_terminal_row,
                &self.virtual_terminal_label,
                &self.virtual_terminal,
            );

            self.os_info_box.set_visible(
                self.os_name.is_visible()
                    || self.version.is_visible()
                    || self.package_manager.is_visible()
                    || self.package_manager_version.is_visible(),
            );
            self.kernel_info_box
                .set_visible(self.kernel_release.is_visible() || self.kernel_version.is_visible());
            self.de_info_box.set_visible(
                self.desktop_environment.is_visible()
                    || self.desktop_environment_version.is_visible()
                    || self.windowing_system.is_visible()
                    || self.virtual_terminal.is_visible(),
            );

            let text = self.get_copy_all();
            let action = gio::SimpleAction::new("info-page-copy-all", None);
            action.connect_activate(move |_, _| {
                if let Some(display) = gtk::gdk::Display::default() {
                    let clipboard = display.clipboard();
                    clipboard.set_text(&text);
                }
            });
            crate::app!().add_action(&action);
        }

        fn get_copy_all(&self) -> String {
            fn get_label_contents(label: &gtk::Label) -> String {
                label.label().to_string()
            }

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
                get_label_contents(&self.os_name),
                get_label_contents(&self.version),
                get_label_contents(&self.package_manager),
                get_label_contents(&self.package_manager_version),
                get_label_contents(&self.kernel_release),
                get_label_contents(&self.kernel_version),
                get_label_contents(&self.desktop_environment),
                get_label_contents(&self.desktop_environment_version),
                get_label_contents(&self.windowing_system),
                get_label_contents(&self.virtual_terminal),
            )
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AboutSystemDialog {
        const NAME: &'static str = "AboutSystemDialog";
        type Type = super::AboutSystemDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AboutSystemDialog {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for AboutSystemDialog {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl AdwDialogImpl for AboutSystemDialog {
        fn closed(&self) {}
    }
}

glib::wrapper! {
    pub struct AboutSystemDialog(ObjectSubclass<imp::AboutSystemDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AboutSystemDialog {
    pub fn new(about: About) -> Self {
        let this: Self = glib::Object::builder()
            .property("follows-content-size", true)
            .build();

        let imp = this.imp();

        imp.setup(about);

        this
    }
}
