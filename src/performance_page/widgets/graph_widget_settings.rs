/* performance_page/widgets/graph_widget_settings.rs
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

use gtk::gio;
use gtk::prelude::*;

use mc_graph_widget::GraphWidget;

pub trait GraphWidgetSettingsExt {
    fn connect_to_settings(&self, settings: &gio::Settings);
    fn connect_to_smooth_settings(&self, settings: &gio::Settings);
}

impl GraphWidgetSettingsExt for GraphWidget {
    fn connect_to_settings(&self, settings: &gio::Settings) {
        bind(self, settings, "performance-page-data-points", |w, s, k| {
            w.set_data_points(s.int(k) as u32);
        });
        bind(self, settings, "performance-smooth-graphs", |w, s, k| {
            w.set_smooth_graphs(s.boolean(k));
        });
        bind(self, settings, "performance-sliding-graphs", |w, s, k| {
            w.set_do_animation(s.boolean(k));
        });
    }

    fn connect_to_smooth_settings(&self, settings: &gio::Settings) {
        bind(self, settings, "performance-sliding-graphs", |w, s, k| {
            w.set_do_animation(s.boolean(k));
        });
    }
}

fn bind<F>(widget: &GraphWidget, settings: &gio::Settings, key: &'static str, apply: F)
where
    F: Fn(&GraphWidget, &gio::Settings, &str) + 'static,
{
    apply(widget, settings, key);

    let weak = widget.downgrade();
    settings.connect_changed(Some(key), move |settings, _| {
        if let Some(widget) = weak.upgrade() {
            apply(&widget, settings, key);
        }
    });
}
