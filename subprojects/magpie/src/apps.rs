/* src/apps.rs
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
use std::collections::HashMap;
use std::time::Instant;

use prost::Message;

use magpie_platform::apps::{
    AppIconResponseKind, AppList, AppResponseKind, AppsIconResponse, AppsIconResponseValue,
    AppsResponse,
};
use magpie_platform::ipc::{Response, ResponseBody};

use crate::{data_cache, nng};

pub fn handle_request() -> nng::Buffer {
    let start = Instant::now();

    let cache = data_cache();
    let apps = cache.apps();

    let response = Response {
        body: Some(ResponseBody::Apps(AppsResponse {
            response: Some(AppResponseKind::Apps(AppList { apps })),
        })),
    };
    response.encoded_len();
    let mut buffer = nng::Buffer::new(response.encoded_len());
    response.encode_raw(&mut buffer);

    log::debug!("PERF: Apps loaded and serialized in {:?}", start.elapsed());

    cache.refresh_apps_async();

    buffer
}

pub fn handle_icons_request(mut app_ids: Vec<String>) -> nng::Buffer {
    let start = Instant::now();

    let cache = data_cache();
    let mut apps = cache.app_icons();

    let num_items = apps.len();

    let requested_apps: HashMap<_, _> = app_ids
        .drain(..)
        .filter_map(|app_id| apps.remove(&app_id).map(|ico| (app_id, ico)))
        .collect();

    let response = Response {
        body: Some(ResponseBody::AppIcons(AppsIconResponse {
            response: Some(AppIconResponseKind::Values(AppsIconResponseValue {
                pairs: requested_apps,
            })),
        })),
    };
    response.encoded_len();
    let mut buffer = nng::Buffer::new(response.encoded_len());
    response.encode_raw(&mut buffer);

    log::debug!(
        "PERF: {} App icons loaded and serialized in {:?}",
        num_items,
        start.elapsed()
    );

    buffer
}
