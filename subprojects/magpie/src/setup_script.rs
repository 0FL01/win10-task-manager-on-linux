/* src/setup_script.rs
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

use std::time::Instant;

use prost::Message;

use magpie_platform::ipc::{Response, ResponseBody};
use magpie_platform::setup_script::{
    ScriptName, ScriptRequestKind, ScriptResponse, ScriptResponseKind,
};

#[cfg(target_os = "linux")]
use magpie_platform_linux::setup_script::{get_elevation_command, get_file_name, open, run};

use crate::{data_cache, nng};

pub fn handle_request(kind: Option<ScriptRequestKind>) -> nng::Buffer {
    let start = Instant::now();

    let cache = data_cache();

    let response = match kind {
        Some(ScriptRequestKind::Run(_)) => match run(false) {
            Ok(()) => ScriptResponseKind::RunSuccess(Default::default()),
            Err(v) => ScriptResponseKind::Error(v),
        },
        Some(ScriptRequestKind::RunRevert(_)) => match run(true) {
            Ok(()) => ScriptResponseKind::RunSuccess(Default::default()),
            Err(v) => ScriptResponseKind::Error(v),
        },
        Some(ScriptRequestKind::Open(_)) => match open() {
            Ok(()) => ScriptResponseKind::RunSuccess(Default::default()),
            Err(v) => ScriptResponseKind::Error(v),
        },
        Some(ScriptRequestKind::GetName(_)) => match get_file_name() {
            Some(file) => ScriptResponseKind::Name(ScriptName {
                file,
                elevation_command: get_elevation_command(),
            }),
            None => ScriptResponseKind::None(Default::default()),
        },
        None => ScriptResponseKind::Error("Empty Request".to_string()),
    };

    let response = Response {
        body: Some(ResponseBody::Script(ScriptResponse {
            response: Some(response),
        })),
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
