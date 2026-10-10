/* widgets/placeholder.rs
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

use std::cell::RefCell;

use adw::subclass::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::{glib, prelude::*};

mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::Placeholder)]
    pub struct Placeholder {
        #[property(get, set = Self::set_profile)]
        profile: RefCell<String>,
    }

    impl Default for Placeholder {
        fn default() -> Self {
            Self {
                profile: RefCell::new(String::new()),
            }
        }
    }

    impl Placeholder {
        fn set_profile(&self, profile: String) {
            *self.profile.borrow_mut() = profile;
            self.rebuild();
        }

        fn rebuild(&self) {
            let obj = self.obj();

            while let Some(child) = obj.first_child() {
                child.unparent();
            }

            match self.profile.borrow().as_str() {
                "sidebar" => build_sidebar(&obj),
                "performance-page-graph" => build_performance_page_graph(&obj),
                "performance-page-info" => build_performance_page_info(&obj),
                "apps-page" => build_apps_page(&obj),
                "services-page" => build_services_page(&obj),
                _ => {}
            }
        }
    }

    fn component(width: i32, height: i32) -> super::ShimmerBlock {
        let component = super::ShimmerBlock::new(width, height);
        component.add_css_class("placeholder-shimmer");
        component.set_halign(gtk::Align::Start);
        component.set_valign(gtk::Align::Center);
        component
    }

    fn build_sidebar(obj: &super::Placeholder) {
        obj.set_orientation(gtk::Orientation::Vertical);
        obj.set_spacing(0);

        obj.set_margin_start(0);
        obj.set_margin_end(0);
        obj.set_margin_top(0);
        obj.set_margin_bottom(0);

        for _ in 0..5 {
            let summary_graph = gtk::Box::default();
            summary_graph.set_orientation(gtk::Orientation::Horizontal);
            summary_graph.set_spacing(10);
            summary_graph.set_margin_start(15);
            summary_graph.set_margin_end(10);
            summary_graph.set_margin_top(15);
            summary_graph.set_margin_bottom(7);

            summary_graph.append(&component(80, 55));

            let lines = gtk::Box::new(gtk::Orientation::Vertical, 10);
            lines.set_valign(gtk::Align::Center);
            lines.append(&component(80, 9));
            lines.append(&component(110, 9));
            lines.append(&component(44, 9));
            summary_graph.append(&lines);

            obj.append(&summary_graph);
        }
    }

    fn build_performance_page_graph(obj: &super::Placeholder) {
        obj.set_orientation(gtk::Orientation::Vertical);
        obj.set_spacing(10);

        obj.set_margin_start(10);
        obj.set_margin_end(10);
        obj.set_margin_top(13);
        obj.set_margin_bottom(10);

        let labels = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        labels.append(&component(120, 30));
        labels.append(&spacer());
        labels.append(&component(200, 20));

        obj.append(&labels);

        let small_labels = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        small_labels.append(&component(80, 9));
        small_labels.append(&spacer());
        small_labels.append(&component(50, 9));

        let graph = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        graph.add_css_class("placeholder-shimmer");
        graph.set_hexpand(true);
        graph.set_vexpand(true);

        let graph_content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        graph_content.append(&small_labels);
        graph_content.append(&graph);

        obj.append(&graph_content);
    }

    fn stat_block(caption_width: i32, value_width: i32) -> gtk::Box {
        let block = gtk::Box::new(gtk::Orientation::Vertical, 3);
        block.set_valign(gtk::Align::Start);
        block.append(&component(caption_width, 9));
        block.append(&component(value_width, 18));
        block
    }

    fn build_performance_page_info(obj: &super::Placeholder) {
        obj.set_orientation(gtk::Orientation::Vertical);
        obj.set_spacing(20);

        obj.set_margin_start(5);
        obj.set_margin_end(10);
        obj.set_margin_top(69);
        obj.set_margin_bottom(10);

        let top_row = gtk::Box::new(gtk::Orientation::Horizontal, 15);
        top_row.append(&stat_block(60, 70));
        top_row.append(&stat_block(45, 90));
        obj.append(&top_row);

        obj.append(&stat_block(75, 110));

        let mid_row = gtk::Box::new(gtk::Orientation::Horizontal, 15);
        mid_row.append(&stat_block(70, 50));
        mid_row.append(&stat_block(55, 50));
        obj.append(&mid_row);

        let details_col1 = gtk::Box::new(gtk::Orientation::Vertical, 10);
        details_col1.append(&component(53, 9));
        details_col1.append(&component(45, 9));
        details_col1.append(&component(71, 9));
        details_col1.append(&component(41, 9));
        details_col1.append(&component(38, 9));
        let details_col2 = gtk::Box::new(gtk::Orientation::Vertical, 10);
        details_col2.append(&component(27, 9));
        details_col2.append(&component(90, 9));
        details_col2.append(&component(64, 9));
        details_col2.append(&component(20, 9));
        details_col2.append(&component(67, 9));

        let details = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        details.set_margin_top(10);
        details.append(&details_col1);
        details.append(&details_col2);

        obj.append(&details);
    }

    fn spacer() -> gtk::Box {
        let spacer = gtk::Box::default();
        spacer.set_hexpand(true);
        spacer
    }

    fn append_table(obj: &super::Placeholder) {
        let table_header = spacer();
        table_header.set_height_request(32);
        table_header.add_css_class("placeholder-shimmer");

        let rows = gtk::Box::default();
        rows.set_vexpand(true);
        rows.set_hexpand(true);
        rows.add_css_class("placeholder-shimmer");

        let table = gtk::Box::new(gtk::Orientation::Vertical, 5);
        table.append(&table_header);
        table.append(&rows);

        obj.append(&table);
    }

    fn build_apps_page(obj: &super::Placeholder) {
        obj.set_orientation(gtk::Orientation::Vertical);
        obj.set_spacing(20);

        obj.set_margin_start(0);
        obj.set_margin_end(0);
        obj.set_margin_top(0);
        obj.set_margin_bottom(0);

        let counts = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let main_count = component(200, 28);
        main_count.set_margin_top(3);
        main_count.set_margin_bottom(10);
        counts.append(&main_count);
        counts.append(&component(110, 9));

        let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        action_row.set_halign(gtk::Align::End);
        action_row.set_valign(gtk::Align::End);
        let button = component(120, 34);
        button.set_margin_end(5);
        action_row.append(&button);
        action_row.append(&component(182, 34));
        action_row.append(&component(87, 34));

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        header.append(&counts);
        header.append(&spacer());
        header.append(&action_row);

        obj.append(&header);
        append_table(obj);
    }

    fn build_services_page(obj: &super::Placeholder) {
        obj.set_orientation(gtk::Orientation::Vertical);
        obj.set_spacing(20);

        obj.set_margin_start(0);
        obj.set_margin_end(0);
        obj.set_margin_top(0);
        obj.set_margin_bottom(0);

        let counts = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let main_count = component(200, 28);
        main_count.set_margin_top(15);
        main_count.set_margin_bottom(5);
        counts.append(&main_count);
        counts.append(&component(110, 9));

        let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        action_row.set_halign(gtk::Align::End);
        let button = component(120, 34);
        button.set_margin_end(5);
        action_row.append(&button);
        action_row.append(&component(145, 34));
        action_row.append(&component(90, 34));
        action_row.append(&component(90, 34));

        let filter_row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        filter_row.append(&component(392, 34));
        filter_row.append(&component(138, 34));

        let controls = gtk::Box::new(gtk::Orientation::Vertical, 12);
        controls.append(&action_row);
        controls.append(&filter_row);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        header.append(&counts);
        header.append(&spacer());
        header.append(&controls);

        obj.append(&header);
        append_table(obj);
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Placeholder {
        const NAME: &'static str = "Placeholder";
        type Type = super::Placeholder;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for Placeholder {
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
        }
    }

    impl WidgetImpl for Placeholder {}

    impl OrientableImpl for Placeholder {}

    impl BoxImpl for Placeholder {}
}

glib::wrapper! {
    pub struct Placeholder(ObjectSubclass<imp::Placeholder>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable, gtk::Orientable;
}

mod shimmer_imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Default)]
    pub struct ShimmerBlock {
        pub(super) width: Cell<i32>,
        pub(super) height: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ShimmerBlock {
        const NAME: &'static str = "ShimmerBlock";
        type Type = super::ShimmerBlock;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ShimmerBlock {}

    impl WidgetImpl for ShimmerBlock {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (0, self.width.get(), -1, -1),
                _ => {
                    // The height represents the thickness of a line/element rather than
                    // something that should reflow, keep it fixed
                    let height = self.height.get();
                    (height, height, -1, -1)
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct ShimmerBlock(ObjectSubclass<shimmer_imp::ShimmerBlock>)
        @extends gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl ShimmerBlock {
    pub fn new(width: i32, height: i32) -> Self {
        let obj: Self = glib::Object::new();
        let imp = obj.imp();
        imp.width.set(width);
        imp.height.set(height);
        obj
    }
}
