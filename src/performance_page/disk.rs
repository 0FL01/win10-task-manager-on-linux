/* performance_page/disk.rs
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

use std::cell::{Cell, OnceCell, RefCell};

use adw::gio::Settings;
use adw::{prelude::AdwDialogExt, subclass::prelude::*};
use glib::{g_warning, ParamSpec, Properties, Value};
use gtk::{gio, glib, prelude::*};

use magpie_types::disks::{Disk, DiskKind};

use crate::application::INTERVAL_STEP;
use crate::i18n::*;
use crate::performance_page::disk_details::DiskDetails;
use crate::performance_page::widgets::{
    AnimationFrame, DatasetGroup, EjectFailureDialog, FillingSettings, GraphWidget,
    GraphWidgetSettingsExt, PartitionUsageItem, RoundingSettings, ScalingSettings, SmartDataDialog,
    SmartFailureDialog,
};
use crate::{app, settings, to_short_human_readable_time, DataType};

use super::PageExt;

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::PerformancePageDisk)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/disk.ui")]
    pub struct PerformancePageDisk {
        #[template_child]
        pub description: TemplateChild<gtk::Box>,
        #[template_child]
        pub disk_id: TemplateChild<gtk::Label>,
        #[template_child]
        pub button_smart: TemplateChild<gtk::Button>,
        #[template_child]
        pub button_eject: TemplateChild<gtk::Button>,
        #[template_child]
        pub model: TemplateChild<gtk::Label>,
        #[template_child]
        pub usage_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub max_y: TemplateChild<gtk::Label>,
        #[template_child]
        pub graph_max_duration: TemplateChild<gtk::Label>,
        #[template_child]
        pub disk_transfer_rate_graph: TemplateChild<GraphWidget>,
        #[template_child]
        pub context_menu: TemplateChild<gtk::Popover>,

        #[property(get = Self::name, set = Self::set_name, type = String)]
        name: RefCell<String>,
        #[property(get, set)]
        base_color: Cell<gtk::gdk::RGBA>,
        #[property(get, set)]
        summary_mode: Cell<bool>,

        #[property(get = Self::infobar_content, type = Option<gtk::Widget>)]
        pub infobar_content: DiskDetails,

        pub raw_disk_id: OnceCell<String>,
    }

    impl Default for PerformancePageDisk {
        fn default() -> Self {
            Self {
                description: Default::default(),
                disk_id: Default::default(),
                button_smart: Default::default(),
                button_eject: Default::default(),
                model: Default::default(),
                usage_graph: Default::default(),
                max_y: Default::default(),
                graph_max_duration: Default::default(),
                disk_transfer_rate_graph: Default::default(),
                context_menu: Default::default(),

                name: RefCell::new(String::new()),
                base_color: Cell::new(gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 1.0)),
                summary_mode: Cell::new(false),

                infobar_content: DiskDetails::new(),

                raw_disk_id: Default::default(),
            }
        }
    }

    impl PerformancePageDisk {
        fn name(&self) -> String {
            self.name.borrow().clone()
        }

        fn set_name(&self, name: String) {
            if name == *self.name.borrow() {
                return;
            }

            self.name.replace(name);
        }

        fn infobar_content(&self) -> Option<gtk::Widget> {
            Some(self.infobar_content.clone().upcast())
        }
    }

    impl PerformancePageDisk {
        fn configure_actions(this: &super::PerformancePageDisk) {
            let actions = gio::SimpleActionGroup::new();
            this.insert_action_group("graph", Some(&actions));

            let action = gio::SimpleAction::new("copy", None);
            action.connect_activate({
                let this = this.downgrade();
                move |_, _| {
                    if let Some(this) = this.upgrade() {
                        let clipboard = this.clipboard();
                        clipboard.set_text(this.imp().data_summary().as_str());
                    }
                }
            });
            actions.add_action(&action);
        }

        fn configure_context_menu(this: &super::PerformancePageDisk) {
            let right_click_controller = gtk::GestureClick::new();
            right_click_controller.set_button(3); // Secondary click (AKA right click)
            right_click_controller.connect_released({
                let this = this.downgrade();
                move |_click, _n_press, x, y| {
                    if let Some(this) = this.upgrade() {
                        this.imp()
                            .context_menu
                            .set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                                x.round() as i32,
                                y.round() as i32,
                                1,
                                1,
                            )));
                        this.imp().context_menu.popup();
                    }
                }
            });
            this.add_controller(right_click_controller);
        }
    }

    impl PerformancePageDisk {
        pub fn set_static_information(
            this: &super::PerformancePageDisk,
            index: Option<i32>,
            disk: &Disk,
        ) -> bool {
            let this = this.imp();

            let _ = this.raw_disk_id.set(disk.id.clone());

            if index.is_some() {
                this.disk_id.set_text(&i18n_f(
                    "Disk {} ({})",
                    &[&format!("{}", index.unwrap()), &disk.id],
                ));
            } else {
                this.disk_id.set_text(&i18n_f("Drive ({})", &[&disk.id]));
            }

            if let Some(disk_model) = disk.model.as_ref() {
                this.model.set_text(disk_model);
            } else {
                this.model.set_text(&i18n("Unknown"));
            }

            this.disk_transfer_rate_graph.set_dashed(1, true);
            this.disk_transfer_rate_graph
                .set_filled(1, FillingSettings::None);

            this.infobar_content
                .legend_read()
                .set_resource(Some("/io/missioncenter/MissionCenter/line-solid-disk.svg"));
            this.infobar_content
                .legend_write()
                .set_resource(Some("/io/missioncenter/MissionCenter/line-dashed-disk.svg"));

            let cap = disk.capacity_bytes;
            this.infobar_content.capacity().set_text(&if cap > 0 {
                crate::to_human_readable_nice(cap as f32, &DataType::MemoryBytes)
            } else {
                i18n("Unknown")
            });

            let is_system_disk = if disk.is_system {
                i18n("Yes")
            } else {
                i18n("No")
            };
            this.infobar_content.system_disk().set_text(&is_system_disk);

            this.infobar_content
                .disk_type()
                .set_text(
                    &if let Some(disk_kind) = disk.kind.and_then(|k| k.try_into().ok()) {
                        let disk_type_str = match disk_kind {
                            DiskKind::Hdd => i18n("HDD"),
                            DiskKind::Ssd => i18n("SSD"),
                            DiskKind::NvMe => i18n("NVMe"),
                            DiskKind::EMmc => i18n("eMMC"),
                            DiskKind::Sd => i18n("SD"),
                            DiskKind::IScsi => i18n("iSCSI"),
                            DiskKind::Optical => i18n("Optical"),
                            DiskKind::Floppy => i18n("Floppy"),
                            DiskKind::ThumbDrive => i18n("Thumb Drive"),
                        };
                        disk_type_str
                    } else {
                        i18n("Unknown")
                    },
                );

            if disk.smart_interface.is_some() {
                this.description.set_margin_top(0);
                this.description.set_spacing(5);

                this.button_smart.set_visible(true);
                this.button_smart.connect_clicked({
                    let this = this.obj().downgrade();
                    move |_| {
                        let Some(this) = this.upgrade() else {
                            return;
                        };
                        let this = this.imp();

                        let Some(disk_id) = this.raw_disk_id.get() else {
                            g_warning!("MissionCenter::Disk", "`disk_id` was not set");
                            return;
                        };

                        let app = app!();
                        let Ok(magpie) = app.sys_info() else {
                            g_warning!("MissionCenter::Disk", "Failed to get magpie client");
                            return;
                        };

                        if let Some(smart_data) = magpie.smart_data(disk_id.clone()) {
                            let dialog = SmartDataDialog::new(smart_data);
                            dialog.present(Some(this.obj().upcast_ref::<gtk::Widget>()));
                        } else {
                            let dialogue = SmartFailureDialog::new();
                            dialogue.present(Some(this.obj().upcast_ref::<gtk::Widget>()));
                        };
                    }
                });
            }

            this.button_eject.connect_clicked({
                let this = this.obj().downgrade();
                move |_| {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    let this = this.imp();

                    let Some(disk_id) = this.raw_disk_id.get() else {
                        g_warning!("MissionCenter::Disk", "Failed to get disk_id for eject");
                        return;
                    };

                    let app = app!();
                    let Ok(magpie) = app.sys_info() else {
                        g_warning!("MissionCenter::Disk", "Failed to get magpie client");
                        return;
                    };

                    match magpie.eject_disk(disk_id) {
                        Ok(_) => {}
                        Err(e) => {
                            let dialog = EjectFailureDialog::new(disk_id.clone(), e);
                            dialog.present(Some(this.obj().upcast_ref::<gtk::Widget>()));
                        }
                    }
                }
            });

            if let Some(serial) = disk.serial_number.as_ref().map(|s| s.trim()) {
                if serial.trim().is_empty() {
                    this.infobar_content.set_serial_number_visible(false);
                } else {
                    this.infobar_content.serial_number().set_text(serial);
                    this.infobar_content.set_serial_number_visible(true);
                }
            } else {
                this.infobar_content.set_serial_number_visible(false);
            }

            if let Some(wwn) = disk.world_wide_name.as_ref().map(|s| s.trim()) {
                if wwn.is_empty() {
                    this.infobar_content.set_wwn_visible(false);
                } else {
                    this.infobar_content.wwn().set_text(wwn);
                    this.infobar_content.set_wwn_visible(true);
                }
            } else {
                this.infobar_content.set_wwn_visible(false);
            }

            true
        }

        pub fn update_readings(
            this: &super::PerformancePageDisk,
            index: Option<usize>,
            disk: &Disk,
        ) -> bool {
            let this = this.imp();

            if index.is_some() {
                this.disk_id.set_text(&i18n_f(
                    "Drive {} ({})",
                    &[&format!("{}", index.unwrap()), &disk.id],
                ));
            } else {
                this.disk_id.set_text(&i18n_f("Drive ({})", &[&disk.id]));
            }

            this.max_y.set_text(&crate::to_human_readable_nice(
                this.disk_transfer_rate_graph.get_dataset_max_scale(0),
                &DataType::DriveBytesPerSecond,
            ));

            this.usage_graph
                .add_data_point(vec![vec![disk.busy_percent]]);

            let cap = disk.formatted_bytes;
            this.infobar_content
                .formatted()
                .set_text(&if let Some(cap) = cap {
                    crate::to_human_readable_nice(cap as f32, &DataType::MemoryBytes)
                } else {
                    i18n("Unknown")
                });

            this.infobar_content
                .active_time()
                .set_text(&format!("{}%", disk.busy_percent.round() as u8));

            if let Some(rotation_rate) = disk.rotation_rate {
                this.infobar_content
                    .rotation_rate()
                    .set_text(&i18n_f("{} RPM", &[&rotation_rate.to_string()]));
                this.infobar_content.set_rotation_visible(true);
            } else {
                this.infobar_content.set_rotation_visible(false);
            }

            this.infobar_content
                .avg_response_time()
                .set_text(&format!("{:.2} ms", disk.response_time_ms));

            this.disk_transfer_rate_graph.add_data_point(vec![
                vec![disk.rx_speed_bytes_ps as f32],
                vec![disk.tx_speed_bytes_ps as f32],
            ]);
            this.infobar_content
                .read_speed()
                .set_text(&crate::to_human_readable_nice(
                    disk.rx_speed_bytes_ps as f32,
                    &DataType::DriveBytesPerSecond,
                ));

            this.infobar_content
                .total_read()
                .set_text(&crate::to_human_readable_nice(
                    disk.rx_bytes_total as f32,
                    &DataType::DriveBytes,
                ));

            this.infobar_content
                .write_speed()
                .set_text(&crate::to_human_readable_nice(
                    disk.tx_speed_bytes_ps as f32,
                    &DataType::DriveBytesPerSecond,
                ));

            this.infobar_content
                .total_write()
                .set_text(&crate::to_human_readable_nice(
                    disk.tx_bytes_total as f32,
                    &DataType::DriveBytes,
                ));

            let stack = this.infobar_content.partitions_stack();

            let mut existing_map = this.infobar_content.imp().partitions_map.borrow_mut();

            existing_map.retain(|devname, row| match disk.partitions.get(devname) {
                Some(partition) => {
                    row.update(partition);
                    true
                }
                None => {
                    stack.remove(row);
                    false
                }
            });

            for (devname, partition) in &disk.partitions {
                if !existing_map.contains_key(devname) {
                    let new_item = PartitionUsageItem::from_part_info(partition);

                    stack.append(&new_item);

                    existing_map.insert(devname.clone(), new_item);
                }
            }

            stack.invalidate_sort();

            this.infobar_content
                .partitions_section()
                .set_visible(!existing_map.is_empty());

            if disk.ejectable {
                this.description.set_margin_top(0);
                this.description.set_spacing(5);
            }

            this.button_eject.set_visible(disk.ejectable);

            true
        }

        pub fn update_animations(this: &super::PerformancePageDisk, ticks: AnimationFrame) -> bool {
            let this = this.imp();

            this.usage_graph.update_animation(ticks);
            this.disk_transfer_rate_graph.update_animation(ticks);

            true
        }

        fn data_summary(&self) -> String {
            format!(
                r#"{}

    {}

    Capacity:    {}
    Formatted:   {}
    System disk: {}
    Type:        {}

    Read speed:            {}
    Total read:            {}
    Write speed:           {}
    Total written          {}
    Active time:           {}
    Average response time: {}"#,
                self.disk_id.label(),
                self.model.label(),
                self.infobar_content.capacity().label(),
                self.infobar_content.formatted().label(),
                self.infobar_content.system_disk().label(),
                self.infobar_content.disk_type().label(),
                self.infobar_content.read_speed().label(),
                self.infobar_content.total_read().label(),
                self.infobar_content.write_speed().label(),
                self.infobar_content.total_write().label(),
                self.infobar_content.active_time().label(),
                self.infobar_content.avg_response_time().label(),
            )
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PerformancePageDisk {
        const NAME: &'static str = "PerformancePageDisk";
        type Type = super::PerformancePageDisk;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PerformancePageDisk {
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

            let mut tx_dataset = DatasetGroup::new();
            let mut rx_dataset = DatasetGroup::new();

            rx_dataset.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
            rx_dataset.dataset_settings.rounding_settings = RoundingSettings::Pow2;
            rx_dataset.dataset_settings.dashed = false;

            tx_dataset.dataset_settings.scaling_settings = ScalingSettings::ScaleUp;
            tx_dataset.dataset_settings.rounding_settings = RoundingSettings::Pow2;
            tx_dataset.dataset_settings.fill = FillingSettings::None;
            tx_dataset.dataset_settings.dashed = true;

            self.disk_transfer_rate_graph.add_dataset(rx_dataset);
            self.disk_transfer_rate_graph.add_dataset(tx_dataset);

            self.disk_transfer_rate_graph.connect_datasets(0, 1);

            let usage_dataset = DatasetGroup::new();

            self.usage_graph.add_dataset(usage_dataset);

            self.disk_transfer_rate_graph
                .connect_to_settings(&settings!());
            self.usage_graph.connect_to_settings(&settings!());

            let obj = self.obj();
            let this = obj.upcast_ref::<super::PerformancePageDisk>().clone();

            Self::configure_actions(&this);
            Self::configure_context_menu(&this);
        }
    }

    impl WidgetImpl for PerformancePageDisk {}

    impl BoxImpl for PerformancePageDisk {}
}

glib::wrapper! {
    pub struct PerformancePageDisk(ObjectSubclass<imp::PerformancePageDisk>)
        @extends gtk::Box, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl PageExt for PerformancePageDisk {
    fn infobar_collapsed(&self) {
        self.imp().infobar_content.set_margin_top(10);
    }

    fn infobar_uncollapsed(&self) {
        self.imp().infobar_content.set_margin_top(65);
    }
}

impl PerformancePageDisk {
    pub fn new(name: &str, settings: &gio::Settings) -> Self {
        let this: Self = glib::Object::builder().property("name", name).build();

        fn update_refresh_rate_sensitive_labels(
            this: &PerformancePageDisk,
            settings: &gio::Settings,
        ) {
            let data_points = settings.int("performance-page-data-points") as u32;
            let delay = settings.uint64("app-update-interval-u64");
            let graph_max_duration =
                (((delay as f64) * INTERVAL_STEP) * (data_points as f64)).round() as u32;

            let this = this.imp();

            this.graph_max_duration
                .set_text(&to_short_human_readable_time(graph_max_duration));
        }
        update_refresh_rate_sensitive_labels(&this, settings);

        settings.connect_changed(Some("performance-page-data-points"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        settings.connect_changed(Some("app-update-interval-u64"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    update_refresh_rate_sensitive_labels(&this, settings);
                }
            }
        });

        settings.connect_changed(Some("performance-page-drive-use-base2"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    this.update_graph_scaling(settings);
                }
            }
        });

        settings.connect_changed(Some("performance-page-drive-use-bytes"), {
            let this = this.downgrade();
            move |settings, _| {
                if let Some(this) = this.upgrade() {
                    this.update_graph_scaling(settings);
                }
            }
        });

        this.update_graph_scaling(settings);

        this
    }

    fn update_graph_scaling(&self, settings: &Settings) {
        let this = self.imp();
        let transfer_rate_graph = &this.disk_transfer_rate_graph;

        let base2 = settings.boolean("performance-page-drive-use-base2");

        if base2 {
            transfer_rate_graph.set_all_datasets_rounding(RoundingSettings::Pow2);
        } else {
            transfer_rate_graph.set_all_datasets_rounding(RoundingSettings::Pow2Base10);

            let bytes = settings.boolean("performance-page-drive-use-bytes");

            transfer_rate_graph.set_all_datasets_watermarking_multiplier(if bytes {
                1.
            } else {
                8.
            });
        }

        transfer_rate_graph.reset_auto_scaling();
    }

    pub fn set_static_information(&self, index: Option<i32>, disk: &Disk) -> bool {
        imp::PerformancePageDisk::set_static_information(self, index, disk)
    }

    pub fn update_readings(&self, index: Option<usize>, disk: &Disk) -> bool {
        imp::PerformancePageDisk::update_readings(self, index, disk)
    }

    pub fn update_animations(&self, ticks: AnimationFrame) -> bool {
        imp::PerformancePageDisk::update_animations(self, ticks)
    }
}
