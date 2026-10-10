/* src/animation.rs
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

/// Per-frame animation snapshot delivered to graph widgets.
#[derive(Clone, Copy, Debug)]
pub struct AnimationFrame {
    /// Animation progress within the current refresh cycle, in [0.0, 1.0].
    /// A value of 0.0 also signals the start of a new cycle
    pub progress: f32,
    /// Window-global gridline phase counter; consumed only when progress == 0.0.
    pub grid_offset: u32,
}
