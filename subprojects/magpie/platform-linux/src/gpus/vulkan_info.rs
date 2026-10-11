/* src/gpus/vulkan_info.rs
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

use std::ffi::CStr;

use ash::vk;
use serde::{Deserialize, Serialize};

use magpie_platform::gpus::ApiVersion;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VulkanDevice {
    pub ids_key: u32,
    pub primary_node: Option<(u32, u32)>,
    pub render_node: Option<(u32, u32)>,
    pub version: ApiVersion,
    pub name: String,
    pub driver: Option<String>,
    pub integrated: Option<bool>,
}

pub struct VulkanInfo {
    entry: ash::Entry,
    vk_instance: ash::Instance,
    has_properties2: bool,
}

impl Drop for VulkanInfo {
    fn drop(&mut self) {
        unsafe {
            self.vk_instance.destroy_instance(None);
        }
    }
}

impl VulkanInfo {
    pub fn new() -> Option<Self> {
        let entry = match unsafe { ash::Entry::load() } {
            Ok(e) => e,
            Err(e) => {
                log::error!(
                    "Failed to get Vulkan information: Could not load 'libvulkan.so.1'; {}",
                    e
                );
                return None;
            }
        };
        log::debug!("Loaded Vulkan library");

        let has_properties2 = unsafe { entry.enumerate_instance_extension_properties(None) }
            .map(|extensions| {
                has_extension(&extensions, ash::khr::get_physical_device_properties2::NAME)
            })
            .unwrap_or(false);
        if !has_properties2 {
            log::debug!(
                "VK_KHR_get_physical_device_properties2 is unavailable, Vulkan devices can only be matched by PCI IDs"
            );
        }

        let enabled_extensions = if has_properties2 {
            vec![ash::khr::get_physical_device_properties2::NAME.as_ptr()]
        } else {
            vec![]
        };

        let app_info = vk::ApplicationInfo {
            api_version: vk::make_api_version(0, 1, 0, 0),
            ..Default::default()
        };
        let create_info = vk::InstanceCreateInfo {
            p_application_info: &app_info,
            enabled_extension_count: enabled_extensions.len() as u32,
            pp_enabled_extension_names: enabled_extensions.as_ptr(),
            ..Default::default()
        };

        let instance = match unsafe { entry.create_instance(&create_info, None) } {
            Ok(i) => i,
            Err(e) => {
                log::error!(
                    "Failed to get Vulkan information: Could not create instance; {}",
                    e
                );
                return None;
            }
        };
        log::debug!("Created Vulkan instance");

        Some(Self {
            entry,
            vk_instance: instance,
            has_properties2,
        })
    }

    pub unsafe fn devices(&self) -> Vec<VulkanDevice> {
        let physical_devices = self
            .vk_instance
            .enumerate_physical_devices()
            .unwrap_or_else(|e| {
                log::warn!(
                    "Failed to get Vulkan information: No Vulkan capable devices found ({})",
                    e
                );
                vec![]
            });

        let properties2 = self.has_properties2.then(|| {
            ash::khr::get_physical_device_properties2::Instance::new(&self.entry, &self.vk_instance)
        });

        let mut devices = Vec::with_capacity(physical_devices.len());
        for device in physical_devices {
            let properties = self.vk_instance.get_physical_device_properties(device);

            let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
            let mut driver = vk::PhysicalDeviceDriverProperties::default();
            let mut has_driver = false;
            if let Some(properties2) = properties2.as_ref() {
                if self.supports_drm_properties(device) {
                    let mut chain = vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
                    properties2.get_physical_device_properties2(device, &mut chain);
                }
                if properties.api_version >= vk::make_api_version(0, 1, 2, 0)
                    || self.supports_extension(device, ash::khr::driver_properties::NAME)
                {
                    let mut chain = vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
                    properties2.get_physical_device_properties2(device, &mut chain);
                    has_driver = true;
                }
            }

            let version = properties.api_version;
            let device = VulkanDevice {
                ids_key: ((properties.vendor_id & 0xffff) << 16) | (properties.device_id & 0xffff),
                primary_node: (drm.has_primary == vk::TRUE)
                    .then_some((drm.primary_major as u32, drm.primary_minor as u32)),
                render_node: (drm.has_render == vk::TRUE)
                    .then_some((drm.render_major as u32, drm.render_minor as u32)),
                version: ApiVersion {
                    major: (version >> 22) & 0x7f,
                    minor: (version >> 12) & 0x3ff,
                    patch: Some(version & 0xfff),
                },
                name: properties
                    .device_name_as_c_str()
                    .ok()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                driver: has_driver
                    .then(|| {
                        let name = CStr::from_ptr(driver.driver_name.as_ptr()).to_string_lossy();
                        let info = CStr::from_ptr(driver.driver_info.as_ptr()).to_string_lossy();
                        format!("{name} {info}").trim().to_owned()
                    })
                    .filter(|driver| !driver.is_empty()),
                integrated: match properties.device_type {
                    vk::PhysicalDeviceType::INTEGRATED_GPU => Some(true),
                    vk::PhysicalDeviceType::DISCRETE_GPU => Some(false),
                    _ => None,
                },
            };

            log::debug!(
                "Found Vulkan device: {:?} {:?}",
                properties.device_name_as_c_str().unwrap_or(c"<unknown>"),
                device
            );
            devices.push(device);
        }

        devices
    }

    unsafe fn supports_drm_properties(&self, device: vk::PhysicalDevice) -> bool {
        self.supports_extension(device, ash::ext::physical_device_drm::NAME)
    }

    unsafe fn supports_extension(&self, device: vk::PhysicalDevice, name: &CStr) -> bool {
        match self
            .vk_instance
            .enumerate_device_extension_properties(device)
        {
            Ok(extensions) => has_extension(&extensions, name),
            Err(_) => false,
        }
    }
}

fn has_extension(extensions: &[vk::ExtensionProperties], name: &CStr) -> bool {
    extensions
        .iter()
        .any(|extension| extension.extension_name_as_c_str().is_ok_and(|e| e == name))
}
