/* src/services/systemd/mod.rs
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
use std::hash::Hasher;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ahash::AHasher;
use futures_lite::StreamExt;
use thiserror::Error;
use zbus::proxy::PropertyStream;
use zbus::zvariant::OwnedObjectPath;
use zbus::{zvariant, Connection, Proxy};

use magpie_platform::services::Service;

use crate::services::systemd::systemd_manager::UnitFilesChangedStream;
use crate::util::{is_snap, user_bus};
use crate::{async_runtime, sync, system_bus};

use service::{ActiveState, LoadState};
use systemd_manager::SystemDManagerProxy;

pub use manager::ServiceManager;

mod manager;
mod service;
mod systemd_manager;

#[derive(Debug, Error)]
pub enum SystemDError {
    #[error("Failed to connect to DBus system bus")]
    DBusConnectionError,
    #[error("Failed to open `libsystemd.so.0`")]
    LibSystemDNotFound,
    #[error("DBus error: {0}")]
    DBusError(#[from] zbus::Error),
    #[error("Library loading error: {0}")]
    LibLoadingError(#[from] libloading::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to open journal: {0}")]
    JournalOpenError(String),
    #[error("Seek failed: {0}")]
    JournalSeekError(String),
    #[error("Failed to add match: {0}")]
    JournalAddMatchError(String),
    #[error("Failed to add disjunction: {0}")]
    JournalAddDisjunctionError(String),
    #[error("Failed to add conjunction: {0}")]
    JournalAddConjunctionError(String),
    #[error("Failed to iterate journal entries: {0}")]
    JournalIterateError(String),
}

async fn stream_has_contents(stream: &mut UnitFilesChangedStream) -> Option<bool> {
    use futures::stream::Stream;
    use std::task::{Context, Poll};

    // Create a dummy context for polling
    let mut stream = std::pin::pin!(stream);

    // Poll the stream for a next value without blocking
    match stream
        .as_mut()
        .poll_next(&mut Context::from_waker(&futures::task::noop_waker()))
    {
        Poll::Ready(Some(_)) => Some(true),
        Poll::Ready(None) => None,    // Stream ended without values
        Poll::Pending => Some(false), // No immediate value available
    }
}

async fn stream_peeker<T>(stream: &mut PropertyStream<'_, T>) -> Option<T>
where
    T: TryFrom<zvariant::OwnedValue>,
    T::Error: Into<zbus::Error>,
    T: Unpin,
{
    use futures::stream::Stream;
    use std::task::{Context, Poll};

    // Create a dummy context for polling
    let mut stream = std::pin::pin!(stream);

    // Poll the stream for a next value without blocking
    match stream
        .as_mut()
        .poll_next(&mut Context::from_waker(&futures::task::noop_waker()))
    {
        Poll::Ready(Some(sig)) => sig.get().await.ok(),
        Poll::Ready(None) => None, // Stream ended without values
        Poll::Pending => None,     // No immediate value available
    }
}

struct ServiceCache {
    pub service: Service,

    pub main_pid_receiver: PropertyStream<'static, u32>,
    pub user_receiver: PropertyStream<'static, String>,
    pub group_receiver: PropertyStream<'static, String>,

    pub active_state_receiver: PropertyStream<'static, String>,
}

impl ServiceCache {
    async fn new(service: Service, unit_path: OwnedObjectPath, bus: &Connection) -> Option<Self> {
        let Ok(unit_proxy) = Proxy::new(
            bus,
            "org.freedesktop.systemd1",
            unit_path.clone(),
            "org.freedesktop.systemd1.Unit",
        )
        .await
        else {
            return None;
        };

        let Ok(service_proxy) = Proxy::new(
            bus,
            "org.freedesktop.systemd1",
            unit_path.clone(),
            "org.freedesktop.systemd1.Service",
        )
        .await
        else {
            return None;
        };

        Some(Self {
            service,

            main_pid_receiver: service_proxy
                .receive_property_changed::<u32>("MainPID")
                .await,
            user_receiver: service_proxy
                .receive_property_changed::<String>("User")
                .await,
            group_receiver: service_proxy
                .receive_property_changed::<String>("Group")
                .await,

            active_state_receiver: unit_proxy
                .receive_property_changed::<String>("ActiveState")
                .await,
        })
    }

    #[inline]
    async fn update_service_properties(&mut self) -> Result<(), SystemDError> {
        let (active_state, pid, user, group) = tokio::join!(
            stream_peeker(&mut self.active_state_receiver),
            stream_peeker(&mut self.main_pid_receiver),
            stream_peeker(&mut self.user_receiver),
            stream_peeker(&mut self.group_receiver),
        );

        if let Some(active_state) = active_state {
            self.set_active(active_state);
        }

        if let Some(pid) = pid {
            self.set_pid(pid);
        }

        if let Some(user) = user {
            self.set_user(user);
        }

        if let Some(group) = group {
            self.set_group(group);
        }

        Ok(())
    }

    fn set_active(&mut self, active_state: String) {
        let active_state: ActiveState = active_state.as_str().into();

        self.service.running = active_state == ActiveState::Active;
        self.service.failed = active_state == ActiveState::Failed;
    }

    fn set_group(&mut self, group: String) {
        self.service.group = if group.is_empty() { None } else { Some(group) }
    }

    fn set_user(&mut self, user: String) {
        self.service.user = if user.is_empty() { None } else { Some(user) }
    }

    fn set_pid(&mut self, pid: u32) {
        self.service.pid = if pid == 0 { None } else { Some(pid) }
    }
}

pub struct SystemD {
    systemd1: SystemDManagerProxy<'static>,
    bus: &'static Connection,
    service_cache: HashMap<u64, ServiceCache>,

    subscribe_ok: bool,
    full_refresh_required: Arc<AtomicBool>,

    to_remove: Vec<u64>,

    unit_files_changed: Option<UnitFilesChangedStream>,

    bus_id: &'static str,
}

impl SystemD {
    pub fn systemd_system() -> Result<Self, SystemDError> {
        match system_bus() {
            Some(bus) => Self::new(bus, "system-bus"),
            None => Err(SystemDError::DBusConnectionError),
        }
    }

    pub fn systemd_user() -> Result<Self, SystemDError> {
        match user_bus() {
            Some(bus) => Self::new(bus, "user-bus"),
            None => Err(SystemDError::DBusConnectionError),
        }
    }

    fn new(bus: &'static Connection, bus_id: &'static str) -> Result<Self, SystemDError> {
        let rt = async_runtime();
        let systemd1 = sync!(rt, async {
            let proxy = SystemDManagerProxy::new(bus).await?;

            // Test the connection by fetching the version
            let version = proxy.version().await?;
            log::debug!("Connected to SystemD {} version: {}", bus_id, version);

            zbus::Result::Ok(proxy)
        })?;

        let full_refresh_required = Arc::new(AtomicBool::new(true));

        let subscribe_ok = if let Err(err) = sync!(rt, systemd1.subscribe()) {
            log::error!("Failed to subscribe to SystemD notifications, service refresh will be significantly more expensive: {err}");
            false
        } else {
            rt.spawn({
                let systemd1 = systemd1.clone();
                let full_refresh_required = full_refresh_required.clone();
                async move {
                    let mut unit_files_changed = match systemd1.receive_unit_files_changed().await {
                        Ok(unit_files_changed) => unit_files_changed,
                        Err(e) => {
                            log::error!("Failed to receive UnitFilesChanged signal: {e}");
                            return;
                        }
                    };

                    let mut reloading = match systemd1.receive_reloading().await {
                        Ok(reloading) => reloading,
                        Err(e) => {
                            log::error!("Failed to receive Reloading signal: {e}");
                            return;
                        }
                    };

                    loop {
                        tokio::select! {
                            _ = unit_files_changed.next() => {
                                full_refresh_required.store(true, Ordering::Release);
                            }
                            _ = reloading.next() => {
                                full_refresh_required.store(true, Ordering::Release);
                            }
                        }
                    }
                }
            });

            true
        };

        let listener = match sync!(rt, systemd1.receive_unit_files_changed()) {
            Ok(sig) => Some(sig),
            Err(e) => {
                log::error!("Failed to receive unit files changed: {e}");
                None
            }
        };

        Ok(Self {
            systemd1,
            bus,
            service_cache: HashMap::new(),

            subscribe_ok,
            full_refresh_required,

            to_remove: Vec::new(),

            unit_files_changed: listener,

            bus_id,
        })
    }

    async fn list_unit_files(&self) -> HashMap<String, (String, bool)> {
        match self.systemd1.list_unit_files().await {
            Ok(unit_files) => {
                let mut result = HashMap::new();

                for (file, state) in unit_files {
                    let Some(file_name) = Path::file_name(Path::new(&file)) else {
                        continue;
                    };

                    result.insert(
                        file_name.to_string_lossy().to_string().replace("@", ""),
                        (file, &state == "enabled"),
                    );
                }

                result
            }
            Err(e) => {
                if is_snap() {
                    log::debug!("Failed to list unit files: {e}. This is expected behavior when running as a Snap.");
                } else {
                    log::error!("Failed to list unit files: {e}");
                }
                Default::default()
            }
        }
    }

    pub fn list_services(&mut self) -> Result<HashMap<u64, Service>, SystemDError> {
        let rt = async_runtime();

        let files_changed = if let Some(ref mut file_change_stream) = self.unit_files_changed {
            sync!(rt, stream_has_contents(file_change_stream))
        } else {
            Some(false)
        }
        .unwrap_or(false);

        let mut unit_files = HashMap::new();

        if self
            .full_refresh_required
            .fetch_and(!self.subscribe_ok, Ordering::Acquire)
            || files_changed
        {
            unit_files = sync!(rt, self.list_unit_files());

            let preprocess_service = |(
                name,
                description,
                load_state,
                active_state,
                _sub_state,
                _following,
                unit_obj_path,
                _job_id,
                _job_type,
                _job_obj_path,
            ): (
                String,
                String,
                String,
                String,
                String,
                String,
                OwnedObjectPath,
                u32,
                String,
                OwnedObjectPath,
            )| {
                let service_id = {
                    let mut hasher = AHasher::default();
                    hasher.write(self.bus_id.as_bytes());
                    hasher.write((*unit_obj_path).as_bytes());
                    hasher.finish()
                };
                let (unit_file_path, enabled) = match unit_files.get(&name) {
                    Some((ufp, e)) => (Some(ufp.clone()), *e),
                    None => (None, false),
                };
                (
                    service_id,
                    (
                        name,
                        description,
                        load_state,
                        active_state,
                        unit_obj_path,
                        unit_file_path,
                        enabled,
                    ),
                )
            };

            let mut services = HashMap::new();
            if !unit_files.is_empty() {
                let unit_names = unit_files.keys().cloned().collect::<Vec<_>>();
                services = sync!(rt, self.systemd1.list_units_by_names(&unit_names))
                    .unwrap_or_else(|e| {
                        log::warn!("Failed to list unit files: {e}");
                        Vec::new()
                    })
                    .drain(..)
                    .map(preprocess_service)
                    .collect()
            }

            if services.is_empty() {
                services = sync!(rt, self.systemd1.list_units())?
                    .drain(..)
                    .filter(|s| {
                        s.0.ends_with(".service")
                            || s.0.ends_with(".mount")
                            || s.0.ends_with(".socket")
                    })
                    .map(preprocess_service)
                    .collect()
            }

            for cached_id in self.service_cache.keys() {
                if !services.contains_key(cached_id) {
                    self.to_remove.push(*cached_id);
                }
            }

            for id in self.to_remove.drain(..) {
                self.service_cache.remove(&id);
            }

            for (
                id,
                (
                    name,
                    description,
                    load_state,
                    active_state,
                    unit_obj_path,
                    unit_file_path,
                    enabled,
                ),
            ) in services.drain()
            {
                if self.service_cache.contains_key(&id) {
                    continue;
                }

                if !(name.ends_with(".service")
                    || name.ends_with(".socket")
                    || name.ends_with(".mount"))
                {
                    continue;
                }

                let load_state: LoadState = load_state.as_str().into();
                if load_state == LoadState::NotFound {
                    continue;
                }

                let active_state: ActiveState = active_state.as_str().into();

                let new_service = Service {
                    id,
                    name,
                    description: if description.is_empty() {
                        None
                    } else {
                        Some(description)
                    },
                    enabled,
                    running: active_state == ActiveState::Active,
                    failed: active_state == ActiveState::Failed,
                    pid: None,
                    user: None,
                    group: None,

                    file_path: unit_file_path,
                };

                let Some(newval) =
                    sync!(rt, ServiceCache::new(new_service, unit_obj_path, self.bus))
                else {
                    continue;
                };
                self.service_cache.insert(id, newval);
            }
        }

        for service_cache in self.service_cache.values_mut() {
            let _ = sync!(rt, service_cache.update_service_properties());
            if let Some((_, enabled)) = unit_files.get(&service_cache.service.name) {
                service_cache.service.enabled = *enabled;
            }
        }

        Ok(self
            .service_cache
            .values()
            .map(|s| (s.service.id, s.service.clone()))
            .collect())
    }

    pub fn full_refresh_required(&self) -> &Arc<AtomicBool> {
        &self.full_refresh_required
    }
}
