/* performance_page/widgets/partition_usage_item
 *
 * Copyright 2026 Mission Center Devs
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

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::{self, g_warning};
use gtk::pango;

use magpie_types::disks::PartitionInfo;

use crate::i18n::{i18n, ni18n};

mod imp {
    use super::*;

    #[derive(Default)]
    pub(super) struct Cache {
        pub mount_points: Vec<String>,
        pub part_size: Option<u64>,
        pub part_used: Option<u64>,
        pub filesystem: Option<String>,
    }

    #[derive(gtk::CompositeTemplate, Default)]
    #[template(
        resource = "/io/missioncenter/MissionCenter/ui/performance_page/disk_partition_usage_item.ui"
    )]
    pub struct PartitionUsageItem {
        #[template_child]
        pub(super) devname_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub(super) filesystem_type: TemplateChild<gtk::Label>,
        #[template_child]
        pub(super) mount_points_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub(super) usage_bar: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub(super) used_amount: TemplateChild<gtk::Label>,
        #[template_child]
        pub(super) usage_pct: TemplateChild<gtk::Label>,
        #[template_child]
        pub(super) total_amount: TemplateChild<gtk::Label>,

        pub(super) cache: RefCell<Cache>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PartitionUsageItem {
        const NAME: &'static str = "PartitionUsageItem";
        type Type = super::PartitionUsageItem;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PartitionUsageItem {
        fn constructed(&self) {
            self.parent_constructed();
        }
    }

    impl WidgetImpl for PartitionUsageItem {}

    impl ListBoxRowImpl for PartitionUsageItem {}
}

glib::wrapper! {
    pub struct PartitionUsageItem(ObjectSubclass<imp::PartitionUsageItem>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable, gtk::Actionable;
}

impl PartitionUsageItem {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();

        this
    }

    pub fn from_part_info(info: &PartitionInfo) -> PartitionUsageItem {
        let out = Self::new();

        out.imp().devname_label.set_text(&info.devname);

        out.update(info);

        out
    }

    pub fn update(&self, info: &PartitionInfo) {
        let imp = self.imp();
        let mount_points_box = &imp.mount_points_box;

        let mut cache = imp.cache.borrow_mut();

        if cache.mount_points != info.mountpoints {
            let _ = std::mem::replace(&mut cache.mount_points, info.mountpoints.clone());

            while let Some(child) = mount_points_box.first_child() {
                child.unparent();
            }

            let mut tooltip_text = String::new();
            for (i, mount_point) in info.mountpoints.iter().enumerate() {
                if i >= 3 {
                    if i == 3 {
                        tooltip_text.push_str(&ni18n(
                            "Additional mount point:",
                            "Additional mount points:",
                            (info.mountpoints.len() - 3) as u32,
                        ));
                    }

                    tooltip_text.push_str(&format!("\n    {}", mount_point));
                    continue;
                }

                let icon = gtk::Image::from_icon_name("mount-point");
                let label = gtk::Label::new(Some(mount_point));
                label.set_halign(gtk::Align::Start);
                label.set_ellipsize(pango::EllipsizeMode::Middle);
                label.set_hexpand(true);

                let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
                row.append(&icon);
                row.append(&label);

                mount_points_box.append(&row);
            }

            if !tooltip_text.is_empty() {
                mount_points_box.set_has_tooltip(true);
                mount_points_box.set_tooltip_text(Some(&tooltip_text));
            } else {
                mount_points_box.set_has_tooltip(false);
                mount_points_box.set_tooltip_text(None);
            }
        }

        if cache.filesystem.as_deref() != info.filesystem.as_deref() {
            let _ = std::mem::replace(&mut cache.filesystem, info.filesystem.clone());
            imp.filesystem_type
                .set_text(&info.filesystem.as_deref().unwrap_or(&i18n("Unknown")));
        }

        let mut part_usage_changed = false;

        if cache.part_used != info.used {
            let _ = std::mem::replace(&mut cache.part_used, info.used);

            part_usage_changed = true;

            if let Some(used) = info.used {
                imp.used_amount.set_visible(true);
                imp.used_amount.set_text(&crate::to_human_readable_nice(
                    used as f32,
                    &crate::DataType::DriveBytes,
                ));
            } else {
                imp.used_amount.set_visible(false);
            }
        }

        if cache.part_size != info.size {
            let _ = std::mem::replace(&mut cache.part_size, info.size);

            part_usage_changed = true;

            if let Some(size) = info.size {
                imp.total_amount.set_visible(true);
                imp.total_amount.set_text(&crate::to_human_readable_nice(
                    size as f32,
                    &crate::DataType::DriveBytes,
                ));
            } else {
                imp.total_amount.set_visible(false);
            }
        }

        if part_usage_changed {
            match (info.size, info.used) {
                (Some(0), _) => {
                    g_warning!(
                        "MissionCenter::PartitionUsageItem",
                        "Partition {} has size 0, cannot calculate usage percentage",
                        info.devname
                    );
                }
                (Some(size), Some(used)) => {
                    let pct = ((used as f64) / (size as f64)).clamp(0., 1.);

                    imp.usage_bar.set_fraction(pct);
                    imp.usage_pct.set_text(&format!("{:.0}%", pct * 100.));
                }
                _ => {}
            }
        }
    }

    pub fn partition_size(&self) -> Option<u64> {
        self.imp().cache.borrow().part_size
    }
}
