/* magpie_client/client.rs
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

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use std::{cell::RefCell, collections::HashMap, sync::Arc};

use arrayvec::ArrayString;
use gtk::glib::{g_critical, g_debug};

use magpie_types::about::{about_response, About};
use magpie_types::apps::apps_icon_response::AppsIconResponseValue;
use magpie_types::apps::apps_response::AppList;
use magpie_types::apps::icon::Icon;
pub use magpie_types::apps::App;
use magpie_types::apps::{apps_icon_response, apps_response};
use magpie_types::battery::battery_response;
use magpie_types::battery::battery_response::BatteryList;
pub use magpie_types::battery::Battery;
use magpie_types::common::Empty;
use magpie_types::cpu::cpu_response;
pub use magpie_types::cpu::Cpu;
use magpie_types::disks::disks_response;
use magpie_types::disks::disks_response::{DiskList, OptionalSmartData};
use magpie_types::disks::disks_response_error::Error;
pub use magpie_types::disks::{Disk, DiskKind, ErrorEjectFailed, SmartData};
use magpie_types::fan::fans_response;
use magpie_types::fan::fans_response::FanList;
pub use magpie_types::fan::Fan;
use magpie_types::gpus::gpus_response;
use magpie_types::gpus::gpus_response::GpuMap;
pub use magpie_types::gpus::Gpu;
use magpie_types::ipc::{self, response};
use magpie_types::memory::memory_response::MemoryInfo;
use magpie_types::memory::{memory_request, memory_response};
pub use magpie_types::memory::{Memory, MemoryDevice};
use magpie_types::network::connections_response;
use magpie_types::network::connections_response::ConnectionList;
pub use magpie_types::network::Connection;
use magpie_types::processes::processes_response;
use magpie_types::processes::processes_response::process_map::NetworkStatsError;
use magpie_types::processes::processes_response::ProcessMap;
pub use magpie_types::processes::Process;
use magpie_types::prost::Message;
use magpie_types::services::services_response;
use magpie_types::services::services_response::ServiceList;
pub use magpie_types::services::Service;
use magpie_types::setup_script::script_response;

use crate::magpie_client::flatpak_app_path;
use crate::{flatpak_data_dir, is_flatpak, show_error_dialog_and_exit};

mod nng {
    pub use nng_c_sys::nng_errno_enum::*;
}

type ResponseBody = response::Body;
type AboutResponse = about_response::Response;
type AppsResponse = apps_response::Response;
type AppsIconResponse = apps_icon_response::Response;
type CpuResponse = cpu_response::Response;
type DisksResponse = disks_response::Response;
type FansResponse = fans_response::Response;
type BatteriesResponse = battery_response::Response;
type GpusResponse = gpus_response::Response;
type MemoryResponse = memory_response::Response;
type ConnectionsResponse = connections_response::Response;
type ProcessesResponse = processes_response::Response;
type ServicesResponse = services_response::Response;
type ScriptResponse = script_response::Response;

const ENV_MC_DEBUG_MAGPIE_PROCESS_SOCK: &str = "MC_DEBUG_MAGPIE_PROCESS_SOCK";

macro_rules! parse_response {
    ($response: ident, $body_kind: path, $response_kind_ok: path, $response_kind_err: path, $do: expr) => {{
        let expected_type = stringify!($response_kind_ok);
        match $response {
            Some($body_kind(response)) => match response.response {
                Some($response_kind_ok(arg)) => $do(arg),
                Some($response_kind_err(e)) => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Error while getting {}: {:?}",
                        expected_type,
                        e
                    );
                    Default::default()
                }
                _ => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Unexpected response: {:?}",
                        response.response
                    );
                    Default::default()
                }
            },
            _ => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Unexpected response: {:?}",
                    $response
                );
                Default::default()
            }
        }
    }};
}

macro_rules! parse_response_with_err {
    ($response: ident, $body_kind: path, $response_kind_ok: path, $response_kind_err: path, $do: expr) => {{
        match $response {
            Some($body_kind(response)) => match response.response {
                Some($response_kind_ok(arg)) => Some(Ok($do(arg))),
                Some($response_kind_err(e)) => Some(Err(e)),
                _ => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Unexpected response: {:?}",
                        response.response
                    );
                    None
                }
            },
            _ => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Unexpected response: {:?}",
                    $response
                );
                None
            }
        }
    }};
}

fn random_string<const CAP: usize>() -> ArrayString<CAP> {
    let mut result = ArrayString::new();
    for _ in 0..CAP {
        if rand::random::<bool>() {
            result.push(rand::random_range(b'a'..=b'z') as char);
        } else {
            result.push(rand::random_range(b'0'..=b'9') as char);
        }
    }

    result
}

fn magpie_command(socket_addr: &str) -> std::process::Command {
    fn executable() -> String {
        use gtk::glib::g_debug;

        let exe_simple = "missioncenter-magpie".to_owned();

        if is_flatpak() {
            let flatpak_app_path = flatpak_app_path();

            let cmd_glibc_status = cmd_flatpak_host!(&format!(
                "{}/bin/missioncenter-magpie-glibc --test",
                flatpak_app_path
            ))
            .status()
            .is_ok_and(|exit_status| exit_status.success());
            if cmd_glibc_status {
                let exe_glibc = format!("{}/bin/missioncenter-magpie-glibc", flatpak_app_path);
                g_debug!(
                    "MissionCenter::Gatherer",
                    "Magpie executable name: {}",
                    &exe_glibc
                );
                return exe_glibc;
            }

            let cmd_musl_status = cmd_flatpak_host!(&format!(
                "{}/bin/missioncenter-magpie-musl --test",
                flatpak_app_path
            ))
            .status()
            .is_ok_and(|exit_status| exit_status.success());
            if cmd_musl_status {
                let exe_musl = format!("{}/bin/missioncenter-magpie-musl", flatpak_app_path);
                g_debug!(
                    "MissionCenter::Gatherer",
                    "Magpie executable name: {}",
                    &exe_musl
                );
                return exe_musl;
            }
        }

        g_debug!(
            "MissionCenter::Gatherer",
            "Magpie executable name: {}",
            &exe_simple
        );

        exe_simple
    }

    let mut command = if is_flatpak() {
        let mut cmd = std::process::Command::new("/app/bin/missioncenter-spawner");
        cmd.arg("-v")
            .arg("--env=LD_PRELOAD=")
            .arg(format!(
                "--env=MC_MAGPIE_HW_DB={}/share/missioncenter/hw.db",
                flatpak_app_path()
            ))
            .arg(format!(
                "--env=RUST_LOG={}",
                std::env::var("RUST_LOG").unwrap_or_default()
            ))
            .arg(executable());
        cmd
    } else {
        std::process::Command::new(executable())
    };

    command
        .env_remove("LD_PRELOAD")
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .arg("--addr")
        .arg(socket_addr);

    command
}

fn connect_socket(socket: &mut nng_c::Socket, socket_addr: &str) -> bool {
    let _ = socket.close();
    socket.id = 0;

    let new_socket = match nng_c::Socket::req0().map_err(|e| e.raw_code()) {
        Ok(s) => s,
        Err(error_code) => {
            let msg = match error_code {
                nng::NNG_ENOMEM => "Out of memory".to_string(),
                nng::NNG_ENOTSUP => "Protocol not supported".to_string(),
                _ => format!("Unknown error: {error_code}"),
            };
            g_critical!("MissionCenter::Gatherer", "Failed to open socket: {msg}");
            return false;
        }
    };

    *socket = new_socket;

    match socket
        .connect(nng_c::str::String::new(socket_addr.as_bytes()))
        .map_err(|e| e.raw_code())
    {
        Ok(_) => true,
        Err(error_code) => {
            let msg = match error_code {
                nng::NNG_EADDRINVAL => "An invalid url was specified".to_string(),
                nng::NNG_ECLOSED => "The socket is not open".to_string(),
                nng::NNG_ECONNREFUSED => "The remote peer refused the connection".to_string(),
                nng::NNG_ECONNRESET => "The remote peer reset the connection".to_string(),
                nng::NNG_EINVAL => {
                    "An invalid set of flags or an invalid url was specified".to_string()
                }
                nng::NNG_ENOMEM => "Insufficient memory is available".to_string(),
                nng::NNG_EPEERAUTH => "Authentication or authorization failure".to_string(),
                nng::NNG_EPROTO => "A protocol error occurred".to_string(),
                nng::NNG_EUNREACHABLE => "The remote address is not reachable".to_string(),
                _ => format!("Unknown error: {error_code}"),
            };
            g_critical!("MissionCenter::Gatherer", "Failed to dial socket: {msg}");
            false
        }
    }
}

fn make_request(
    request: ipc::Request,
    socket: &mut nng_c::Socket,
    socket_addr: &str,
) -> Option<ipc::Response> {
    fn try_reconnect(socket: &mut nng_c::Socket, socket_addr: &str) {
        socket.close();

        for i in 0..=5 {
            if !connect_socket(socket, socket_addr) {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to reconnect to Magpie. Retrying in 100ms (try {}/5)",
                    i + 1
                );
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }

            return;
        }

        show_error_dialog_and_exit(
            "Lost connection to Magpie and failed to reconnect after 5 tries. Giving up.",
        );
    }

    let mut req_buf = Vec::new();

    if let Err(e) = request.encode(&mut req_buf) {
        g_critical!(
            "MissionCenter::Gatherer",
            "Failed to encode request {:?}: {}",
            req_buf,
            e
        );
        return None;
    }

    if let Err(error_code) = socket
        .send(nng_c::socket::Buf::from(req_buf.as_slice()))
        .map_err(|e| e.raw_code())
    {
        match error_code {
            nng::NNG_EAGAIN => {
                g_critical!("MissionCenter::Gatherer","Failed to send request: The operation would block, but NNG_FLAG_NONBLOCK was specified");
            }
            nng::NNG_ECLOSED => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: The socket is not open"
                );
                try_reconnect(socket, socket_addr);
            }
            nng::NNG_EINVAL => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: An invalid set of flags was specified"
                );
            }
            nng::NNG_EMSGSIZE => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: The value of size is too large"
                );
            }
            nng::NNG_ENOMEM => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: Insufficient memory is available"
                );
            }
            nng::NNG_ENOTSUP => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: The protocol for socket does not support sending"
                );
            }
            nng::NNG_ESTATE => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: The socket cannot send data in this state"
                );
            }
            nng::NNG_ETIMEDOUT => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: The operation timed out"
                );
            }
            _ => {
                g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to send request: Unknown error: {error_code}",
                );
            }
        }
        return None;
    }

    let message = match socket.recv_msg().map_err(|e| e.raw_code()) {
        Ok(buffer) => buffer,
        Err(error_code) => {
            match error_code {
                nng::NNG_EAGAIN => {
                    g_critical!("MissionCenter::Gatherer","Failed to read message: The operation would block, but NNG_FLAG_NONBLOCK was specified");
                }
                nng::NNG_ECLOSED => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Failed to read message: The socket is not open"
                    );
                    try_reconnect(socket, socket_addr);
                }
                nng::NNG_EINVAL => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Failed to read message: An invalid set of flags was specified"
                    );
                }
                nng::NNG_EMSGSIZE => {
                    g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to read message: The received message did not fit in the size provided"
                );
                }
                nng::NNG_ENOMEM => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Failed to read message: Insufficient memory is available"
                    );
                }
                nng::NNG_ENOTSUP => {
                    g_critical!(
                    "MissionCenter::Gatherer",
                    "Failed to read message: The protocol for socket does not support receiving"
                );
                }
                nng::NNG_ESTATE => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Failed to read message: The socket cannot receive data in this state"
                    );
                }
                nng::NNG_ETIMEDOUT => {
                    g_debug!(
                        "MissionCenter::Gatherer",
                        "No message received for 64ms, waiting and trying again..."
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Failed to read message: Unknown error: {error_code}",
                    );
                }
            };
            return None;
        }
    };

    if message.body().is_empty() {
        g_critical!(
            "MissionCenter::Gatherer",
            "Failed to read response: Empty message"
        );
        return None;
    }

    let response = match ipc::Response::decode(message.body()) {
        Ok(r) => r,
        Err(e) => {
            g_critical!(
                "MissionCenter::Gatherer",
                "Error while decoding response: {:?}",
                e
            );
            return None;
        }
    };

    Some(response)
}

pub struct Client {
    socket: RefCell<nng_c::Socket>,

    socket_addr: Arc<str>,
    child_thread: RefCell<std::thread::JoinHandle<()>>,
    stop_requested: Arc<AtomicBool>,

    core_count: AtomicU32,
    scale_cpu_usage_to_core_count: AtomicBool,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Client {
    pub fn new() -> Self {
        let socket_addr =
            if let Ok(mut existing_sock) = std::env::var(ENV_MC_DEBUG_MAGPIE_PROCESS_SOCK) {
                existing_sock.push('\0');
                Arc::from(existing_sock)
            } else {
                if is_flatpak() {
                    Arc::from(format!(
                        "ipc://{}/magpie.ipc\0",
                        flatpak_data_dir().display()
                    ))
                } else {
                    Arc::from(format!("ipc:///tmp/magpie_{}.ipc\0", random_string::<8>()))
                }
            };

        let socket = nng_c::Socket::req0().expect("Could not create initial socket");

        Self {
            socket: RefCell::new(socket),

            socket_addr,
            child_thread: RefCell::new(std::thread::spawn(|| {})),
            stop_requested: Arc::new(AtomicBool::new(false)),

            core_count: AtomicU32::new(1),
            scale_cpu_usage_to_core_count: AtomicBool::new(false),
        }
    }

    pub fn start(&self) {
        fn start_magpie_process_thread(
            socket_addr: Arc<str>,
            stop_requested: Arc<AtomicBool>,
        ) -> std::thread::JoinHandle<()> {
            std::thread::spawn(move || {
                fn spawn_child(socket_addr: &str) -> std::process::Child {
                    match magpie_command(socket_addr.trim_end_matches('\0')).spawn() {
                        Ok(child) => child,
                        Err(e) => {
                            g_critical!(
                                "MissionCenter::Gatherer",
                                "Failed to spawn Magpie process: {}",
                                &e
                            );
                            show_error_dialog_and_exit(&format!(
                                "Failed to spawn Magpie process: {}",
                                e
                            ));
                        }
                    }
                }

                let mut child = spawn_child(&socket_addr);

                while !stop_requested.load(Ordering::Relaxed) {
                    match child.try_wait() {
                        Ok(Some(exit_status)) => {
                            let _ = std::fs::remove_file(&socket_addr[6..]);

                            if !stop_requested.load(Ordering::Relaxed) {
                                g_critical!(
                                    "MissionCenter::Gatherer",
                                    "Magpie process exited unexpectedly: {}. Restarting...",
                                    exit_status
                                );
                                std::mem::swap(&mut child, &mut spawn_child(&socket_addr));
                            }
                        }
                        Ok(None) => {
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                        Err(e) => {
                            g_critical!(
                                "MissionCenter::Gatherer",
                                "Failed to wait for Gatherer process to stop: {}",
                                &e
                            );
                            show_error_dialog_and_exit(&format!(
                                "Failed to wait for Gatherer process to stop: {}",
                                e
                            ));
                        }
                    }
                }

                let _ = child.kill();
            })
        }

        if !std::env::var(ENV_MC_DEBUG_MAGPIE_PROCESS_SOCK).is_ok() {
            *self.child_thread.borrow_mut() =
                start_magpie_process_thread(self.socket_addr.clone(), self.stop_requested.clone());
        }

        const START_WAIT_TIME_MS: u64 = 300;
        const RETRY_COUNT: i32 = 50;

        // Let the child process start up
        for _ in 0..RETRY_COUNT {
            std::thread::sleep(Duration::from_millis(START_WAIT_TIME_MS / 2));

            if connect_socket(&mut *self.socket.borrow_mut(), &self.socket_addr) {
                return;
            }

            std::thread::sleep(Duration::from_millis(START_WAIT_TIME_MS / 2));
        }

        show_error_dialog_and_exit("Failed to connect to Gatherer socket");
    }

    pub fn stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
        let child_thread = std::mem::replace(
            &mut *self.child_thread.borrow_mut(),
            std::thread::spawn(|| {}),
        );
        let _ = child_thread.join();
    }
}

impl Client {
    pub fn set_scale_cpu_usage_to_core_count(&self, v: bool) {
        self.scale_cpu_usage_to_core_count
            .store(v, Ordering::Relaxed);
    }

    pub fn about(&self) -> About {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(ipc::req_get_about(), &mut socket, self.socket_addr.as_ref())
            .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::About,
            AboutResponse::AboutInfo,
            AboutResponse::Error,
            |about: About| about
        )
    }

    pub fn cpu(&self) -> Cpu {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(ipc::req_get_cpu(), &mut socket, self.socket_addr.as_ref())
            .and_then(|response| response.body);

        let cpu = parse_response!(
            response,
            ResponseBody::Cpu,
            CpuResponse::Cpu,
            CpuResponse::Error,
            |cpu: Cpu| cpu
        );
        self.core_count.store(
            cpu.core_usage_percent.len().max(1) as u32,
            Ordering::Relaxed,
        );

        cpu
    }

    pub fn memory(&self) -> Memory {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_memory(memory_request::Kind::Memory),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Memory,
            MemoryResponse::MemoryInfo,
            MemoryResponse::Error,
            |memory: MemoryInfo| {
                let Some(memory_response::memory_info::Response::Memory(memory)) = memory.response
                else {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Unexpected response when getting memory",
                    );
                    return Default::default();
                };

                memory
            }
        )
    }

    pub fn memory_devices(&self) -> Vec<MemoryDevice> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_memory(memory_request::Kind::MemoryDevices),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Memory,
            MemoryResponse::MemoryInfo,
            MemoryResponse::Error,
            |memory: MemoryInfo| {
                let Some(memory_response::memory_info::Response::MemoryDevices(mut devices)) =
                    memory.response
                else {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Unexpected response when getting memory devices",
                    );
                    return vec![];
                };

                std::mem::take(&mut devices.devices)
            }
        )
    }

    pub fn disks_info(&self) -> Vec<Disk> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(ipc::req_get_disks(), &mut socket, self.socket_addr.as_ref())
            .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Disks,
            DisksResponse::Disks,
            DisksResponse::Error,
            |mut disks: DiskList| { std::mem::take(&mut disks.disks) }
        )
    }

    pub fn eject_disk(&self, disk_id: String) -> Result<(), ErrorEjectFailed> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_eject_disk(disk_id),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        let result = parse_response_with_err!(
            response,
            ResponseBody::Disks,
            DisksResponse::Eject,
            DisksResponse::Error,
            |_: Empty| { () }
        );

        let Some(result) = result else { return Ok(()) };
        match result {
            Ok(()) => Ok(()),
            Err(e) => match e.error {
                Some(Error::Eject(e)) => Err(e),
                _ => {
                    g_critical!(
                        "MissionCenter::Gatherer",
                        "Unexpected error response: {e:?}"
                    );
                    Ok(())
                }
            },
        }
    }

    pub fn smart_data(&self, disk_id: String) -> Option<SmartData> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_smart_data(disk_id),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Disks,
            DisksResponse::Smart,
            DisksResponse::Error,
            |mut smart_data: OptionalSmartData| { std::mem::take(&mut smart_data.smart) }
        )
    }

    pub fn fans_info(&self) -> Vec<Fan> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(ipc::req_get_fans(), &mut socket, self.socket_addr.as_ref())
            .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Fans,
            FansResponse::Fans,
            FansResponse::Error,
            |mut fans: FanList| { std::mem::take(&mut fans.fans) }
        )
    }

    pub fn batteries_info(&self) -> Vec<Battery> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_batteries(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Batteries,
            BatteriesResponse::Batteries,
            BatteriesResponse::Error,
            |mut batteries: BatteryList| { std::mem::take(&mut batteries.batteries) }
        )
    }

    pub fn network_connections(&self) -> HashMap<String, Connection> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_connections(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Connections,
            ConnectionsResponse::Connections,
            ConnectionsResponse::Error,
            |mut connections: ConnectionList| { std::mem::take(&mut connections.connections) }
        )
    }

    pub fn gpus(&self) -> HashMap<String, Gpu> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(ipc::req_get_gpus(), &mut socket, self.socket_addr.as_ref())
            .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Gpus,
            GpusResponse::Gpus,
            GpusResponse::Error,
            |mut gpus: GpuMap| { std::mem::take(&mut gpus.gpus) }
        )
    }

    pub fn processes(&self) -> (HashMap<u32, Process>, Option<NetworkStatsError>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_processes(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        let (mut processes, network_stats_error) = parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::Processes,
            ProcessesResponse::Error,
            |mut processes: ProcessMap| {
                (
                    std::mem::take(&mut processes.processes),
                    std::mem::take(&mut processes.network_stats_error),
                )
            }
        );

        let scale_cpu_usage_to_core_count =
            self.scale_cpu_usage_to_core_count.load(Ordering::Relaxed);
        let factor = if !scale_cpu_usage_to_core_count {
            self.core_count.load(Ordering::Relaxed) as f32
        } else {
            1.
        };
        for process in processes.values_mut() {
            process.usage_stats.cpu_usage /= factor;
        }

        (processes, network_stats_error)
    }

    pub fn apps(&self) -> HashMap<String, App> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(ipc::req_get_apps(), &mut socket, self.socket_addr.as_ref())
            .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Apps,
            AppsResponse::Apps,
            AppsResponse::Error,
            |mut app_list: AppList| {
                app_list
                    .apps
                    .drain(..)
                    .map(|app| (app.id.clone(), app))
                    .collect()
            }
        )
    }

    pub fn app_icons(&self, app_ids: Vec<String>) -> HashMap<String, Icon> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_app_icons(app_ids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::AppIcons,
            AppsIconResponse::Values,
            AppsIconResponse::Error,
            |mut app_list: AppsIconResponseValue| {
                app_list
                    .pairs
                    .drain()
                    .map(|(k, v)| (k, v.icon.unwrap_or_default()))
                    .collect()
            }
        )
    }

    pub fn user_services(&self) -> HashMap<u64, Service> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_user_services(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Services,
            ServicesResponse::Error,
            |mut service_list: ServiceList| {
                service_list
                    .services
                    .drain(..)
                    .map(|service| (service.id, service))
                    .collect()
            }
        )
    }

    pub fn system_services(&self) -> HashMap<u64, Service> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_system_services(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Services,
            ServicesResponse::Error,
            |mut service_list: ServiceList| {
                service_list
                    .services
                    .drain(..)
                    .map(|service| (service.id, service))
                    .collect()
            }
        )
    }

    pub fn service_logs(&self, service_id: u64, pid: Option<NonZeroU32>) -> String {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_get_logs(service_id, pid),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Logs,
            ServicesResponse::Error,
            |logs| logs
        )
    }

    pub fn terminate_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_terminate_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn kill_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_kill_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn interrupt_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_interrupt_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn signal_user_one_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_signal_user_one_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn signal_user_two_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_signal_user_two_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn hangup_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_hangup_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn continue_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_continue_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn suspend_processes(&self, pids: Vec<u32>) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_suspend_processes(pids),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Processes,
            ProcessesResponse::TermKill,
            ProcessesResponse::Error,
            |_| {}
        )
    }

    pub fn start_service(&self, service_id: u64) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_start_service(service_id),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Empty,
            ServicesResponse::Error,
            |_| {}
        )
    }

    pub fn stop_service(&self, service_id: u64) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_stop_service(service_id),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Empty,
            ServicesResponse::Error,
            |_| {}
        )
    }

    pub fn restart_service(&self, service_id: u64) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_restart_service(service_id),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Empty,
            ServicesResponse::Error,
            |_| {}
        )
    }

    pub fn enable_service(&self, service_id: u64) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_enable_service(service_id),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Empty,
            ServicesResponse::Error,
            |_| {}
        )
    }

    pub fn disable_service(&self, service_id: u64) {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_disable_service(service_id),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        parse_response!(
            response,
            ResponseBody::Services,
            ServicesResponse::Empty,
            ServicesResponse::Error,
            |_| {}
        )
    }

    pub fn setup_script_name(&self) -> Option<(String, String)> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_script_name(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        let result = parse_response_with_err!(
            response,
            ResponseBody::Script,
            ScriptResponse::Name,
            ScriptResponse::None,
            |value| value
        );

        match result {
            Some(Ok(v)) => Some((v.file, v.elevation_command)),
            _ => None,
        }
    }

    pub fn setup_script_run(&self) -> Result<(), String> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_script_run(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        let result = parse_response_with_err!(
            response,
            ResponseBody::Script,
            ScriptResponse::RunSuccess,
            ScriptResponse::Error,
            |value| value
        );

        match result {
            Some(Ok(_)) => Ok(()),
            Some(Err(e)) => Err(e),
            None => Err("Error receiving result from magpie".to_string()),
        }
    }

    pub fn setup_script_run_revert(&self) -> Result<(), String> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_script_run_revert(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        let result = parse_response_with_err!(
            response,
            ResponseBody::Script,
            ScriptResponse::RunSuccess,
            ScriptResponse::Error,
            |value| value
        );

        match result {
            Some(Ok(_)) => Ok(()),
            Some(Err(e)) => Err(e),
            None => Err("Error receiving result from magpie".to_string()),
        }
    }

    pub fn setup_script_open(&self) -> Result<(), String> {
        let mut socket = self.socket.borrow_mut();

        let response = make_request(
            ipc::req_script_open(),
            &mut socket,
            self.socket_addr.as_ref(),
        )
        .and_then(|response| response.body);

        let result = parse_response_with_err!(
            response,
            ResponseBody::Script,
            ScriptResponse::RunSuccess,
            ScriptResponse::Error,
            |value| value
        );

        match result {
            Some(Ok(_)) => Ok(()),
            Some(Err(e)) => Err(e),
            None => Err("Error receiving result from magpie".to_string()),
        }
    }
}
