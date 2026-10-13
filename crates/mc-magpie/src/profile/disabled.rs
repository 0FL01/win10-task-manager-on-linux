/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::marker::PhantomData;
use std::rc::Rc;

use super::{Counter, Phase};

#[inline]
pub fn enabled() -> bool {
    false
}

#[inline]
pub fn count(_counter: Counter) {}

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

pub struct Reporter;

impl Reporter {
    #[inline]
    pub fn start() -> Self {
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
        let reporter = Reporter::start();
        assert!(!enabled());
        let mut scope = Span::new(Phase::Startup);
        scope.items(usize::MAX);
        for counter in [
            Counter::Published,
            Counter::Overwritten,
            Counter::Applied,
            Counter::Overrun,
            Counter::RequestFull,
            Counter::EventFull,
        ] {
            count(counter);
        }
        drop(scope);
        drop(reporter);
        assert!(!enabled());
        assert_eq!(std::mem::size_of::<Span>(), 0);
        assert_eq!(std::mem::size_of::<Reporter>(), 0);
        assert!(std::mem::needs_drop::<Span>());
    }
}
