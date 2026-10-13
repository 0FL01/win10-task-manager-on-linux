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
            cached_disks_response(cache.disks_snapshot())
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

fn cached_disks_response(snapshot: DiskList) -> Response {
    Response {
        body: Some(ResponseBody::Disks(DisksResponse {
            response: Some(DisksResponseKind::Disks(snapshot)),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use magpie_platform::disks::{Disk, DiskSmartData};
    use std::collections::HashMap;

    #[test]
    fn disk_dispatch_serializes_the_cached_smart_map_in_the_existing_response() {
        #[derive(Clone, PartialEq, Message)]
        struct LegacyDiskList {
            #[prost(message, repeated, tag = "1")]
            disks: Vec<Disk>,
        }
        let snapshot = DiskList {
            disks: vec![Disk {
                id: "a".into(),
                ..Default::default()
            }],
            smart_data: HashMap::from([("a".into(), DiskSmartData::default())]),
        };
        // An old client still reads inventory and skips the additive tag 2.
        let legacy = LegacyDiskList::decode(snapshot.encode_to_vec().as_slice()).unwrap();
        assert_eq!(legacy.disks, snapshot.disks);
        let response = cached_disks_response(snapshot.clone());
        let decoded = Response::decode(response.encode_to_vec().as_slice()).unwrap();
        let Some(ResponseBody::Disks(response)) = decoded.body else {
            panic!("disk response expected")
        };
        assert_eq!(response.response, Some(DisksResponseKind::Disks(snapshot)));
    }

    #[test]
    fn legacy_disk_list_and_manual_smart_keep_their_wire_tags() {
        // Existing DisksResponse.disks tag 2, containing an empty old DiskList.
        let response = DisksResponse::decode(&[0x12, 0x00][..]).unwrap();
        let Some(DisksResponseKind::Disks(snapshot)) = response.response else {
            panic!("disk response expected")
        };
        assert!(snapshot.disks.is_empty());
        assert!(snapshot.smart_data.is_empty());

        // Existing manual SMART tag 3, OptionalSmartData.smart tag 1.
        let manual =
            DisksResponse::decode(&[0x1a, 0x06, 0x0a, 0x04, 0x08, 0x00, 0x10, 0x00][..]).unwrap();
        assert_eq!(
            manual.response,
            Some(DisksResponseKind::Smart(DiskOptionalSmartData {
                smart: Some(DiskSmartData::default()),
            }))
        );
        assert_eq!(
            manual.encode_to_vec(),
            [0x1a, 0x06, 0x0a, 0x04, 0x08, 0x00, 0x10, 0x00]
        );
    }
}
