/* src/services/openrc/service.rs
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

#[derive(Debug, Clone, Eq, PartialEq)]
#[repr(C)]
pub enum State {
    Stopped = 0x0001,
    Started = 0x0002,
    Stopping = 0x0004,
    Starting = 0x0008,
    Inactive = 0x0010,

    /* Service may or may not have been hotplugged */
    Hotplugged = 0x0100,

    /* Optional states service could also be in */
    Failed = 0x0200,
    Scheduled = 0x0400,
    Wasinactive = 0x0800,
    Crashed = 0x1000,
}

impl From<State> for u32 {
    fn from(state: State) -> u32 {
        state as u32
    }
}
