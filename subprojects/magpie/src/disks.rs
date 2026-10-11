/* src/disks.rs
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

use magpie_platform::disks::{
    DiskList, DiskOptionalSmartData, DisksRequestKind, DisksResponse, DisksResponseError,
    DisksResponseErrorKind, DisksResponseKind,
};
use magpie_platform::ipc::{Response, ResponseBody};

use crate::{data_cache, nng};

pub fn handle_request(kind: Option<DisksRequestKind>) -> nng::Buffer {
    let start = Instant::now();

    let cache = data_cache();

    let dbg_request_name;

    let response = match kind {
        Some(DisksRequestKind::Disks(..)) => {
            dbg_request_name = "Disks";
            let disks = cache.disks();
            Response {
                body: Some(ResponseBody::Disks(DisksResponse {
                    response: Some(DisksResponseKind::Disks(DiskList { disks })),
                })),
            }
        }
        Some(DisksRequestKind::Eject(req)) => {
            dbg_request_name = "Eject";
            let eject_result = cache.eject_disk(&req.id);
            match eject_result {
                Ok(_) => Response {
                    body: Some(ResponseBody::Disks(DisksResponse {
                        response: Some(DisksResponseKind::Eject(Default::default())),
                    })),
                },
                Err(e) => Response {
                    body: Some(ResponseBody::Disks(DisksResponse {
                        response: Some(DisksResponseKind::Error(DisksResponseError {
                            error: Some(DisksResponseErrorKind::Eject(e)),
                        })),
                    })),
                },
            }
        }
        Some(DisksRequestKind::Smart(req)) => {
            dbg_request_name = "Smart";
            let smart_data = cache.smart_data(&req.id);
            Response {
                body: Some(ResponseBody::Disks(DisksResponse {
                    response: Some(DisksResponseKind::Smart(DiskOptionalSmartData {
                        smart: smart_data,
                    })),
                })),
            }
        }
        _ => {
            dbg_request_name = "Unknown";
            log::error!("Invalid or empty request: {:?}", kind);
            Response {
                body: Some(ResponseBody::Disks(DisksResponse {
                    response: Some(DisksResponseKind::Error(DisksResponseError {
                        error: Some(DisksResponseErrorKind::UnknownRequest(Default::default())),
                    })),
                })),
            }
        }
    };

    let mut buffer = nng::Buffer::new(response.encoded_len());
    response.encode_raw(&mut buffer);

    log::debug!(
        "PERF: {dbg_request_name} (disks) loaded and serialized in {:?}",
        start.elapsed()
    );

    cache.refresh_disks_async();

    buffer
}
