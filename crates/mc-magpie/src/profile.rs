/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! UI/client profiling API. Without `diagnostic-profile`, scopes and counters are
//! zero-sized no-ops: no clocks, aggregates or reporter machinery are compiled.
//! Diagnostic builds also require the `TaskManager::Profile=trace` log target.
//! Timings are inclusive; thread CPU excludes peers, the daemon and GPU work.
//!
//! Scopes remain same-thread guards in either build mode:
//! ```compile_fail
//! use mc_magpie::profile::{Phase, Span};
//! let scope = Span::new(Phase::Startup);
//! std::thread::spawn(move || drop(scope));
//! ```

#[cfg(feature = "diagnostic-profile")]
mod diagnostic;
#[cfg(feature = "diagnostic-profile")]
pub use diagnostic::{count, enabled, Reporter, Span};

#[cfg(not(feature = "diagnostic-profile"))]
mod disabled;
#[cfg(not(feature = "diagnostic-profile"))]
pub use disabled::{count, enabled, Reporter, Span};

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Phase {
    Startup,
    AppInit,
    Logic,
    Apply,
    GpuSnapshot,
    PerformanceUpdate,
    AppsUpdate,
    ServicesUpdate,
    DetailsUpdate,
    ServicesViewUpdate,
    TableTree,
    TableGpu,
    TableTotals,
    TableFormat,
    TableFlatten,
    Ui,
    ProcessesDraw,
    PerformanceDraw,
    DetailsDraw,
    ServicesDraw,
    DialogsDraw,
    IconDecode,
    IconUpload,
    GatherStartup,
    GatherCycle,
    Processes,
    Users,
    Sockets,
    Apps,
    Disks,
    Gpus,
    Cpu,
    Memory,
    Network,
    Fans,
    Batteries,
    UserServices,
    SystemServices,
    GatherIcons,
    DiskSort,
    Action,
    IpcEncode,
    IpcSend,
    IpcWait,
    IpcDecode,
    SocketDump,
    SocketOwners,
}

#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Counter {
    Published,
    Overwritten,
    Applied,
    Overrun,
    RequestFull,
    EventFull,
}
