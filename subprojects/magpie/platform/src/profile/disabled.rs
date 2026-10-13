/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::marker::PhantomData;
use std::rc::Rc;

use super::{Clock, Domain, MetricError, Phase, ProcError};

#[inline]
pub fn enabled() -> bool {
    false
}

/// Same-thread guard even when timing is compiled out.
pub struct Span(PhantomData<Rc<()>>);

impl Span {
    #[inline]
    pub fn new(_phase: Phase) -> Self {
        Self(PhantomData)
    }

    #[inline]
    pub fn items(&mut self, _items: usize) {}
}

impl Drop for Span {
    #[inline]
    fn drop(&mut self) {}
}

pub struct QueueTicket;

impl QueueTicket {
    #[inline]
    pub fn new(_domain: Domain) -> Self {
        Self
    }

    #[inline]
    pub fn arrived(self) {}
}

#[inline]
pub fn coalesced(_phase: Phase) {}

/// False retains the caller's original warning/error logging.
#[inline]
pub fn proc_error(_category: ProcError) -> bool {
    false
}

#[inline]
pub fn proc_io_error(_error: &std::io::Error) -> bool {
    false
}

/// False retains the caller's original warning/error logging.
#[inline]
pub fn metric_error(_category: MetricError) -> bool {
    false
}

pub struct Reporter;

impl Reporter {
    /// The native clock is deliberately never called or stored.
    #[inline]
    pub fn start(_clock: Clock) -> Self {
        Self
    }
}

impl Drop for Reporter {
    #[inline]
    fn drop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiling_is_compiled_out_even_with_trace_logging() {
        log::set_max_level(log::LevelFilter::Trace);
        let reporter = Reporter::start(|| panic!("compiled-out native CPU clock"));
        assert!(!enabled());
        let mut scope = Span::new(Phase::Startup);
        scope.items(usize::MAX);
        QueueTicket::new(Domain::Cpu).arrived();
        coalesced(Phase::Refresh(Domain::Cpu));
        drop(scope);
        drop(reporter);
        assert!(!enabled());
        assert_eq!(std::mem::size_of::<Span>(), 0);
        assert_eq!(std::mem::size_of::<Reporter>(), 0);
        assert_eq!(std::mem::size_of::<QueueTicket>(), 0);
        assert!(std::mem::needs_drop::<Span>());
    }

    #[test]
    fn compiled_out_error_helpers_preserve_original_logs() {
        for category in [
            ProcError::Exited,
            ProcError::Permission,
            ProcError::Empty,
            ProcError::Invalid,
            ProcError::Unexpected,
            ProcError::Root,
        ] {
            assert!(!proc_error(category));
        }
        for kind in [
            std::io::ErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::InvalidData,
        ] {
            assert!(!proc_io_error(&std::io::Error::from(kind)));
        }
        for category in [
            MetricError::Read,
            MetricError::Value,
            MetricError::NetworkType,
        ] {
            assert!(!metric_error(category));
        }
    }
}
