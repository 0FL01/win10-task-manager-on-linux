/* src/network.rs
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

use std::time::Instant;

use prost::Message;

use magpie_platform::ipc::{Response, ResponseBody};
use magpie_platform::network::{ConnectionList, ConnectionResponseKind, ConnectionsResponse};

use crate::{data_cache, nng};

pub fn handle_request() -> nng::Buffer {
    let start = Instant::now();

    let cache = data_cache();
    let connections = cache.connections();

    let response = Response {
        body: Some(ResponseBody::Connections(ConnectionsResponse {
            response: Some(ConnectionResponseKind::Connections(ConnectionList {
                connections,
            })),
        })),
    };
    response.encoded_len();
    let mut buffer = nng::Buffer::new(response.encoded_len());
    response.encode_raw(&mut buffer);

    log::debug!(
        "PERF: Network connections loaded and serialized in {:?}",
        start.elapsed()
    );

    cache.refresh_network_async();

    buffer
}
