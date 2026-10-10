/* table_view/row_model.rs
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

use gtk::{
    gio, glib,
    glib::{prelude::*, subclass::prelude::*, ParamSpec, Properties, Value},
};

use crate::i18n::i18n;
use crate::table_view::cached_icon::LightCachedIcon;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::RowModel)]
    pub struct RowModel {
        #[property(get = Self::id, set = Self::set_id)]
        pub id: Cell<glib::GString>,

        #[property(get, set)]
        pub pid: Cell<u32>,

        #[property(get, set)]
        pub service_id: Cell<u64>,

        #[property(get = Self::name, set = Self::set_name)]
        pub name: Cell<glib::GString>,

        #[property(get, type = ContentType, builder(ContentType::SectionHeader))]
        pub content_type: Cell<ContentType>,
        #[property(get, type = SectionType, builder(SectionType::FirstSection))]
        pub section_type: Cell<SectionType>,

        #[property(get, set)]
        pub cpu_usage: Cell<f32>,
        #[property(get, set)]
        pub memory_usage: Cell<u64>,
        #[property(get, set)]
        pub swap_usage: Cell<u64>,
        #[property(get, set)]
        pub disk_usage: Cell<f32>,
        #[property(get, set)]
        pub network_usage: Cell<f32>,
        #[property(get, set)]
        pub gpu_usage: Cell<f32>,
        #[property(get, set)]
        pub gpu_memory_usage: Cell<u64>,

        #[property(get, set)]
        pub service_enabled: Cell<bool>,
        #[property(get, set)]
        pub service_running: Cell<bool>,
        #[property(get, set)]
        pub service_failed: Cell<bool>,
        #[property(get, set)]
        pub service_stopped: Cell<bool>,

        #[property(get = Self::user, set = Self::set_user)]
        pub user: Cell<glib::GString>,
        #[property(get = Self::group, set = Self::set_group)]
        pub group: Cell<glib::GString>,
        #[property(get = Self::description, set = Self::set_description)]
        pub description: Cell<glib::GString>,
        #[property(get = Self::file_path, set = Self::set_file_path)]
        pub file_path: Cell<glib::GString>,

        #[property(get = Self::command_line, set = Self::set_command_line)]
        pub command_line: Cell<glib::GString>,

        #[property(get, set)]
        pub light_icon_changed: Cell<bool>,

        pub light_icon: Cell<LightCachedIcon>,

        pub children: RefCell<gio::ListStore>,
    }

    impl Default for RowModel {
        fn default() -> Self {
            Self {
                id: Cell::new(glib::GString::default()),
                light_icon: Cell::new(Default::default()),

                pid: Cell::new(0),

                service_id: Cell::new(0),

                name: Cell::new(Default::default()),

                content_type: Cell::new(ContentType::SectionHeader),
                section_type: Cell::new(SectionType::FirstSection),

                cpu_usage: Cell::new(0.),
                memory_usage: Cell::new(0),
                swap_usage: Cell::new(0),
                disk_usage: Cell::new(0.),
                network_usage: Cell::new(0.),
                gpu_usage: Cell::new(0.),
                gpu_memory_usage: Cell::new(0),

                service_enabled: Cell::new(false),
                service_running: Cell::new(false),
                service_failed: Cell::new(false),
                service_stopped: Cell::new(false),

                user: Cell::new(Default::default()),
                group: Cell::new(Default::default()),
                description: Cell::new(Default::default()),
                file_path: Cell::new(Default::default()),

                command_line: Cell::new(Default::default()),

                light_icon_changed: Cell::new(false),

                children: RefCell::new(gio::ListStore::new::<super::RowModel>()),
            }
        }
    }

    impl RowModel {
        pub fn id(&self) -> glib::GString {
            let id = self.id.take();
            self.id.set(id.clone());

            id
        }

        pub fn set_id(&self, id: &str) {
            self.id.set(glib::GString::from(id));
        }

        pub fn name(&self) -> glib::GString {
            let name = self.name.take();
            self.name.set(name.clone());

            name
        }

        pub fn set_name(&self, name: &str) {
            self.name.set(glib::GString::from(name));
        }

        pub fn user(&self) -> glib::GString {
            let user = self.user.take();
            self.user.set(user.clone());

            user
        }

        pub fn set_user(&self, user: &str) {
            self.user.set(glib::GString::from(user));
        }

        pub fn group(&self) -> glib::GString {
            let group = self.group.take();
            self.group.set(group.clone());

            group
        }

        pub fn set_group(&self, group: &str) {
            self.group.set(glib::GString::from(group));
        }

        pub fn description(&self) -> glib::GString {
            let description = self.description.take();
            self.description.set(description.clone());

            description
        }

        pub fn set_description(&self, description: &str) {
            self.description.set(glib::GString::from(description));
        }

        pub fn file_path(&self) -> glib::GString {
            let file_path = self.file_path.take();
            let result = file_path.clone();
            self.file_path.set(file_path);

            result
        }

        pub fn set_file_path(&self, file_path: &str) {
            let current_file_path = self.file_path.take();
            if current_file_path == file_path {
                self.file_path.set(current_file_path);
                return;
            }

            self.file_path.set(glib::GString::from(file_path));
        }

        pub fn command_line(&self) -> glib::GString {
            let command_line = self.command_line.take();
            self.command_line.set(command_line.clone());

            command_line
        }

        pub fn set_command_line(&self, command_line: &str) {
            self.command_line.set(glib::GString::from(command_line));
        }

        pub fn light_icon(&self) -> LightCachedIcon {
            let icon_name = self.light_icon.take();
            self.light_icon.set(icon_name.clone());

            icon_name
        }

        pub fn set_light_icon(&self, icon_name: LightCachedIcon) {
            let old = self.light_icon.replace(icon_name.clone());

            if old != icon_name {
                self.light_icon_changed.set(!self.light_icon_changed.get());
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RowModel {
        const NAME: &'static str = "RowModel";
        type Type = super::RowModel;
    }

    impl ObjectImpl for RowModel {
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
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, glib::Enum, Ord, PartialOrd)]
#[enum_type(name = "ContentType")]
pub enum ContentType {
    SectionHeader,
    Service,
    App,
    Process,
}

impl From<ContentType> for String {
    fn from(value: ContentType) -> Self {
        match value {
            ContentType::SectionHeader => i18n("Section Header"),
            ContentType::Service => i18n("Service"),
            ContentType::App => i18n("App"),
            ContentType::Process => i18n("Process"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, glib::Enum, Ord, PartialOrd)]
#[enum_type(name = "SectionType")]
pub enum SectionType {
    FirstSection,
    SecondSection,
}

pub struct RowModelBuilder {
    id: glib::GString,

    light_cached_icon: LightCachedIcon,

    pid: u32,

    service_id: u64,

    name: glib::GString,
    command_line: glib::GString,

    content_type: ContentType,
    section_type: SectionType,

    cpu_usage: f32,
    memory_usage: u64,
    swap_usage: u64,
    disk_usage: f32,
    network_usage: f32,
    gpu_usage: f32,
    gpu_mem_usage: u64,

    // service related
    enabled: bool,
    running: bool,
    stopped: bool,
    failed: bool,

    user: glib::GString,
    group: glib::GString,
    file_path: glib::GString,
    description: glib::GString,
}

#[allow(unused)]
impl RowModelBuilder {
    pub fn new() -> Self {
        Self {
            id: glib::GString::default(),

            light_cached_icon: Default::default(),

            pid: 0,

            service_id: 0,

            name: glib::GString::default(),
            command_line: Default::default(),

            content_type: ContentType::SectionHeader,
            section_type: SectionType::FirstSection,

            cpu_usage: 0.,
            memory_usage: 0,
            swap_usage: 0,
            disk_usage: 0.,
            network_usage: 0.,
            gpu_usage: 0.,
            gpu_mem_usage: 0,

            enabled: false,
            running: false,
            stopped: false,
            failed: false,

            user: Default::default(),
            group: Default::default(),
            file_path: Default::default(),
            description: Default::default(),
        }
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = id.into();
        self
    }

    pub fn light_cached_icon(mut self, light_cached_icon: LightCachedIcon) -> Self {
        self.light_cached_icon = light_cached_icon;
        self
    }

    pub fn pid(mut self, pid: u32) -> Self {
        self.pid = pid;
        self
    }

    pub fn service_id(mut self, service_id: u64) -> Self {
        self.service_id = service_id;
        self
    }

    pub fn name(mut self, name: &str) -> Self {
        self.name = name.into();
        self
    }

    pub fn command_line(mut self, command_line: &str) -> Self {
        self.command_line = command_line.into();
        self
    }

    pub fn content_type(mut self, content_type: ContentType) -> Self {
        self.content_type = content_type;
        self
    }

    pub fn section_type(mut self, section_type: SectionType) -> Self {
        self.section_type = section_type;
        self
    }

    pub fn cpu_usage(mut self, cpu_usage: f32) -> Self {
        self.cpu_usage = cpu_usage;
        self
    }

    pub fn memory_usage(mut self, memory_usage: u64) -> Self {
        self.memory_usage = memory_usage;
        self
    }

    pub fn swap_usage(mut self, swap_usage: u64) -> Self {
        self.swap_usage = swap_usage;
        self
    }

    pub fn disk_usage(mut self, disk_usage: f32) -> Self {
        self.disk_usage = disk_usage;
        self
    }

    pub fn network_usage(mut self, network_usage: f32) -> Self {
        self.network_usage = network_usage;
        self
    }

    pub fn gpu_usage(mut self, gpu_usage: f32) -> Self {
        self.gpu_usage = gpu_usage;
        self
    }

    pub fn gpu_mem_usage(mut self, gpu_mem_usage: u64) -> Self {
        self.gpu_mem_usage = gpu_mem_usage;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn running(mut self, running: bool) -> Self {
        self.running = running;
        self
    }

    pub fn stopped(mut self, stopped: bool) -> Self {
        self.stopped = stopped;
        self
    }

    pub fn failed(mut self, failed: bool) -> Self {
        self.failed = failed;
        self
    }

    pub fn user(mut self, user: &str) -> Self {
        self.user = user.into();
        self
    }

    pub fn group(mut self, group: &str) -> Self {
        self.group = group.into();
        self
    }

    pub fn file_path(mut self, file_path: &str) -> Self {
        self.file_path = file_path.into();
        self
    }

    pub fn description(mut self, description: &str) -> Self {
        self.description = description.into();
        self
    }

    pub fn build(self) -> RowModel {
        let this = RowModel::new(self.content_type);

        {
            let this = this.imp();

            this.id.set(self.id);
            this.light_icon.set(self.light_cached_icon);
            this.pid.set(self.pid);
            this.service_id.set(self.service_id);
            this.name.set(self.name);

            this.section_type.set(self.section_type);

            this.cpu_usage.set(self.cpu_usage);
            this.memory_usage.set(self.memory_usage);
            this.swap_usage.set(self.swap_usage);
            this.disk_usage.set(self.disk_usage);
            this.network_usage.set(self.network_usage);
            this.gpu_usage.set(self.gpu_usage);
            this.gpu_memory_usage.set(self.gpu_mem_usage);

            this.service_enabled.set(self.enabled);
            this.service_running.set(self.running);
            this.service_stopped.set(self.stopped);
            this.service_failed.set(self.failed);

            this.user.set(self.user);
            this.group.set(self.group);
            this.file_path.set(self.file_path);
            this.description.set(self.description);
        }

        this
    }
}

glib::wrapper! {
    pub struct RowModel(ObjectSubclass<imp::RowModel>);
}

impl RowModel {
    pub fn new(content_type: ContentType) -> Self {
        let this: Self = glib::Object::builder().build();
        this.imp().content_type.set(content_type);

        this
    }

    pub fn children(&self) -> gio::ListStore {
        self.imp().children.borrow().clone()
    }

    pub fn set_children(&self, children: gio::ListStore) {
        self.imp().children.replace(children);
    }
}
