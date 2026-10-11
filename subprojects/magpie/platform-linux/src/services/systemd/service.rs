/* src/services/systemd/service.rs
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

use zbus::proxy::MethodFlags;
use zbus::zvariant::{Array, DynamicTuple, OwnedObjectPath, Signature};
use zbus::Proxy;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LoadState {
    Loaded,
    Error,
    Masked,
    NotFound,
    Unknown,
}

impl<'a> From<&'a str> for LoadState {
    fn from(value: &'a str) -> Self {
        match value {
            "loaded" => LoadState::Loaded,
            "error" => LoadState::Error,
            "masked" => LoadState::Masked,
            "not-found" => LoadState::NotFound,
            _ => LoadState::Unknown,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ActiveState {
    Active,
    Reloading,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Unknown,
}

impl<'a> From<&'a str> for ActiveState {
    fn from(value: &'a str) -> Self {
        match value {
            "active" => ActiveState::Active,
            "reloading" => ActiveState::Reloading,
            "inactive" => ActiveState::Inactive,
            "failed" => ActiveState::Failed,
            "activating" => ActiveState::Activating,
            "deactivating" => ActiveState::Deactivating,
            _ => ActiveState::Unknown,
        }
    }
}

pub async fn start(
    proxy: &Proxy<'static>,
    name: &str,
    mode: &str,
) -> zbus::Result<Option<OwnedObjectPath>> {
    let reply = proxy
        .call_with_flags(
            "StartUnit",
            MethodFlags::AllowInteractiveAuth.into(),
            &DynamicTuple((name, mode)),
        )
        .await?;
    Ok(reply)
}

pub async fn stop(
    proxy: &Proxy<'static>,
    name: &str,
    mode: &str,
) -> zbus::Result<Option<OwnedObjectPath>> {
    let reply = proxy
        .call_with_flags(
            "StopUnit",
            MethodFlags::AllowInteractiveAuth.into(),
            &DynamicTuple((name, mode)),
        )
        .await?;
    Ok(reply)
}

pub async fn restart(
    proxy: &Proxy<'static>,
    name: &str,
    mode: &str,
) -> zbus::Result<Option<OwnedObjectPath>> {
    let reply = proxy
        .call_with_flags(
            "RestartUnit",
            MethodFlags::AllowInteractiveAuth.into(),
            &DynamicTuple((name, mode)),
        )
        .await?;
    Ok(reply)
}

pub async fn enable(
    proxy: &Proxy<'static>,
    name: &str,
) -> zbus::Result<Option<(bool, Vec<(String, String, String)>)>> {
    let mut files = Array::new(&Signature::Str);
    files.append(name.to_owned().into())?;

    let reply = proxy
        .call_with_flags(
            "EnableUnitFiles",
            MethodFlags::AllowInteractiveAuth.into(),
            &DynamicTuple((files, false, true)),
        )
        .await?;
    Ok(reply)
}

pub async fn disable(
    proxy: &Proxy<'static>,
    name: &str,
) -> zbus::Result<Option<Vec<(String, String, String)>>> {
    let mut files = Array::new(&Signature::Str);
    files.append(name.to_owned().into())?;

    let reply = proxy
        .call_with_flags(
            "DisableUnitFiles",
            MethodFlags::AllowInteractiveAuth.into(),
            &DynamicTuple((files, false)),
        )
        .await?;
    Ok(reply)
}
