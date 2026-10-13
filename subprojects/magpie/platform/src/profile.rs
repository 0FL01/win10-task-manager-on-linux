/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Daemon profiling API. Without `diagnostic-profile`, scopes, queue tickets and
//! error helpers are no-ops: no clocks, aggregates or reporter are compiled.
//! Diagnostic builds also require the `TaskManager::Profile=trace` log target.
//! Timings are inclusive; CPU excludes runtime peers and forked probe children.
//!
//! Scopes remain same-thread guards in either build mode:
//! ```compile_fail
//! use magpie_platform::profile::{Phase, Span};
//! let scope = Span::new(Phase::Startup);
//! std::thread::spawn(move || drop(scope));
//! ```

#[cfg(feature = "diagnostic-profile")]
mod diagnostic;
#[cfg(feature = "diagnostic-profile")]
pub use diagnostic::{
    coalesced, enabled, metric_error, proc_error, proc_io_error, QueueTicket, Reporter, Span,
};

#[cfg(not(feature = "diagnostic-profile"))]
mod disabled;
#[cfg(not(feature = "diagnostic-profile"))]
pub use disabled::{
    coalesced, enabled, metric_error, proc_error, proc_io_error, QueueTicket, Reporter, Span,
};

pub const TARGET: &str = "TaskManager::Profile";
type Clock = fn() -> Option<u64>;

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum Domain {
    About,
    Apps,
    Battery,
    Cpu,
    Disks,
    Fans,
    Gpus,
    Memory,
    Network,
    Processes,
    Services,
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
    Init(Domain),
    Request(Domain),
    Refresh(Domain),
    Publish(Domain),
    Queue(Domain),
    Startup,
    RequestTotal,
    ProcessEnumeration,
    ProcessMaps,
    ProcessScan,
    ProcessTree,
    ProcessGpuMerge,
    GpuInventory,
    GpuFdinfo,
    GpuNative,
    GpuNvml,
    GpuNativeInit,
    GpuForkedProbes,
    GpuNvmlInit,
    AppsInventory,
    AppsSnapshot,
    AppsMatching,
    AppsIcons,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ProcError {
    Exited,
    Permission,
    Empty,
    Invalid,
    Unexpected,
    Root,
}

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum MetricError {
    Read,
    Value,
    NetworkType,
}
