/* widgets/list_cell.rs
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
use std::rc::Rc;

use adw::{self, subclass::prelude::*};
use glib::{g_critical, g_warning, ParamSpec, Properties, Value, Variant};
use gtk::glib;
use gtk::graphene;
use gtk::prelude::*;

#[allow(unreachable_code)]
mod imp {
    use super::*;

    #[derive(Properties)]
    #[properties(wrapper_type = super::ListCell)]
    pub struct ListCell {
        #[property(set = Self::set_item_id, type = glib::GString)]
        item_id: RefCell<Rc<str>>,
        #[property(set = Self::set_action_name, type = glib::GString)]
        action_name: RefCell<Rc<str>>,
        #[property(set)]
        is_tree_view: Cell<bool>,

        gestures_installed: Cell<bool>,
    }

    impl Default for ListCell {
        fn default() -> Self {
            let empty_str = Rc::<str>::from("");
            Self {
                item_id: RefCell::new(empty_str.clone()),
                action_name: RefCell::new(empty_str),
                is_tree_view: Cell::new(false),
                gestures_installed: Cell::new(false),
            }
        }
    }

    impl ListCell {
        fn set_item_id(&self, item_name: &str) {
            *self.item_id.borrow_mut() = Rc::<str>::from(item_name);
        }

        fn set_action_name(&self, action_name: &str) {
            *self.action_name.borrow_mut() = Rc::<str>::from(action_name);
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ListCell {
        const NAME: &'static str = "ListCell";
        type Type = super::ListCell;
        type ParentType = adw::Bin;

        fn class_init(_klass: &mut Self::Class) {}

        fn instance_init(_obj: &glib::subclass::InitializingObject<Self>) {}
    }

    impl ObjectImpl for ListCell {
        fn properties() -> &'static [ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
            self.derived_set_property(id, value, pspec)
        }

        fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
            self.derived_property(id, pspec)
        }
    }

    impl WidgetImpl for ListCell {
        fn realize(&self) {
            self.parent_realize();

            if self.gestures_installed.get() {
                return;
            }

            let this = self.obj();
            if let Some(mut row_widget) = this.parent().and_then(|p| p.parent()) {
                if self.is_tree_view.get() {
                    if let Some(rw) = row_widget.parent() {
                        row_widget = rw;
                    }
                }

                let gesture_handler = {
                    let this = this.downgrade();
                    move |widget: &gtk::Widget, x: f64, y: f64| {
                        let Some(this) = this.upgrade() else {
                            return;
                        };
                        let this = this.imp();

                        let Some(root) = widget.root() else {
                            return;
                        };

                        // The anchor rect is passed in root coordinates; the action handler
                        // translates it into the menu parent's coordinate space.
                        let (anchor_x, anchor_y, anchor_w, anchor_h) = if x > 0. && y > 0. {
                            match widget.compute_point(&root, &graphene::Point::new(x as _, y as _))
                            {
                                Some(p) => (p.x() as f64, p.y() as f64, 1., 1.),
                                None => {
                                    g_critical!(
                                        "MissionCenter::ListCell",
                                        "Failed to compute_point, context menu will not be anchored to mouse position"
                                    );
                                    (x, y, 1., 1.)
                                }
                            }
                        } else {
                            match widget.compute_bounds(&root) {
                                Some(bounds) => (
                                    bounds.x() as f64,
                                    bounds.y() as f64,
                                    bounds.width() as f64,
                                    bounds.height() as f64,
                                ),
                                None => {
                                    g_warning!(
                                        "MissionCenter::ListCell",
                                        "Failed to get bounds for row widget, popup will display in an arbitrary location"
                                    );
                                    (0., 0., 0., 0.)
                                }
                            }
                        };

                        let item_name = this.item_id.borrow().as_ref().to_owned();
                        let _ = this.obj().activate_action(
                            this.action_name.borrow().as_ref(),
                            Some(&Variant::from((
                                item_name, anchor_x, anchor_y, anchor_w, anchor_h,
                            ))),
                        );
                    }
                };

                let gesture_click = gtk::GestureClick::new();
                gesture_click.set_button(3);
                gesture_click.connect_released({
                    let gesture_handler = gesture_handler.clone();
                    move |gesture, _, x, y| {
                        if let Some(widget) = gesture.widget() {
                            gesture_handler(&widget, x, y);
                        }
                    }
                });

                let gesture_touch = gtk::GestureLongPress::new();
                gesture_touch.set_button(1);
                gesture_touch.set_touch_only(true);
                gesture_touch.connect_pressed(move |gesture, x, y| {
                    if let Some(widget) = gesture.widget() {
                        gesture_handler(&widget, x, y);
                    }
                });

                row_widget.add_controller(gesture_click);
                row_widget.add_controller(gesture_touch);

                self.gestures_installed.set(true);
            }
        }
    }

    impl BinImpl for ListCell {}
}

glib::wrapper! {
    pub struct ListCell(ObjectSubclass<imp::ListCell>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ListCell {
    pub fn new(action_name: &str) -> Self {
        glib::Object::builder()
            .property("action-name", action_name)
            .build()
    }
}
