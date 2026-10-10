/* performance_page/widgets/mem_composition_widget.rs
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

use std::cell::Cell;

use glib::{ParamSpec, Properties, Value};
use gtk::prelude::WidgetExt;
use gtk::{
    gdk,
    gdk::{gdk_pixbuf, prelude::*},
    glib, graphene,
    gsk::{self, FillRule, PathBuilder, Stroke},
    prelude::*,
    subclass::prelude::*,
    Snapshot,
};

use magpie_types::memory::Memory;

use crate::i18n::i18n_f;

const GRAPH_RADIUS: f32 = 7.;
const FILL_ALPHA: f32 = 50. / 255.;

mod imp {
    use super::*;
    use crate::DataType;

    #[derive(Properties)]
    #[properties(wrapper_type = super::MemoryCompositionWidget)]
    pub struct MemoryCompositionWidget {
        #[property(get, set)]
        base_color: Cell<gdk::RGBA>,
        #[property(get, set)]
        range_min: Cell<f32>,
        #[property(get, set)]
        range_max: Cell<f32>,

        pub(crate) mem_info: Cell<Memory>,

        tooltip_texts: Cell<Vec<(f32, String)>>,
    }

    impl Default for MemoryCompositionWidget {
        fn default() -> Self {
            Self {
                base_color: Cell::new(gdk::RGBA::new(0., 0., 0., 1.)),
                range_min: Cell::new(0.0),
                range_max: Cell::new(100.0),

                mem_info: Cell::new(Memory::default()),

                tooltip_texts: Cell::new(vec![]),
            }
        }
    }

    impl MemoryCompositionWidget {
        #[inline]
        fn generate_pattern(&self, scale_factor: f32, color: gdk::RGBA) -> gdk_pixbuf::Pixbuf {
            let pixel_size = scale_factor.trunc() as i32;
            let pattern_width = pixel_size * 2;
            let pattern_height = pixel_size * 2;

            let mut pattern_data = Vec::with_capacity((pattern_width * pattern_height) as usize);
            for _ in 0..pixel_size {
                for _ in 0..pixel_size {
                    pattern_data.push([
                        (color.red() * 255.) as u8,
                        (color.green() * 255.) as u8,
                        (color.blue() * 255.) as u8,
                        50,
                    ]);
                }
                for _ in 0..pixel_size {
                    pattern_data.push([0, 0, 0, 0]);
                }
            }
            for _ in 0..pixel_size {
                for _ in 0..pixel_size {
                    pattern_data.push([0, 0, 0, 0]);
                }
                for _ in 0..pixel_size {
                    pattern_data.push([
                        (color.red() * 255.) as u8,
                        (color.green() * 255.) as u8,
                        (color.blue() * 255.) as u8,
                        50,
                    ]);
                }
            }

            gdk_pixbuf::Pixbuf::from_bytes(
                &glib::Bytes::from(pattern_data.as_flattened()),
                gdk_pixbuf::Colorspace::Rgb,
                true,
                8,
                pattern_width,
                pattern_height,
                pattern_width * 4,
            )
        }

        #[inline]
        fn render_bar(
            &self,
            snapshot: &Snapshot,
            x: f32,
            width: f32,
            height: f32,
            stroke: &Stroke,
            stroke_color: &gdk::RGBA,
            fill: Option<&gdk::RGBA>,
        ) -> f32 {
            if let Some(fill) = fill {
                let path_builder = PathBuilder::new();
                path_builder.move_to(x, 0.);
                path_builder.line_to(x + width, 0.);
                path_builder.line_to(x + width, height);
                path_builder.line_to(x, height);
                path_builder.close();

                snapshot.append_fill(&path_builder.to_path(), FillRule::Winding, fill);
            }

            let path_builder = PathBuilder::new();
            path_builder.move_to(x, 0.);
            path_builder.line_to(x, height);
            let path = path_builder.to_path();

            snapshot.append_stroke(&path, &stroke, stroke_color);

            x + width
        }

        #[inline]
        fn draw_outline(&self, snapshot: &Snapshot, bounds: &gsk::RoundedRect, color: &gdk::RGBA) {
            let stroke_color = gdk::RGBA::new(color.red(), color.green(), color.blue(), 1.);
            snapshot.append_border(&bounds, &[1.; 4], &[stroke_color.clone(); 4]);
        }

        fn configure_tooltips(&self) {
            let this = self.obj();
            let this = this.upcast_ref::<super::MemoryCompositionWidget>();

            this.set_has_tooltip(true);
            this.set_tooltip_text(Some("Memory Composition"));
            this.connect_query_tooltip(|this, x, _, keyboard, tooltip| {
                if keyboard {
                    tooltip.set_text(Some("Memory Composition"));
                    return true;
                }

                let tooltip_texts = this.imp().tooltip_texts.take();

                let x = x as f32;
                let mut text_pos = tooltip_texts.len() - 1;
                for (i, (pos, _)) in tooltip_texts.iter().enumerate().rev() {
                    if x <= *pos {
                        text_pos = i;
                    } else {
                        break;
                    }
                }

                tooltip.set_text(Some(&tooltip_texts[text_pos].1));
                this.imp().tooltip_texts.set(tooltip_texts);

                true
            });
        }

        fn render(&self, snapshot: &Snapshot, width: f32, height: f32, scale_factor: f64) {
            #[allow(deprecated)]
            let texture = gdk::Texture::for_pixbuf(
                &self.generate_pattern(scale_factor as f32, self.base_color.get()),
            );

            let radius = graphene::Size::new(GRAPH_RADIUS, GRAPH_RADIUS);
            let bounds = gsk::RoundedRect::new(
                graphene::Rect::new(0., 0., width, height),
                radius,
                radius,
                radius,
                radius,
            );

            SnapshotExt::push_rounded_clip(snapshot, &bounds);

            let mem_info = self.mem_info.get();

            let total = if mem_info.mem_total > 0 {
                mem_info.mem_total
            } else {
                1
            };

            let base_color = self.base_color.get();
            let fill_color = gdk::RGBA::new(
                base_color.red(),
                base_color.green(),
                base_color.blue(),
                FILL_ALPHA,
            );

            let stroke = Stroke::new(1.);
            let stroke_color =
                gdk::RGBA::new(base_color.red(), base_color.green(), base_color.blue(), 1.);

            let mut tooltip_texts = self.tooltip_texts.take();
            tooltip_texts.clear();

            // Used memory
            // https://gitlab.com/procps-ng/procps/-/blob/master/library/meminfo.c?ref_type=heads#L736
            let mem_avail = if mem_info.mem_available > mem_info.mem_total {
                mem_info.mem_free
            } else {
                mem_info.mem_available
            };
            let used = total.saturating_sub(mem_avail);
            let x = self.render_bar(
                snapshot,
                0.,
                (width * (used as f32 / total as f32)).trunc(),
                height,
                &stroke,
                &stroke_color,
                Some(&fill_color),
            );

            let used_hr = crate::to_human_readable_nice(used as _, &DataType::MemoryBytes);
            tooltip_texts.push((
                x,
                i18n_f(
                    "In use ({})\n\nMemory used by the operating system and running applications",
                    &[&used_hr],
                ),
            ));

            // Dirty memory
            let modified = mem_info.dirty as f32;
            let bar_width = (width * (modified / total as f32)).trunc();
            let new_x = self.render_bar(
                snapshot,
                x.trunc(),
                bar_width,
                height,
                &stroke,
                &stroke_color,
                Some(&fill_color),
            );
            snapshot.push_repeat(&graphene::Rect::new(x, 0., bar_width, height), None);
            snapshot.append_texture(
                &texture,
                &graphene::Rect::new(0., 0., texture.width() as f32, texture.height() as f32),
            );
            snapshot.pop();

            let x = new_x;

            let modified_hr = crate::to_human_readable_nice(modified, &DataType::MemoryBytes);
            tooltip_texts.push((
                x,
                i18n_f(
                    "Modified ({})\n\nMemory whose contents must be written to disk before it can be used by another process",
                    &[&modified_hr],
                )
            ));

            // Stand-by memory
            let standby = total.saturating_sub(used + mem_info.mem_free);
            let bar_width = (width * (standby as f32 / total as f32)).trunc();

            let new_x =
                self.render_bar(snapshot, x, bar_width, height, &stroke, &stroke_color, None);
            snapshot.push_repeat(&graphene::Rect::new(x, 0., bar_width, height), None);
            snapshot.append_texture(
                &texture,
                &graphene::Rect::new(0., 0., texture.width() as f32, texture.height() as f32),
            );
            snapshot.pop();

            let x = new_x;

            let standby_hr = crate::to_human_readable_nice(standby as _, &DataType::MemoryBytes);
            tooltip_texts.push((
                x,
                i18n_f(
                    "Standby ({})\n\nMemory that contains cached data and code that is not actively in use",
                    &[&standby_hr],
                )
            ));

            // // Free memory
            let free = mem_info.mem_free as f32;
            self.render_bar(snapshot, x, 1., height, &stroke, &stroke_color, None);

            let free_hr = crate::to_human_readable_nice(free, &DataType::MemoryBytes);
            tooltip_texts.push((
                width + 1.,
                i18n_f(
                    "Free ({})\n\nMemory that is not currently in use, and that will be repurposed first when the operating system, drivers, or applications need more memory",
                    &[&free_hr],
                ),
            ));

            SnapshotExt::pop(snapshot);

            self.draw_outline(snapshot, &bounds, &stroke_color);

            self.tooltip_texts.set(tooltip_texts);
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MemoryCompositionWidget {
        const NAME: &'static str = "MemoryCompositionWidget";
        type Type = super::MemoryCompositionWidget;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for MemoryCompositionWidget {
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

            self.configure_tooltips();
        }
    }

    impl WidgetImpl for MemoryCompositionWidget {
        fn realize(&self) {
            self.parent_realize();
        }

        fn snapshot(&self, snapshot: &Snapshot) {
            use glib::g_critical;

            let this = self.obj();

            let native = match this.native() {
                Some(native) => native,
                None => {
                    g_critical!(
                        "MissionCenter::MemoryCompositionWidget",
                        "Failed to get native"
                    );
                    return;
                }
            };

            let surface = match native.surface() {
                Some(surface) => surface,
                None => {
                    g_critical!(
                        "MissionCenter::MemoryCompositionWidget",
                        "Failed to get surface"
                    );
                    return;
                }
            };

            self.render(
                snapshot,
                this.width() as f32,
                this.height() as f32,
                surface.scale(),
            );
        }
    }
}

glib::wrapper! {
    pub struct MemoryCompositionWidget(ObjectSubclass<imp::MemoryCompositionWidget>)
        @extends gtk::Widget,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl MemoryCompositionWidget {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn update_memory_information(&self, mem_info: &Memory) {
        self.imp().mem_info.set(mem_info.clone());
        self.queue_draw();
    }
}
