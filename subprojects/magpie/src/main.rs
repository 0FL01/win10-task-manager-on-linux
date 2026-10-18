/* src/main.rs
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

#[cfg(all(feature = "production-logging", feature = "diagnostic-profile"))]
compile_error!(
    "production-logging and diagnostic-profile are mutually exclusive; use --no-default-features for diagnostic builds"
);

#[cfg(all(
    not(debug_assertions),
    not(feature = "production-logging"),
    not(feature = "diagnostic-profile")
))]
compile_error!("release builds require production-logging or explicit diagnostic-profile");

#[cfg(all(feature = "production-logging", not(debug_assertions)))]
const _: () = assert!(
    matches!(log::STATIC_MAX_LEVEL, log::LevelFilter::Warn),
    "production releases must statically cap logging at Warn"
);

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use clap::Parser;
use prost::Message;
use signal_hook::consts::signal::*;
use signal_hook::flag as signal_flag;

use magpie_platform::ipc::{Request, RequestBody, Response, ResponseBody, ResponseError};
#[cfg(feature = "diagnostic-profile")]
use magpie_platform::profile::Reporter;
use magpie_platform::profile::{Domain, Phase, Span};
use magpie_platform::DataCache;

#[cfg(target_os = "linux")]
use magpie_platform_linux as magpie_platform_os;

use magpie_platform_os::DataCacheTypes;

mod about;
mod apps;
mod battery;
mod cpu;
mod disks;
mod fan;
mod footprint;
mod gpus;
mod memory;
mod network;
mod nng;
mod processes;
mod services;

fn data_cache() -> &'static DataCache<DataCacheTypes> {
    static DATA_CACHE: OnceLock<DataCache<DataCacheTypes>> = OnceLock::new();
    DATA_CACHE.get_or_init(DataCache::new)
}

fn os_signals_listen(flag: Arc<AtomicBool>) {
    const SIGTERM_U: usize = SIGTERM as usize;
    const SIGINT_U: usize = SIGINT as usize;
    const SIGQUIT_U: usize = SIGQUIT as usize;

    let term = Arc::new(AtomicUsize::new(0));

    signal_flag::register_usize(SIGTERM, Arc::clone(&term), SIGTERM_U)
        .expect("Failed to register TERM signal");
    signal_flag::register_usize(SIGINT, Arc::clone(&term), SIGINT_U)
        .expect("Failed to register INT signal");
    signal_flag::register_usize(SIGQUIT, Arc::clone(&term), SIGQUIT_U)
        .expect("Failed to register QUIT signal");

    loop {
        match term.load(Ordering::Relaxed) {
            0 => {
                std::thread::sleep(Duration::from_millis(50));
            }
            SIGTERM_U => {
                log::debug!("Caught TERM signal, stopping...");
                flag.store(true, Ordering::Relaxed);
                break;
            }
            SIGINT_U => {
                log::debug!("Caught INT signal, stopping...");
                flag.store(true, Ordering::Relaxed);
                break;
            }
            SIGQUIT_U => {
                log::debug!("Caught QUIT signal, stopping...");
                flag.store(true, Ordering::Relaxed);
                break;
            }
            _ => unreachable!(),
        }
    }
}

fn handle_request(request_data: &[u8], socket: nng::nng_socket) {
    let mut profile = Span::new(Phase::RequestTotal);
    profile.items(request_data.len());
    let response = match Request::decode(request_data) {
        Ok(req) => {
            if let Some(body) = req.body {
                let domain = match &body {
                    RequestBody::GetAbout(_) => Domain::About,
                    RequestBody::GetApps(_) | RequestBody::GetAppsIcons(_) => Domain::Apps,
                    RequestBody::GetBattery(_) => Domain::Battery,
                    RequestBody::GetCpu(_) => Domain::Cpu,
                    RequestBody::GetDisks(_) => Domain::Disks,
                    RequestBody::GetFans(_) => Domain::Fans,
                    RequestBody::GetGpus(_) => Domain::Gpus,
                    RequestBody::GetMemory(_) => Domain::Memory,
                    RequestBody::GetConnections(_) => Domain::Network,
                    RequestBody::GetProcesses(_) => Domain::Processes,
                    RequestBody::GetServices(_) => Domain::Services,
                };
                let _profile = Span::new(Phase::Request(domain));
                match body {
                    RequestBody::GetAbout(_) => about::handle_request(),
                    RequestBody::GetApps(_) => apps::handle_request(),
                    RequestBody::GetAppsIcons(req) => apps::handle_icons_request(req.app_ids),
                    RequestBody::GetBattery(_) => battery::handle_request(),
                    RequestBody::GetCpu(_) => cpu::handle_request(),
                    RequestBody::GetDisks(req) => disks::handle_request(req.request),
                    RequestBody::GetFans(_) => fan::handle_request(),
                    RequestBody::GetGpus(_) => gpus::handle_request(),
                    RequestBody::GetMemory(req) => memory::handle_request(req.kind()),
                    RequestBody::GetConnections(_) => network::handle_request(),
                    RequestBody::GetProcesses(req) => processes::handle_request(req.request),
                    RequestBody::GetServices(req) => services::handle_request(req.request),
                }
            } else {
                log::error!("Empty request");

                let response = Response {
                    body: Some(ResponseBody::Error(ResponseError {
                        message: "Empty request".to_string(),
                    })),
                };

                let mut buffer = nng::Buffer::new(response.encoded_len());
                response.encode_raw(&mut buffer);

                buffer
            }
        }
        Err(e) => {
            log::error!("{:?}", e);

            let response = Response {
                body: Some(ResponseBody::Error(ResponseError {
                    message: format!("Error: {:?}", e),
                })),
            };

            let mut buffer = nng::Buffer::new(response.encoded_len());
            response.encode_raw(&mut buffer);

            buffer
        }
    };

    let (response, response_len) = response.leak();
    let res = unsafe {
        nng::nng_send(
            socket,
            response as *mut _,
            response_len,
            nng::NNG_FLAG_ALLOC,
        )
    };
    match res {
        nng::NNG_OK => {}
        nng::NNG_EAGAIN => {
            log::error!("Failed to send reply: The operation would block, but NNG_FLAG_NONBLOCK was specified");
        }
        nng::NNG_ECLOSED => {
            log::error!("Failed to send reply: The socket is not open");
        }
        nng::NNG_EINVAL => {
            log::error!("Failed to send reply: An invalid set of flags was specified");
        }
        nng::NNG_EMSGSIZE => {
            log::error!("Failed to send reply: The value of size is too large");
        }
        nng::NNG_ENOMEM => {
            log::error!("Failed to send reply: Insufficient memory is available");
        }
        nng::NNG_ENOTSUP => {
            log::error!("Failed to send reply: The protocol for socket does not support sending");
        }
        nng::NNG_ESTATE => {
            log::error!("Failed to send reply: The socket cannot send data in this state");
        }
        nng::NNG_ETIMEDOUT => {
            log::error!("Failed to send reply: The operation timed out");
        }
        _ => {
            log::error!("Failed to send reply: Unknown error: {}", res);
        }
    }
    if res != nng::NNG_OK {
        // NNG frees an `NNG_FLAG_ALLOC` buffer only on success (`nng_send` in nng.c).
        unsafe { nng::nng_free(response as *mut _, response_len) };
    }
}

fn bind(socket_addr: &str) -> magpie_platform::Result<nng::nng_socket> {
    let mut socket = nng::nng_socket { id: 0 };
    let res = unsafe { nng::nng_rep0_open(&mut socket) };
    match res {
        nng::NNG_OK => {}
        nng::NNG_ENOMEM => {
            log::error!("Failed to open socket: Out of memory");
            return Err(magpie_platform::Error::msg("Failed to open socket"));
        }
        nng::NNG_ENOTSUP => {
            log::error!("Failed to open socket: Protocol not supported");
            return Err(magpie_platform::Error::msg("Failed to open socket"));
        }
        _ => {
            log::error!("Failed to open socket: Unknown error: {}", res);
            return Err(magpie_platform::Error::msg("Failed to open socket"));
        }
    }

    let res = unsafe { nng::nng_setopt_ms(socket, nng::NNG_OPT_RECVTIMEO.as_ptr() as _, 100) };
    match res {
        nng::NNG_OK => {}
        nng::NNG_ECLOSED => {
            log::error!(
                "Failed to configure socket timeout: Parameter does not refer to an open socket"
            );
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_EINVAL => {
            log::error!("Failed to configure socket timeout: The value being passed is invalid");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_ENOTSUP => {
            log::error!("Failed to configure socket timeout: The option opt is not supported");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_EREADONLY => {
            log::error!("Failed to configure socket timeout: The option opt is read-only");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_ESTATE => {
            log::error!("Failed to configure socket timeout: The socket is in an inappropriate state for setting this option");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        _ => {
            log::error!("Failed to configure socket timeout: Unknown error: {}", res);
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
    }

    let res = unsafe {
        nng::nng_listen(
            socket,
            socket_addr.as_ptr() as *const _,
            std::ptr::null_mut(),
            0,
        )
    };
    match res {
        nng::NNG_OK => {}
        nng::NNG_EADDRINUSE => {
            log::error!("Failed to listen on {socket_addr}: Address in use");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_EADDRINVAL => {
            log::error!("Failed to listen on {socket_addr}: Address invalid");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_ECLOSED => {
            log::error!("Failed to listen on {socket_addr}: Connection closed prematurely");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_EINVAL => {
            log::error!("Failed to listen on {socket_addr}: Invalid argument");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_ENOMEM => {
            log::error!("Failed to listen on {socket_addr}: Out of memory");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        _ => {
            log::error!("Failed to listen on {socket_addr}: Unknown error: {res}");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
    }

    log::info!("Listening on {}...", socket_addr);

    Ok(socket)
}

fn serve(socket: nng::nng_socket, stop_requested: Arc<AtomicBool>) -> magpie_platform::Result<()> {
    std::thread::spawn({
        let stop_requested = stop_requested.clone();
        move || os_signals_listen(stop_requested)
    });

    let mut trimmer = footprint::Trimmer::new();
    while !stop_requested.load(Ordering::Relaxed) {
        trimmer.tick();

        let mut message_buffer: *mut libc::c_void = std::ptr::null_mut();
        let mut message_len: libc::size_t = 0;

        let res = unsafe {
            nng::nng_recv(
                socket,
                (&mut message_buffer) as *mut *mut _ as *mut _,
                &mut message_len,
                nng::NNG_FLAG_ALLOC,
            )
        };
        match res {
            nng::NNG_OK => {}
            nng::NNG_ETIMEDOUT => {
                log::debug!("No message received for 100ms, waiting and trying again...");
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            nng::NNG_EAGAIN => {
                log::error!("Failed to read message: The operation would block, but NNG_FLAG_NONBLOCK was specified");
                continue;
            }
            nng::NNG_ECLOSED => {
                log::error!("Failed to read message: The socket is not open");
                continue;
            }
            nng::NNG_EINVAL => {
                log::error!("Failed to read message: An invalid set of flags was specified");
                continue;
            }
            nng::NNG_EMSGSIZE => {
                log::error!(
                    "Failed to read message: The received message did not fit in the size provided"
                );
                continue;
            }
            nng::NNG_ENOMEM => {
                log::error!("Failed to read message: Insufficient memory is available");
                continue;
            }
            nng::NNG_ENOTSUP => {
                log::error!(
                    "Failed to read message: The protocol for socket does not support receiving"
                );
                continue;
            }
            nng::NNG_ESTATE => {
                log::error!("Failed to read message: The socket cannot receive data in this state");
                continue;
            }
            _ => {
                log::error!("Failed to read message: Unknown error: {}", res);
                continue;
            }
        }

        if message_len == 0 || message_buffer.is_null() {
            log::error!("Failed to read message: Empty message");
            continue;
        }

        let request =
            unsafe { core::slice::from_raw_parts(message_buffer as *const u8, message_len) };
        handle_request(request, socket);
        unsafe { nng::nng_free(message_buffer, message_len) };
    }

    Ok(())
}

/// Validate the packaged open GPU ABI without discovering devices or loading
/// optional driver libraries (NVML). An unnamed Cargo binary keeps the original
/// --test behavior; the installed daemon must have its complete private bundle.
#[cfg(target_os = "linux")]
fn test_packaged_gpu_libraries() -> magpie_platform::Result<()> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;

    let executable = std::env::current_exe()
        .map_err(|error| magpie_platform::Error::msg(format!("current executable: {error}")))?;
    let flavor = match executable.file_name().and_then(|name| name.to_str()) {
        Some("missioncenter-magpie") => "",
        Some("missioncenter-magpie-glibc") => "glibc",
        Some("missioncenter-magpie-musl") => "musl",
        _ => return Ok(()),
    };
    let directory = executable
        .parent()
        .and_then(|bin| bin.parent())
        .ok_or_else(|| magpie_platform::Error::msg("Invalid packaged daemon path"))?
        .join("lib/taskmanager/gpu")
        .join(flavor);
    let libraries: [(&str, &[&CStr]); 3] = [
        (
            "libdrm.so.2",
            &[
                c"drmGetDevices",
                c"drmFreeDevices",
                c"drmGetVersion",
                c"drmFreeVersion",
                c"drmGetMagic",
                c"drmAuthMagic",
                c"drmDropMaster",
            ],
        ),
        (
            "libdrm_amdgpu.so.1",
            &[
                c"amdgpu_device_initialize",
                c"amdgpu_device_deinitialize",
                c"amdgpu_get_marketing_name",
                c"amdgpu_query_hw_ip_info",
                c"amdgpu_query_info",
                c"amdgpu_query_gpu_info",
                c"amdgpu_query_sensor_info",
            ],
        ),
        (
            "libudev.so.1",
            &[
                c"udev_new",
                c"udev_unref",
                c"udev_enumerate_new",
                c"udev_enumerate_unref",
                c"udev_enumerate_add_match_subsystem",
                c"udev_enumerate_scan_devices",
                c"udev_enumerate_get_list_entry",
                c"udev_list_entry_get_next",
                c"udev_list_entry_get_name",
                c"udev_device_new_from_syspath",
                c"udev_device_get_devnode",
                c"udev_device_get_property_value",
                c"udev_device_get_parent",
                c"udev_device_get_sysattr_value",
                c"udev_device_unref",
            ],
        ),
    ];
    for (soname, symbols) in libraries {
        let path = directory.join(soname);
        let path_c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| magpie_platform::Error::msg("Invalid GPU library path"))?;
        unsafe {
            // An absolute private path prevents a missing bundle from passing
            // accidentally using a system copy. NOW checks all relocations.
            let handle = libc::dlopen(path_c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if handle.is_null() {
                let error = libc::dlerror();
                let detail = if error.is_null() {
                    "unknown loader error".into()
                } else {
                    CStr::from_ptr(error).to_string_lossy().into_owned()
                };
                return Err(magpie_platform::Error::msg(format!(
                    "GPU bootstrap {}: {detail}",
                    path.display()
                )));
            }
            for symbol in symbols {
                if libc::dlsym(handle, symbol.as_ptr()).is_null() {
                    libc::dlclose(handle);
                    return Err(magpie_platform::Error::msg(format!(
                        "GPU bootstrap {soname}: missing ABI symbol {}",
                        symbol.to_string_lossy()
                    )));
                }
            }
            libc::dlclose(handle);
        }
    }
    Ok(())
}

/// Native current-thread clock only. It excludes runtime/worker peers and
/// forked probe CPU. A failed clock is unavailable, never a fabricated zero.
#[cfg(feature = "diagnostic-profile")]
fn profile_thread_cpu_ns() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: initialized writable timespec; this constant requests only
        // the calling thread, without constructing clocks for arbitrary TIDs.
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) } != 0 {
            return None;
        }
        let seconds = u64::try_from(time.tv_sec).ok()?;
        let nanos = u64::try_from(time.tv_nsec).ok()?;
        seconds.checked_mul(1_000_000_000)?.checked_add(nanos)
    }
    #[cfg(not(target_os = "linux"))]
    None
}

fn main() -> magpie_platform::Result<()> {
    #[derive(Parser, Debug)]
    #[command(version, about, long_about = None)]
    struct Args {
        #[arg(long)]
        addr: Option<String>,

        #[arg(long)]
        test: bool,
    }

    footprint::configure();

    let mut args = Args::parse();
    if args.test {
        #[cfg(target_os = "linux")]
        test_packaged_gpu_libraries()?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, SIGKILL);
    }

    env_logger::init();
    #[cfg(feature = "diagnostic-profile")]
    let _profile_reporter = Reporter::start(profile_thread_cpu_ns);
    unsafe { nng::nng_log_set_logger(Some(nng::nng_log_fn)) };
    // A profile-only trace filter must not enable verbose native NNG logging.
    let nng_level = if log::log_enabled!(target: "magpie::nng", log::Level::Debug)
        || log::log_enabled!(target: "magpie::nng", log::Level::Trace)
    {
        nng::NNG_LOG_DEBUG
    } else if log::log_enabled!(target: "magpie::nng", log::Level::Info) {
        nng::NNG_LOG_INFO
    } else if log::log_enabled!(target: "magpie::nng", log::Level::Warn) {
        nng::NNG_LOG_WARN
    } else if log::log_enabled!(target: "magpie::nng", log::Level::Error) {
        nng::NNG_LOG_ERR
    } else {
        nng::NNG_LOG_NONE
    };
    unsafe { nng::nng_log_set_level(nng_level) };

    let addr = match std::mem::take(&mut args.addr) {
        Some(mut addr) => {
            addr.push('\0');
            addr
        }
        None => {
            return Err(magpie_platform::Error::msg(
                "No address provided, use `--addr` flag",
            ));
        }
    };

    let socket = bind(&addr)?;

    const WARM_UP_DEADLINE: Duration = Duration::from_secs(3);
    {
        let now = std::time::Instant::now();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

        std::thread::spawn(move || {
            let data_cache = data_cache();

            // Refresh the synchronous data sets before signaling completion.
            std::thread::scope(|s| {
                s.spawn(|| data_cache.refresh_cpu());
                s.spawn(|| data_cache.refresh_memory());
                s.spawn(|| {
                    data_cache.refresh_processes();
                    // Application matching needs this first process map, but not disks/network.
                    data_cache.refresh_apps_async();
                });
                s.spawn(|| data_cache.refresh_batteries());
                s.spawn(|| data_cache.refresh_network());
                s.spawn(|| data_cache.refresh_fans());
            });

            // Publish the small core sample first, then discover slower domains on existing
            // workers. Starting driver/fdinfo probes earlier competes with the initial /proc
            // scan and delays both its data and the GUI's first frame.
            data_cache.refresh_gpus_async();
            data_cache.refresh_services_async();
            data_cache.refresh_disks_async();
            let _ = done_tx.send(());
        });

        match done_rx.recv_timeout(WARM_UP_DEADLINE) {
            Ok(()) => log::info!("PERF: Initial data cache refresh took {:?}", now.elapsed()),
            Err(_) => log::warn!(
                "Initial data cache refresh did not finish within {WARM_UP_DEADLINE:?}; \
                 serving with partial data (it will keep populating in the background)"
            ),
        }
    }

    let stop_requested = Arc::new(AtomicBool::new(false));
    serve(socket, stop_requested)
}
