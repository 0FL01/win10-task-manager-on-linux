/* crates/mc-ui/src/always_on_top.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Always-on-top request/acknowledgement state. Native window control lives in the shell.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlwaysOnTopMode {
    Viewport,
    External,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlwaysOnTopRequest {
    pub id: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlwaysOnTopError {
    Unavailable,
    NotFound,
    Ambiguous,
    Rejected,
    TimedOut,
    Failed,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlwaysOnTopResult {
    pub id: u64,
    pub actual: Option<bool>,
    pub error: Option<AlwaysOnTopError>,
}

pub(crate) struct AlwaysOnTopState {
    pub mode: AlwaysOnTopMode,
    pub actual: Option<bool>,
    pub error: Option<AlwaysOnTopError>,
    pending: Option<AlwaysOnTopRequest>,
    request_ready: bool,
    next_id: u64,
}

impl AlwaysOnTopState {
    pub fn new(mode: AlwaysOnTopMode, desired: bool) -> Self {
        let mut state = Self {
            mode,
            actual: None,
            error: (mode == AlwaysOnTopMode::Unavailable).then_some(AlwaysOnTopError::Unavailable),
            pending: None,
            request_ready: false,
            next_id: 1,
        };
        if mode == AlwaysOnTopMode::External {
            // Restore even `false`: the compositor may remember a previous keep-above rule.
            state.queue(desired);
        }
        state
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn enabled(&self) -> bool {
        self.mode == AlwaysOnTopMode::External
            && !self.pending()
            && !matches!(
                self.error,
                Some(AlwaysOnTopError::Unavailable | AlwaysOnTopError::Cleanup)
            )
    }

    fn queue(&mut self, enabled: bool) {
        self.pending = Some(AlwaysOnTopRequest {
            id: self.next_id,
            enabled,
        });
        self.next_id += 1;
        self.request_ready = true;
        self.error = None;
    }

    pub fn request(&mut self, enabled: bool) {
        if self.enabled() {
            self.queue(enabled);
        }
    }

    pub fn take_request(&mut self) -> Option<AlwaysOnTopRequest> {
        if !self.request_ready {
            return None;
        }
        self.request_ready = false;
        self.pending
    }

    /// Returns a preference to persist only after the requested value has been verified.
    pub fn finish(&mut self, result: AlwaysOnTopResult) -> Option<bool> {
        let pending = self.pending.filter(|pending| pending.id == result.id)?;
        self.pending = None;
        self.request_ready = false;
        self.actual = result.actual;
        let confirmed = result.actual == Some(pending.enabled);
        self.error = result.error.or_else(|| {
            if confirmed {
                None
            } else if result.actual.is_some() {
                Some(AlwaysOnTopError::Rejected)
            } else {
                Some(AlwaysOnTopError::Failed)
            }
        });
        // Cleanup failure is special: the operation succeeded, but another script must not run.
        (confirmed && matches!(self.error, None | Some(AlwaysOnTopError::Cleanup)))
            .then_some(pending.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(
        request: AlwaysOnTopRequest,
        actual: Option<bool>,
        error: Option<AlwaysOnTopError>,
    ) -> AlwaysOnTopResult {
        AlwaysOnTopResult {
            id: request.id,
            actual,
            error,
        }
    }

    #[test]
    fn pending_requests_wait_for_confirmation() {
        let mut state = AlwaysOnTopState::new(AlwaysOnTopMode::External, false);
        assert_eq!(state.actual, None);
        assert!(!state.enabled());
        state.request(true);
        let initial = state.take_request().unwrap();
        assert!(!initial.enabled);
        assert!(state.pending());
        state.request(true);
        assert_eq!(state.take_request(), None);
        assert_eq!(
            state.finish(result(initial, Some(false), None)),
            Some(false)
        );
        assert!(state.enabled());

        state.request(true);
        let request = state.take_request().unwrap();
        assert!(request.id > initial.id);
        assert_eq!(state.actual, Some(false));
        assert!(!state.enabled());
        state.request(false);
        assert_eq!(state.take_request(), None);
        assert_eq!(state.finish(result(request, Some(true), None)), Some(true));
        assert_eq!(state.actual, Some(true));
        assert!(state.enabled());
    }

    #[test]
    fn failure_preserves_preference_and_clears_known_state() {
        let mut preference = true;
        let mut state = AlwaysOnTopState::new(AlwaysOnTopMode::External, preference);
        let initial = state.take_request().unwrap();
        preference = state
            .finish(result(initial, None, Some(AlwaysOnTopError::TimedOut)))
            .unwrap_or(preference);
        assert!(
            preference,
            "failed startup restoration must retain the preference"
        );
        assert_eq!(state.actual, None);
        assert_eq!(state.error, Some(AlwaysOnTopError::TimedOut));
        assert!(state.enabled());
        state.request(false);
        let request = state.take_request().unwrap();
        preference = state
            .finish(result(request, None, Some(AlwaysOnTopError::Failed)))
            .unwrap_or(preference);
        assert!(preference);
        assert_eq!(state.actual, None);
        assert!(state.enabled());
    }

    #[test]
    fn stale_results_do_not_finish_a_new_request() {
        let mut state = AlwaysOnTopState::new(AlwaysOnTopMode::External, false);
        let initial = state.take_request().unwrap();
        state.finish(result(initial, Some(false), None));
        state.request(true);
        let request = state.take_request().unwrap();
        assert_eq!(
            state.finish(result(initial, None, Some(AlwaysOnTopError::Failed))),
            None
        );
        assert_eq!(state.actual, Some(false));
        assert_eq!(state.error, None);
        assert!(state.pending());
        assert_eq!(state.finish(result(request, Some(true), None)), Some(true));
        assert_eq!(state.finish(result(request, Some(false), None)), None);
        assert_eq!(state.actual, Some(true));
    }

    #[test]
    fn rejected_value_is_shown_and_can_be_retried() {
        let mut state = AlwaysOnTopState::new(AlwaysOnTopMode::External, true);
        let initial = state.take_request().unwrap();
        assert_eq!(
            state.finish(result(
                initial,
                Some(false),
                Some(AlwaysOnTopError::Rejected)
            )),
            None
        );
        assert_eq!(state.actual, Some(false));
        assert_eq!(state.error, Some(AlwaysOnTopError::Rejected));
        assert!(state.enabled());
        state.request(true);
        assert_eq!(state.error, None);
        let request = state.take_request().unwrap();
        assert_eq!(state.finish(result(request, Some(true), None)), Some(true));
    }

    #[test]
    fn verified_cleanup_failure_persists_but_disables_further_requests() {
        let mut state = AlwaysOnTopState::new(AlwaysOnTopMode::External, true);
        let initial = state.take_request().unwrap();
        assert_eq!(
            state.finish(result(initial, Some(true), Some(AlwaysOnTopError::Cleanup))),
            Some(true)
        );
        assert_eq!(state.actual, Some(true));
        assert_eq!(state.error, Some(AlwaysOnTopError::Cleanup));
        assert!(!state.enabled());
        state.request(false);
        assert_eq!(state.take_request(), None);
    }

    #[test]
    fn other_modes_do_not_queue_external_requests() {
        for mode in [AlwaysOnTopMode::Viewport, AlwaysOnTopMode::Unavailable] {
            let mut state = AlwaysOnTopState::new(mode, true);
            assert_eq!(state.take_request(), None);
            state.request(false);
            assert_eq!(state.take_request(), None);
            assert_eq!(state.actual, None);
        }
    }
}
