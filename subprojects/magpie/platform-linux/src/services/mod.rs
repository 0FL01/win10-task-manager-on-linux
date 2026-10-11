/* src/services/mod.rs
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
use std::num::NonZero;

use magpie_platform::services::Service;

mod openrc;
mod systemd;

#[allow(clippy::large_enum_variant)]
enum ServiceCacheKind {
    None,
    SystemD((systemd::SystemD, Option<systemd::SystemD>)),
    OpenRC(openrc::OpenRC),
}

enum ServiceManagerKind {
    None,
    SystemD((systemd::ServiceManager, Option<systemd::ServiceManager>)),
    OpenRC(openrc::ServiceManager),
}

pub struct ServiceCache {
    kind: ServiceCacheKind,
    user_services: HashMap<u64, Service>,
    system_services: HashMap<u64, Service>,
}

pub struct ServiceManager {
    kind: ServiceManagerKind,
}

impl magpie_platform::services::ServiceCache for ServiceCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        let kind = match systemd::SystemD::systemd_system() {
            Ok(system) => match systemd::SystemD::systemd_user() {
                Ok(user) => ServiceCacheKind::SystemD((system, Some(user))),
                Err(e) => {
                    log::error!("Initialized SystemD service cache without user bus: {e}");
                    ServiceCacheKind::SystemD((system, None))
                }
            },
            Err(e) => {
                log::debug!("Failed to initialize SystemD service cache: {e}");
                match openrc::OpenRC::new() {
                    Ok(openrc) => ServiceCacheKind::OpenRC(openrc),
                    Err(e) => {
                        log::debug!("Failed to initialize OpenRC service cache: {e}");
                        ServiceCacheKind::None
                    }
                }
            }
        };

        Self {
            kind,
            user_services: HashMap::new(),
            system_services: HashMap::new(),
        }
    }

    fn refresh(&mut self) {
        match &mut self.kind {
            ServiceCacheKind::SystemD((system, user)) => {
                let user = user.as_mut();
                match user.map(|user| user.list_services()) {
                    Some(Ok(mut services)) => {
                        std::mem::swap(&mut self.user_services, &mut services);
                    }
                    Some(Err(e)) => {
                        log::error!("Failed to list systemd user services: {e}");
                    }
                    None => {}
                };

                match system.list_services() {
                    Ok(mut services) => {
                        std::mem::swap(&mut self.system_services, &mut services);
                    }
                    Err(e) => {
                        log::error!("Failed to list systemd system services: {e}");
                    }
                };
            }
            ServiceCacheKind::OpenRC(openrc) => {
                self.system_services = match openrc.list_services() {
                    Ok(services) => services,
                    Err(e) => {
                        log::error!("Failed to list OpenRC services: {e}");
                        return;
                    }
                };
            }
            ServiceCacheKind::None => {}
        }
    }

    fn user_entries(&self) -> &HashMap<u64, Service> {
        &self.user_services
    }

    fn system_entries(&self) -> &HashMap<u64, Service> {
        &self.system_services
    }
}

impl magpie_platform::services::ServiceManager for ServiceManager {
    type ServiceCache = ServiceCache;

    fn new() -> Self {
        let kind = match systemd::ServiceManager::system_manager() {
            Ok(system) => match systemd::ServiceManager::user_manager() {
                Ok(user) => ServiceManagerKind::SystemD((system, Some(user))),
                Err(e) => {
                    log::error!("Initialized SystemD service manager without user bus: {e}");
                    ServiceManagerKind::SystemD((system, None))
                }
            },
            Err(e) => {
                log::debug!("Failed to initialize SystemD service manager: {e}");
                match openrc::ServiceManager::new() {
                    Ok(openrc) => ServiceManagerKind::OpenRC(openrc),
                    Err(e) => {
                        log::debug!("Failed to initialize OpenRC service manager: {e}");
                        ServiceManagerKind::None
                    }
                }
            }
        };

        Self { kind }
    }

    fn logs(
        &self,
        service_cache: &ServiceCache,
        id: u64,
        pid: Option<NonZero<u32>>,
    ) -> Option<String> {
        match &self.kind {
            ServiceManagerKind::SystemD((system, user)) => {
                let Some((svc_mgr, service)) =
                    find_service_by_id(id, service_cache, system, user.as_ref())
                else {
                    log::warn!("Service with id {id} not found");
                    return None;
                };

                let Some(svc_mgr) = svc_mgr else {
                    log::warn!("Service manager not found for service id {id}");
                    return None;
                };

                match svc_mgr.service_logs(&service.name, pid) {
                    Ok(logs) => Some(logs),
                    Err(e) => {
                        log::warn!(
                            "Failed to get logs for systemd service {} {e}",
                            service.name
                        );
                        None
                    }
                }
            }
            ServiceManagerKind::OpenRC(_) => None,
            ServiceManagerKind::None => None,
        }
    }

    fn start(&self, service_cache: &ServiceCache, id: u64) {
        match &self.kind {
            ServiceManagerKind::SystemD((system, user)) => {
                let Some((svc_mgr, service)) =
                    find_service_by_id(id, service_cache, system, user.as_ref())
                else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                let Some(svc_mgr) = svc_mgr else {
                    log::warn!("Service manager not found for service id {id}");
                    return;
                };

                match svc_mgr.start_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Failed to start systemd service {}: {e}", service.name);
                    }
                }
            }
            ServiceManagerKind::OpenRC(openrc) => {
                let Some(service) = service_cache.system_services.get(&id) else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                match openrc.start_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => log::warn!("Failed to start OpenRC service {}: {e}", service.name),
                }
            }
            ServiceManagerKind::None => {}
        }
    }

    fn stop(&self, service_cache: &ServiceCache, id: u64) {
        match &self.kind {
            ServiceManagerKind::SystemD((system, user)) => {
                let Some((svc_mgr, service)) =
                    find_service_by_id(id, service_cache, system, user.as_ref())
                else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                let Some(svc_mgr) = svc_mgr else {
                    log::warn!("Service manager not found for service id {id}");
                    return;
                };

                match svc_mgr.stop_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Failed to stop systemd service {}: {e}", service.name);
                    }
                }
            }
            ServiceManagerKind::OpenRC(openrc) => {
                let Some(service) = service_cache.system_services.get(&id) else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                match openrc.stop_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => log::warn!("Failed to stop OpenRC service {}: {e}", service.name),
                }
            }
            ServiceManagerKind::None => {}
        }
    }

    fn restart(&self, service_cache: &ServiceCache, id: u64) {
        match &self.kind {
            ServiceManagerKind::SystemD((system, user)) => {
                let Some((svc_mgr, service)) =
                    find_service_by_id(id, service_cache, system, user.as_ref())
                else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                let Some(svc_mgr) = svc_mgr else {
                    log::warn!("Service manager not found for service id {id}");
                    return;
                };

                match svc_mgr.restart_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Failed to restart systemd service {}: {e}", service.name);
                    }
                }
            }
            ServiceManagerKind::OpenRC(openrc) => {
                let Some(service) = service_cache.system_services.get(&id) else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                match openrc.restart_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => log::warn!("Failed to restart OpenRC service {}: {e}", service.name),
                }
            }
            ServiceManagerKind::None => {}
        }
    }

    fn enable(&self, service_cache: &ServiceCache, id: u64) {
        match &self.kind {
            ServiceManagerKind::SystemD((system, user)) => {
                let Some((svc_mgr, service)) =
                    find_service_by_id(id, service_cache, system, user.as_ref())
                else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                let Some(svc_mgr) = svc_mgr else {
                    log::warn!("Service manager not found for service id {id}");
                    return;
                };

                let systemd = match &service_cache.kind {
                    ServiceCacheKind::SystemD((systemd_system, systemd_user)) => {
                        if std::ptr::eq(svc_mgr, system) {
                            systemd_system
                        } else {
                            let Some(systemd_user) = systemd_user else {
                                log::warn!(
                                    "User service cache manager not found for service id {id}"
                                );
                                return;
                            };
                            systemd_user
                        }
                    }
                    _ => {
                        log::warn!("Service cache kind does not match service manager kind");
                        return;
                    }
                };

                match svc_mgr.enable_service(&service.name, systemd) {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Failed to enable systemd service {}: {e}", service.name);
                    }
                }
            }
            ServiceManagerKind::OpenRC(openrc) => {
                let Some(service) = service_cache.system_services.get(&id) else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                match openrc.enable_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => log::warn!("Failed to enable OpenRC service {}: {e}", service.name),
                }
            }
            ServiceManagerKind::None => {}
        }
    }

    fn disable(&self, service_cache: &ServiceCache, id: u64) {
        match &self.kind {
            ServiceManagerKind::SystemD((system, user)) => {
                let Some((svc_mgr, service)) =
                    find_service_by_id(id, service_cache, system, user.as_ref())
                else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                let Some(svc_mgr) = svc_mgr else {
                    log::warn!("Service manager not found for service id {id}");
                    return;
                };

                let systemd = match &service_cache.kind {
                    ServiceCacheKind::SystemD((systemd_system, systemd_user)) => {
                        if std::ptr::eq(svc_mgr, system) {
                            systemd_system
                        } else {
                            let Some(systemd_user) = systemd_user else {
                                log::warn!(
                                    "User service cache manager not found for service id {id}"
                                );
                                return;
                            };
                            systemd_user
                        }
                    }
                    _ => {
                        log::warn!("Service cache kind does not match service manager kind");
                        return;
                    }
                };

                match svc_mgr.disable_service(&service.name, systemd) {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Failed to disable systemd service {}: {e}", service.name);
                    }
                }
            }
            ServiceManagerKind::OpenRC(openrc) => {
                let Some(service) = service_cache.system_services.get(&id) else {
                    log::warn!("Service with id {id} not found");
                    return;
                };

                match openrc.disable_service(&service.name) {
                    Ok(_) => {}
                    Err(e) => log::warn!("Failed to disable OpenRC service {}: {e}", service.name),
                }
            }
            ServiceManagerKind::None => {}
        }
    }
}

fn find_service_by_id<'sm, 'sc>(
    id: u64,
    service_cache: &'sc ServiceCache,
    system_mgr: &'sm systemd::ServiceManager,
    user_mgr: Option<&'sm systemd::ServiceManager>,
) -> Option<(Option<&'sm systemd::ServiceManager>, &'sc Service)> {
    service_cache
        .user_services
        .get(&id)
        .map(|s| (user_mgr, s))
        .or_else(|| {
            service_cache
                .system_services
                .get(&id)
                .map(|s| (Some(system_mgr), s))
        })
}
