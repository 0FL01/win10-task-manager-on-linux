/* table_view/process_details_dialog.rs
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

use std::cell::RefCell;

use adw::subclass::prelude::*;
use adw::PreferencesRow;
use gtk::glib::{self};
use gtk::prelude::{StaticTypeExt, WidgetExt};

use crate::table_view::columns::*;
use crate::table_view::row_model::{ContentType, RowModel};

mod imp {
    use super::*;

    #[derive(gtk::CompositeTemplate)]
    #[template(
        resource = "/io/missioncenter/MissionCenter/ui/table_view/process_details_dialog.ui"
    )]
    pub struct ProcessDetailsDialog {
        #[template_child]
        icon: TemplateChild<gtk::Image>,
        #[template_child]
        title: TemplateChild<gtk::Label>,

        #[template_child]
        id_value: TemplateChild<gtk::Label>,
        #[template_child]
        app_id_label: TemplateChild<gtk::Label>,
        #[template_child]
        pid_label: TemplateChild<gtk::Label>,
        #[template_child]
        kind: TemplateChild<gtk::Label>,
        #[template_child]
        command_line_row: TemplateChild<PreferencesRow>,
        #[template_child]
        command_line: TemplateChild<gtk::Label>,

        #[template_child]
        cpu: TemplateChild<LabelCell>,
        #[template_child]
        memory: TemplateChild<LabelCell>,
        #[template_child]
        swap: TemplateChild<LabelCell>,
        #[template_child]
        drives: TemplateChild<LabelCell>,
        #[template_child]
        network: TemplateChild<LabelCell>,
        #[template_child]
        gpu: TemplateChild<LabelCell>,
        #[template_child]
        gpu_memory: TemplateChild<LabelCell>,

        pub model: RefCell<RowModel>,
    }

    impl Default for ProcessDetailsDialog {
        fn default() -> Self {
            Self {
                icon: TemplateChild::default(),
                title: TemplateChild::default(),

                id_value: TemplateChild::default(),
                app_id_label: TemplateChild::default(),
                pid_label: TemplateChild::default(),
                kind: TemplateChild::default(),
                command_line_row: TemplateChild::default(),
                command_line: TemplateChild::default(),

                cpu: TemplateChild::default(),
                memory: TemplateChild::default(),
                swap: TemplateChild::default(),
                drives: TemplateChild::default(),
                network: TemplateChild::default(),
                gpu: TemplateChild::default(),
                gpu_memory: TemplateChild::default(),

                model: RefCell::new(RowModel::new(ContentType::SectionHeader)),
            }
        }
    }

    impl ProcessDetailsDialog {
        pub fn bind(&self) {
            let model = self.model.borrow();

            match model.content_type() {
                ContentType::App => {
                    self.app_id_label.set_visible(true);
                    self.pid_label.set_visible(false);
                }
                ContentType::Process => {
                    self.app_id_label.set_visible(false);
                    self.pid_label.set_visible(true);
                }
                _ => {} // should never happen
            }

            model
                .imp()
                .light_icon()
                .apply_to_image_custom_size(&self.icon, 48);

            self.title.set_label(&model.name());

            self.id_value.set_label(&model.id());

            let content_type: String = model.content_type().into();
            self.kind.set_label(&content_type);

            let cli: String = model.command_line().into();
            self.command_line.set_label(&cli);

            self.command_line_row.set_visible(!cli.is_empty());

            cpu_label_formatter(&*self.cpu, model.cpu_usage().into());
            self.cpu.bind(&*model, "cpu-usage", cpu_label_formatter);

            memory_label_formatter(&*self.memory, model.memory_usage().into());
            self.memory
                .bind(&*model, "memory-usage", memory_label_formatter);

            swap_label_formatter(&*self.swap, model.swap_usage().into());
            self.swap
                .bind(&*model, "shared-memory-usage", swap_label_formatter);

            drive_label_formatter(&*self.drives, model.disk_usage().into());
            self.drives
                .bind(&*model, "disk-usage", drive_label_formatter);

            network_label_formatter(&*self.network, model.network_usage().into());
            self.network
                .bind(&*model, "network-usage", network_label_formatter);

            gpu_label_formatter(&*self.gpu, model.gpu_usage().into());
            self.gpu.bind(&*model, "gpu-usage", gpu_label_formatter);

            gpu_memory_label_formatter(&*self.gpu_memory, model.gpu_memory_usage().into());
            self.gpu_memory
                .bind(&*model, "gpu-memory-usage", gpu_memory_label_formatter);
        }

        fn unbind(&self) {
            self.cpu.unbind();
            self.memory.unbind();
            self.swap.unbind();
            self.drives.unbind();
            self.network.unbind();
            self.gpu.unbind();
            self.gpu_memory.unbind();
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ProcessDetailsDialog {
        const NAME: &'static str = "ProcessDetailsDialog";
        type Type = super::ProcessDetailsDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            LabelCell::ensure_type();

            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ProcessDetailsDialog {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for ProcessDetailsDialog {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl AdwDialogImpl for ProcessDetailsDialog {
        fn closed(&self) {
            self.unbind();
        }
    }
}

glib::wrapper! {
    pub struct ProcessDetailsDialog(ObjectSubclass<imp::ProcessDetailsDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ProcessDetailsDialog {
    pub fn new(model: RowModel) -> Self {
        let this: Self = glib::Object::builder()
            .property("follows-content-size", true)
            .build();

        let imp = this.imp();

        imp.model.replace(model);
        imp.bind();

        this
    }
}
