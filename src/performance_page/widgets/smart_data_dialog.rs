/* performance_page/widgets/smart_data_dialog.rs
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

use std::time::{SystemTime, UNIX_EPOCH};

use adw::{prelude::*, subclass::prelude::*};
use gtk::gio;
use gtk::glib::{self, g_critical};
use gtk::{Align, ColumnViewColumn};

use magpie_types::disks::smart_data::{Ata, Nvme};
use magpie_types::disks::{smart_data, SmartData};

use crate::i18n::*;

use super::SmartNvmeDialogRow;
use super::SmartSataDialogRow;

mod imp {
    use super::*;
    use crate::DataType;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/io/missioncenter/MissionCenter/ui/performance_page/disk_smart_data_dialog.ui"
    )]
    pub struct SmartDataDialog {
        #[template_child]
        pub sata_column_view: TemplateChild<gtk::ColumnView>,
        #[template_child]
        pub sata_id_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_attribute_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_value_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_normalized_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_threshold_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_worst_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_type_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_updates_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub sata_assessment_column: TemplateChild<ColumnViewColumn>,

        #[template_child]
        pub nvme_column_view: TemplateChild<gtk::ColumnView>,
        #[template_child]
        pub nvme_name_column: TemplateChild<ColumnViewColumn>,
        #[template_child]
        pub nvme_value_column: TemplateChild<ColumnViewColumn>,

        #[template_child]
        pub powered_on: TemplateChild<gtk::Label>,
        #[template_child]
        pub status: TemplateChild<gtk::Label>,
        #[template_child]
        pub last_updated: TemplateChild<gtk::Label>,
        #[template_child]
        pub sata_data: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub nvme_data: TemplateChild<gtk::ScrolledWindow>,
    }

    impl SmartDataDialog {
        pub fn update_model(&self, data: SmartData) {
            let powered_on_nice = crate::to_long_human_readable_time(data.powered_on_seconds);
            self.powered_on.set_text(&powered_on_nice);

            if let Ok(since_the_epoch) = SystemTime::now().duration_since(UNIX_EPOCH) {
                let last_updated_nice = crate::to_long_human_readable_time(
                    (since_the_epoch.as_secs_f32() as u64).saturating_sub(data.last_update_time),
                );
                self.last_updated
                    .set_text(&i18n_f("{} ago", &[&last_updated_nice]));
            } else {
                g_critical!("MissionCenter::SMARTDialog", "Time somehow went backwards");
                self.last_updated.set_text(&i18n("Unknown"));
            }

            self.status
                .set_text(format!("{:?}", data.test_result()).as_str());

            match data.data {
                Some(smart_data::Data::Ata(ata)) => self.apply_ata_smart_data(ata),
                Some(smart_data::Data::Nvme(nvme)) => self.apply_nvme_smart_data(nvme),
                None => {
                    self.sata_data.set_visible(false);
                    self.nvme_data.set_visible(false);
                }
            }
        }

        fn apply_ata_smart_data(&self, ata_smart_data: Ata) {
            self.sata_data.set_visible(true);
            self.nvme_data.set_visible(false);

            let mut rows = Vec::new();

            for parsed_result in ata_smart_data.attributes {
                let new_row = SmartSataDialogRow::new(
                    parsed_result.id as u8,
                    parsed_result.name,
                    parsed_result.value,
                    parsed_result.pretty,
                    parsed_result.pretty_unit,
                    parsed_result.threshold,
                    parsed_result.worst,
                    &match parsed_result.flags & 0b1 {
                        1 => i18n("Pre-Fail"),
                        _ => i18n("Old-Age"),
                    },
                    &match parsed_result.flags & 0b10 >> 1 {
                        0 => i18n("Online"),
                        _ => i18n("Offline"),
                    },
                    // thanks GDU: https://gitlab.gnome.org/GNOME/gnome-disk-utility/-/blob/5ad540d4afe46f112174baeb9818c1eda64f2cc0/src/disks/gdu-ata-smart-dialog.c#L680
                    &if parsed_result.value > 0
                        && parsed_result.threshold > 0
                        && parsed_result.value <= parsed_result.threshold
                    {
                        i18n("FAILING")
                    } else if parsed_result.worst > 0
                        && parsed_result.threshold > 0
                        && parsed_result.worst <= parsed_result.threshold
                    {
                        i18n("Failed in the past")
                    } else {
                        i18n("Ok")
                    },
                );

                rows.push(new_row);
            }

            let rows: gio::ListStore = rows.into_iter().collect();

            let column_view: gtk::ColumnView = self.sata_column_view.get();
            let id_col: ColumnViewColumn = self.sata_id_column.get();
            let att_col: ColumnViewColumn = self.sata_attribute_column.get();
            let val_col: ColumnViewColumn = self.sata_value_column.get();
            let nor_col: ColumnViewColumn = self.sata_normalized_column.get();
            let thr_col: ColumnViewColumn = self.sata_threshold_column.get();
            let wor_col: ColumnViewColumn = self.sata_worst_column.get();
            let typ_col: ColumnViewColumn = self.sata_type_column.get();
            let upd_col: ColumnViewColumn = self.sata_updates_column.get();
            let ass_col: ColumnViewColumn = self.sata_assessment_column.get();

            Self::setup_sata_column_factory(id_col, Align::Start, |mi| mi.smart_id().to_string());
            Self::setup_sata_column_factory(att_col, Align::Start, |mi| mi.attribute().to_string());
            Self::setup_sata_column_factory(val_col, Align::Start, |mi| mi.value().to_string());
            Self::setup_sata_column_factory(nor_col, Align::Start, |mi| {
                mi.normalized().to_string()
            });
            Self::setup_sata_column_factory(thr_col, Align::Start, |mi| mi.threshold().to_string());
            Self::setup_sata_column_factory(wor_col, Align::Start, |mi| mi.worst().to_string());
            Self::setup_sata_column_factory(typ_col, Align::Start, |mi| mi.typee().to_string());
            Self::setup_sata_column_factory(upd_col, Align::Start, |mi| mi.updates().to_string());
            Self::setup_sata_column_factory(ass_col, Align::Start, |mi| {
                mi.assessment().to_string()
            });

            let sort_model = gtk::SortListModel::builder()
                .model(&rows)
                .sorter(&column_view.sorter().unwrap())
                .build();

            column_view.set_model(Some(&gtk::SingleSelection::new(Some(sort_model))));
        }

        fn apply_nvme_smart_data(&self, result: Nvme) {
            self.sata_data.set_visible(false);
            self.nvme_data.set_visible(true);

            let rows = [
                SmartNvmeDialogRow::new(
                    i18n("Percent Used"),
                    if let Some(percent_used) = result.percent_used {
                        format!("{}%", percent_used)
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Available Spare"),
                    if let Some(avail_spare) = result.avail_spare {
                        avail_spare.to_string()
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Spare Threshold"),
                    if let Some(spare_thresh) = result.spare_thresh {
                        spare_thresh.to_string()
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Total Data Read"),
                    if let Some(total_data_read) = result.total_data_read {
                        crate::to_human_readable_nice(total_data_read as f32, &DataType::DriveBytes)
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Total Data Written"),
                    if let Some(total_data_written) = result.total_data_written {
                        crate::to_human_readable_nice(
                            total_data_written as f32,
                            &DataType::DriveBytes,
                        )
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Warning Temp Time"),
                    if let Some(warning_temp_time) = result.warn_composite_temp_time {
                        crate::to_long_human_readable_time(warning_temp_time as u64)
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Critical Temp Time"),
                    if let Some(critical_temp_time) = result.crit_composite_temp_time {
                        crate::to_long_human_readable_time(critical_temp_time as u64)
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Control Busy Time"),
                    if let Some(ctrl_busy_minutes) = result.ctrl_busy_minutes {
                        crate::to_long_human_readable_time(ctrl_busy_minutes)
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Warning Temperature"),
                    if let Some(wctemp) = result.warn_composite_temp_thresh {
                        i18n_f("{} °C", &[&format!("{}", wctemp - 273)])
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Critical Temperature"),
                    if let Some(cctemp) = result.crit_composite_temp_thresh {
                        i18n_f("{} °C", &[&format!("{}", cctemp - 273)])
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(i18n("Temps"), format!("{:?}", result.temp_sensors)),
                SmartNvmeDialogRow::new(
                    i18n("Unsafe Shutdowns"),
                    if let Some(unsafe_shutdowns) = result.unsafe_shutdowns {
                        unsafe_shutdowns.to_string()
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Media Errors"),
                    if let Some(media_errors) = result.media_errors {
                        media_errors.to_string()
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Number Error Log Entries"),
                    if let Some(num_err_log_entries) = result.num_err_log_entries {
                        num_err_log_entries.to_string()
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Power Cycles"),
                    if let Some(power_cycles) = result.power_cycles {
                        power_cycles.to_string()
                    } else {
                        i18n("N/A")
                    },
                ),
                SmartNvmeDialogRow::new(
                    i18n("Control Busy Time"),
                    if let Some(ctrl_busy_minutes) = result.ctrl_busy_minutes {
                        ctrl_busy_minutes.to_string()
                    } else {
                        i18n("N/A")
                    },
                ),
            ];

            let rows: gio::ListStore = rows.into_iter().collect();

            let column_view: gtk::ColumnView = self.nvme_column_view.get();
            let id_col: ColumnViewColumn = self.nvme_name_column.get();
            let att_col: ColumnViewColumn = self.nvme_value_column.get();

            Self::setup_nvme_column_factory(id_col, Align::Start, |mi| mi.label().to_string());
            Self::setup_nvme_column_factory(att_col, Align::Start, |mi| mi.value().to_string());

            let sort_model = gtk::SortListModel::builder()
                .model(&rows)
                .sorter(&column_view.sorter().unwrap())
                .build();

            column_view.set_model(Some(&gtk::SingleSelection::new(Some(sort_model))));
        }

        fn setup_sata_column_factory<'a, E>(id_col: ColumnViewColumn, alignment: Align, extract: E)
        where
            E: Fn(SmartSataDialogRow) -> String + 'static,
        {
            let factory_id_col = gtk::SignalListItemFactory::new();
            factory_id_col.connect_setup(move |_factory, list_item| {
                let cell = list_item.downcast_ref::<gtk::ColumnViewCell>().unwrap();
                cell.set_child(Some(&gtk::Label::builder().halign(alignment).build()));
            });
            factory_id_col.connect_bind(move |_factory, list_item| {
                let cell = match list_item.downcast_ref::<gtk::ColumnViewCell>() {
                    Some(cell) => cell,
                    None => {
                        g_critical!(
                            "MissionCenter::SMARTDialog",
                            "Failed to obtain GtkColumnViewCell from list item"
                        );
                        return;
                    }
                };

                let model_item = match cell
                    .item()
                    .and_then(|i| i.downcast::<SmartSataDialogRow>().ok())
                {
                    Some(model_item) => model_item,
                    None => {
                        g_critical!(
                            "MissionCenter::SMARTDialog",
                            "Failed to obtain SmartDialogRow item from GtkColumnViewCell"
                        );
                        return;
                    }
                };

                let label_object = match cell.child().and_then(|c| c.downcast::<gtk::Label>().ok())
                {
                    Some(label) => label,
                    None => {
                        g_critical!(
                            "MissionCenter::SMARTDialog",
                            "Failed to obtain child GtkLabel from GtkColumnViewCell"
                        );
                        return;
                    }
                };

                label_object.set_label(&extract(model_item));
            });

            id_col.set_factory(Some(&factory_id_col));
        }

        fn setup_nvme_column_factory<'a, E>(id_col: ColumnViewColumn, alignment: Align, extract: E)
        where
            E: Fn(SmartNvmeDialogRow) -> String + 'static,
        {
            let factory_id_col = gtk::SignalListItemFactory::new();
            factory_id_col.connect_setup(move |_factory, list_item| {
                let cell = list_item.downcast_ref::<gtk::ColumnViewCell>().unwrap();
                cell.set_child(Some(&gtk::Label::builder().halign(alignment).build()));
            });
            factory_id_col.connect_bind(move |_factory, list_item| {
                let cell = match list_item.downcast_ref::<gtk::ColumnViewCell>() {
                    Some(cell) => cell,
                    None => {
                        g_critical!(
                            "MissionCenter::SMARTDialog",
                            "Failed to obtain GtkColumnViewCell from list item"
                        );
                        return;
                    }
                };

                let model_item = match cell
                    .item()
                    .and_then(|i| i.downcast::<SmartNvmeDialogRow>().ok())
                {
                    Some(model_item) => model_item,
                    None => {
                        g_critical!(
                            "MissionCenter::SMARTDialog",
                            "Failed to obtain SmartDialogRow item from GtkColumnViewCell"
                        );
                        return;
                    }
                };

                let label_object = match cell.child().and_then(|c| c.downcast::<gtk::Label>().ok())
                {
                    Some(label) => label,
                    None => {
                        g_critical!(
                            "MissionCenter::SMARTDialog",
                            "Failed to obtain child GtkLabel from GtkColumnViewCell"
                        );
                        return;
                    }
                };

                label_object.set_label(&extract(model_item));
            });

            id_col.set_factory(Some(&factory_id_col));
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SmartDataDialog {
        const NAME: &'static str = "SmartDataDialog";
        type Type = super::SmartDataDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SmartDataDialog {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for SmartDataDialog {
        fn realize(&self) {
            self.parent_realize();
        }
    }

    impl AdwDialogImpl for SmartDataDialog {
        fn closed(&self) {}
    }
}

glib::wrapper! {
    pub struct SmartDataDialog(ObjectSubclass<imp::SmartDataDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SmartDataDialog {
    pub fn new(smart_data: SmartData) -> Self {
        let this: Self = glib::Object::builder()
            .property("follows-content-size", true)
            .build();
        {
            let this = this.imp();
            this.update_model(smart_data);
        }

        this
    }
}
