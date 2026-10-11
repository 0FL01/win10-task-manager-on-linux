/* src/processes.rs
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

use magpie_platform::ipc::{Response, ResponseBody};
use magpie_platform::processes::{
    ProcessMap, ProcessesRequestKind, ProcessesResponse, ProcessesResponseError,
    ProcessesResponseKind,
};
use prost::Message;

use crate::{data_cache, nng};

pub fn handle_request(kind: Option<ProcessesRequestKind>) -> nng::Buffer {
    let start = Instant::now();

    let cache = data_cache();

    let dbg_request_name;

    let response = match kind {
        Some(ProcessesRequestKind::ProcessMap(..)) => {
            dbg_request_name = "ProcessMap";
            let processes = cache.processes();
            let network_status = cache.processes_network_status();
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::Processes(ProcessMap {
                        processes,
                        network_stats_error: network_status,
                    })),
                })),
            }
        }
        Some(ProcessesRequestKind::Terminate(mut req)) => {
            dbg_request_name = "Terminate";
            cache.terminate_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        Some(ProcessesRequestKind::Kill(mut req)) => {
            dbg_request_name = "Kill";
            cache.kill_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        Some(ProcessesRequestKind::Interrupt(mut req)) => {
            dbg_request_name = "Interrupt";
            cache.interrupt_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        Some(ProcessesRequestKind::User1(mut req)) => {
            dbg_request_name = "User1";
            cache.signal_user_one_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        Some(ProcessesRequestKind::User2(mut req)) => {
            dbg_request_name = "User2";
            cache.signal_user_two_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        Some(ProcessesRequestKind::Hangup(mut req)) => {
            dbg_request_name = "Hangup";
            cache.hangup_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        Some(ProcessesRequestKind::Continue(mut req)) => {
            dbg_request_name = "Continue";
            cache.continue_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        Some(ProcessesRequestKind::Suspend(mut req)) => {
            dbg_request_name = "Suspend";
            cache.suspend_processes(std::mem::take(&mut req.pids));
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::TermKill(Default::default())),
                })),
            }
        }
        _ => {
            dbg_request_name = "Unknown";
            log::error!("Invalid or empty request: {:?}", kind);
            Response {
                body: Some(ResponseBody::Processes(ProcessesResponse {
                    response: Some(ProcessesResponseKind::Error(ProcessesResponseError {})),
                })),
            }
        }
    };

    let mut buffer = nng::Buffer::new(response.encoded_len());
    response.encode_raw(&mut buffer);

    log::debug!(
        "PERF: {dbg_request_name} (processes) loaded and serialized in {:?}",
        start.elapsed()
    );

    cache.refresh_processes_async();

    buffer
}
