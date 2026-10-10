/* performance_page/disk_details.rs
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

use glib::{ParamSpec, Properties, Value};
use gtk::{gdk::prelude::*, glib, subclass::prelude::*};

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::performance_page::widgets::PartitionUsageItem;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::DiskDetails)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/disk_details.ui")]
    pub struct DiskDetails {
        #[template_child]
        pub rotation_rate: TemplateChild<gtk::Label>,

        #[template_child]
        pub active_time: TemplateChild<gtk::Label>,
        #[template_child]
        pub avg_response_time: TemplateChild<gtk::Label>,
        #[template_child]
        pub legend_read: TemplateChild<gtk::Picture>,
        #[template_child]
        pub read_speed: TemplateChild<gtk::Label>,
        #[template_child]
        pub total_read: TemplateChild<gtk::Label>,
        #[template_child]
        pub legend_write: TemplateChild<gtk::Picture>,
        #[template_child]
        pub write_speed: TemplateChild<gtk::Label>,
        #[template_child]
        pub total_write: TemplateChild<gtk::Label>,
        #[template_child]
        pub capacity: TemplateChild<gtk::Label>,
        #[template_child]
        pub formatted: TemplateChild<gtk::Label>,
        #[template_child]
        pub system_disk: TemplateChild<gtk::Label>,
        #[template_child]
        pub disk_type: TemplateChild<gtk::Label>,

        #[template_child]
        pub wwn: TemplateChild<gtk::Label>,
        #[template_child]
        pub serial_number: TemplateChild<gtk::Label>,

        #[template_child]
        pub partitions_section: TemplateChild<gtk::Box>,
        #[template_child]
        pub partitions_stack: TemplateChild<gtk::ListBox>,

        pub partitions_map: RefCell<HashMap<String, PartitionUsageItem>>,

        #[property(get, set)]
        rotation_visible: Cell<bool>,
        #[property(get, set)]
        wwn_visible: Cell<bool>,
        #[property(get, set)]
        serial_number_visible: Cell<bool>,
    }

    impl Default for DiskDetails {
        fn default() -> Self {
            Self {
                rotation_rate: Default::default(),
                active_time: Default::default(),
                avg_response_time: Default::default(),
                legend_read: Default::default(),
                read_speed: Default::default(),
                total_read: Default::default(),
                legend_write: Default::default(),
                write_speed: Default::default(),
                total_write: Default::default(),
                capacity: Default::default(),
                formatted: Default::default(),
                system_disk: Default::default(),
                disk_type: Default::default(),
                wwn: Default::default(),
                serial_number: Default::default(),
                partitions_section: Default::default(),
                partitions_stack: Default::default(),
                partitions_map: Default::default(),
                rotation_visible: Cell::new(false),
                wwn_visible: Cell::new(false),
                serial_number_visible: Cell::new(false),
            }
        }
    }

    impl DiskDetails {}

    #[glib::object_subclass]
    impl ObjectSubclass for DiskDetails {
        const NAME: &'static str = "DiskDetails";
        type Type = super::DiskDetails;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for DiskDetails {
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

            self.partitions_stack.set_sort_func(|a, b| {
                let Some(a) = a.downcast_ref::<PartitionUsageItem>() else {
                    return gtk::Ordering::Equal;
                };
                let Some(b) = b.downcast_ref::<PartitionUsageItem>() else {
                    return gtk::Ordering::Equal;
                };

                gtk::Ordering::from(b.partition_size().cmp(&a.partition_size()))
            });
        }
    }

    impl WidgetImpl for DiskDetails {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl BoxImpl for DiskDetails {}
}

glib::wrapper! {
    pub struct DiskDetails(ObjectSubclass<imp::DiskDetails>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl DiskDetails {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn rotation_rate(&self) -> &gtk::Label {
        &self.imp().rotation_rate
    }

    pub fn active_time(&self) -> &gtk::Label {
        &self.imp().active_time
    }

    pub fn avg_response_time(&self) -> &gtk::Label {
        &self.imp().avg_response_time
    }

    pub fn legend_read(&self) -> &gtk::Picture {
        &self.imp().legend_read
    }

    pub fn read_speed(&self) -> &gtk::Label {
        &self.imp().read_speed
    }

    pub fn total_read(&self) -> &gtk::Label {
        &self.imp().total_read
    }

    pub fn legend_write(&self) -> &gtk::Picture {
        &self.imp().legend_write
    }

    pub fn write_speed(&self) -> &gtk::Label {
        &self.imp().write_speed
    }

    pub fn total_write(&self) -> &gtk::Label {
        &self.imp().total_write
    }

    pub fn capacity(&self) -> &gtk::Label {
        &self.imp().capacity
    }

    pub fn formatted(&self) -> &gtk::Label {
        &self.imp().formatted
    }

    pub fn system_disk(&self) -> &gtk::Label {
        &self.imp().system_disk
    }

    pub fn disk_type(&self) -> &gtk::Label {
        &self.imp().disk_type
    }

    pub fn serial_number(&self) -> &gtk::Label {
        &self.imp().serial_number
    }

    pub fn wwn(&self) -> &gtk::Label {
        &self.imp().wwn
    }

    pub fn partitions_section(&self) -> &gtk::Box {
        &self.imp().partitions_section
    }

    pub fn partitions_stack(&self) -> &gtk::ListBox {
        &self.imp().partitions_stack
    }
}
