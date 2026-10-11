/* src/services.rs
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

use std::num::NonZeroU32;
use std::time::Instant;

use prost::Message;

use magpie_platform::ipc::{Response, ResponseBody};
use magpie_platform::services::{
    ServiceList, ServiceResponseKind, ServicesRequestKind, ServicesResponse, ServicesResponseError,
    ServicesResponseErrorKind,
};

use crate::{data_cache, nng};

pub fn handle_request(kind: Option<ServicesRequestKind>) -> nng::Buffer {
    let start = Instant::now();

    let cache = data_cache();

    let response = match kind {
        Some(ServicesRequestKind::UserServiceList(_)) => {
            let services = cache.user_services();
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Services(ServiceList { services })),
                })),
            }
        }
        Some(ServicesRequestKind::SystemServiceList(_)) => {
            let services = cache.system_services();
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Services(ServiceList { services })),
                })),
            }
        }
        Some(ServicesRequestKind::ServiceLogs(mut req)) => {
            let id = std::mem::take(&mut req.service_id);
            let pid = req.pid.and_then(NonZeroU32::new);

            let logs = cache.service_logs(id, pid);
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Logs(logs.unwrap_or_default())),
                })),
            }
        }
        Some(ServicesRequestKind::ServiceStart(mut id)) => {
            let id = std::mem::take(&mut id.service_id);

            data_cache().service_start(id);
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Empty(Default::default())),
                })),
            }
        }
        Some(ServicesRequestKind::ServiceStop(mut id)) => {
            let id = std::mem::take(&mut id.service_id);

            data_cache().service_stop(id);
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Empty(Default::default())),
                })),
            }
        }
        Some(ServicesRequestKind::ServiceRestart(mut id)) => {
            let id = std::mem::take(&mut id.service_id);

            data_cache().service_restart(id);
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Empty(Default::default())),
                })),
            }
        }
        Some(ServicesRequestKind::ServiceEnable(mut id)) => {
            let id = std::mem::take(&mut id.service_id);

            data_cache().service_enable(id);
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Empty(Default::default())),
                })),
            }
        }
        Some(ServicesRequestKind::ServiceDisable(mut id)) => {
            let id = std::mem::take(&mut id.service_id);

            data_cache().service_disable(id);
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Empty(Default::default())),
                })),
            }
        }
        None => {
            log::warn!("Received empty services request");
            Response {
                body: Some(ResponseBody::Services(ServicesResponse {
                    response: Some(ServiceResponseKind::Error(ServicesResponseError {
                        error: Some(ServicesResponseErrorKind::EmptyRequest(Default::default())),
                    })),
                })),
            }
        }
    };

    response.encoded_len();
    let mut buffer = nng::Buffer::new(response.encoded_len());
    response.encode_raw(&mut buffer);

    log::debug!(
        "PERF: Services loaded and serialized in {:?}",
        start.elapsed()
    );

    cache.refresh_services_async();

    buffer
}
