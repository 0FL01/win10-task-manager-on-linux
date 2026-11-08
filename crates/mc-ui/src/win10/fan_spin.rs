/* crates/mc-ui/src/win10/fan_spin.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Rotor motion of the Cooling fan glyphs (own design): speed follows RPM, changes are eased,
//! and a stopping rotor comes to rest on a quarter turn so the static glyph stays pixel-exact.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use super::metrics as m;

/// The rotor has four identical blades: a quarter turn looks the same.
const PERIOD: f32 = FRAC_PI_2;
/// A stopping rotor slower than this keeps this speed until it reaches the next quarter turn
const CRAWL: f32 = 0.2 * TAU;
/// Longest step; longer gaps (stalls, a hidden window) do not jump the rotor
pub const MAX_DT: f32 = 0.1;

/// Rotor speed in rad/s for a fan reading; stopped and unknown fans do not turn.
pub fn target_omega(rpm: Option<u32>) -> f32 {
    let rps = rpm.unwrap_or(0) as f32 / 60. * m::FAN_SPIN_RATIO;
    rps.min(m::FAN_SPIN_MAX_RPS) * TAU
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FanSpin {
    phase: f32,
    omega: f32,
}

impl FanSpin {
    /// Rotor angle in radians, `0..PERIOD`.
    pub fn angle(&self) -> f32 {
        self.phase
    }

    pub fn omega(&self) -> f32 {
        self.omega
    }

    /// Advances `dt` seconds towards `target` rad/s. Returns `true` while the rotor moves or
    /// is about to, i.e. while another frame is needed.
    pub fn step(&mut self, target: f32, dt: f32) -> bool {
        let dt = dt.clamp(0., MAX_DT);
        let target = target.max(0.);
        if target == 0. && self.omega == 0. && self.phase == 0. {
            return false;
        }
        // Exact exponential approach, so the motion does not depend on the frame rate
        let decay = (-dt / m::FAN_SPIN_EASE_S).exp();
        let start = self.omega;
        let mut omega = target + (start - target) * decay;
        let mut advance = target * dt + (start - target) * m::FAN_SPIN_EASE_S * (1. - decay);
        if target == 0. && omega < CRAWL {
            omega = CRAWL;
            advance = advance.max(CRAWL * dt);
            if self.phase + advance >= PERIOD {
                *self = Self::default();
                return false;
            }
        }
        self.omega = omega;
        self.phase = (self.phase + advance).rem_euclid(PERIOD);
        true
    }
}

/// Remembered rotors before unseen ones (scrolled away, removed fans) are dropped
const MAX_SPINS: usize = 64;

/// Shared clock and per-glyph rotor state. Rotors advance only while their glyph is painted
/// on screen; a frame is requested only while one of them moves.
#[derive(Default)]
pub struct FanAnimator {
    spins: HashMap<String, (FanSpin, u64)>,
    frame: u64,
    last_time: Option<f64>,
    dt: f32,
    enabled: bool,
    paused: bool,
    moving: bool,
    was_moving: bool,
}

impl FanAnimator {
    /// Starts a UI pass at input time `now` (seconds).
    pub fn begin(&mut self, now: f64, enabled: bool, paused: bool) {
        // Time only counts while something moved; an idle gap must not jump the rotors.
        self.dt = match self.last_time {
            Some(last) if self.was_moving || self.moving => (now - last).max(0.) as f32,
            _ => 0.,
        };
        self.was_moving = self.moving;
        self.moving = false;
        self.last_time = Some(now);
        self.frame += 1;
        self.enabled = enabled;
        self.paused = paused;
    }

    /// Rotor angle of a glyph painted on screen this pass, advanced to the pass's time.
    /// Disabled animation draws the static glyph; paused updates let rotors come to rest.
    pub fn angle(&mut self, key: &str, rpm: Option<u32>) -> f32 {
        if !self.enabled {
            self.spins.remove(key);
            return 0.;
        }
        let target = if self.paused { 0. } else { target_omega(rpm) };
        let frame = self.frame;
        let entry = match self.spins.get_mut(key) {
            Some(entry) => entry,
            None => {
                if target == 0. {
                    return 0.;
                }
                self.prune();
                self.spins
                    .entry(key.to_owned())
                    .or_insert((FanSpin::default(), frame))
            }
        };
        // Several passes of one frame share its time: only the first advances the rotor.
        let dt = if entry.1 == frame && entry.0 != FanSpin::default() {
            0.
        } else {
            self.dt
        };
        entry.1 = frame;
        self.moving |= entry.0.step(target, dt);
        entry.0.angle()
    }

    fn prune(&mut self) {
        if self.spins.len() >= MAX_SPINS {
            let frame = self.frame;
            self.spins
                .retain(|_, (spin, seen)| *seen == frame || spin.omega() > 0.);
            if self.spins.len() >= MAX_SPINS {
                self.spins.clear();
            }
        }
    }

    /// Whether a visible rotor moved in this pass.
    pub fn moving(&self) -> bool {
        self.moving
    }

    /// Ends the pass: requests the next frame while a visible rotor moves. The presenter's
    /// vsync paces these frames; the interval only bounds presenters without one.
    pub fn end(&self, ctx: &egui::Context) {
        if self.moving {
            ctx.request_repaint_after(Duration::from_secs_f32(m::FAN_SPIN_MIN_FRAME_S));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(spin: &mut FanSpin, target: f32, seconds: f32, fps: f32) {
        let frames = (seconds * fps).round() as usize;
        for _ in 0..frames {
            spin.step(target, 1. / fps);
        }
    }

    #[test]
    fn speed_mapping_is_linear_and_capped() {
        assert_eq!(target_omega(None), 0.);
        assert_eq!(target_omega(Some(0)), 0.);
        assert!(
            (target_omega(Some(1200)) - TAU).abs() < 1e-5,
            "1200 RPM → 1 rev/s"
        );
        assert!((target_omega(Some(100_000)) - 3. * TAU).abs() < 1e-5);
    }

    #[test]
    fn motion_does_not_depend_on_the_frame_rate() {
        let target = target_omega(Some(1500));
        let (mut slow, mut fast) = (FanSpin::default(), FanSpin::default());
        run(&mut slow, target, 1., 60.);
        run(&mut fast, target, 1., 144.);
        let unwrap = |d: f32| (d + PERIOD / 2.).rem_euclid(PERIOD) - PERIOD / 2.;
        assert!(unwrap(slow.angle() - fast.angle()).abs() < 1e-3);
        assert!((slow.omega() - fast.omega()).abs() < 1e-3);
    }

    #[test]
    fn easing_approaches_the_target_without_overshoot() {
        let target = target_omega(Some(2400));
        let mut spin = FanSpin::default();
        let mut previous = 0.;
        for _ in 0..120 {
            spin.step(target, 1. / 60.);
            assert!(spin.omega() >= previous && spin.omega() <= target);
            previous = spin.omega();
        }
        assert!((spin.omega() - target).abs() / target < 0.01);
    }

    #[test]
    fn stopping_rests_exactly_on_a_quarter_turn() {
        let mut spin = FanSpin::default();
        run(&mut spin, target_omega(Some(3000)), 0.73, 60.);
        assert!(spin.angle() > 0.);
        let mut frames = 0;
        while spin.step(0., 1. / 60.) {
            frames += 1;
            assert!(frames < 600, "settles in bounded time");
        }
        assert_eq!(spin, FanSpin::default());
        assert!(!spin.step(0., 1. / 60.), "a resting rotor needs no frames");
    }

    #[test]
    fn animator_requests_frames_only_while_moving() {
        let mut animator = FanAnimator::default();
        animator.begin(0., true, false);
        assert_eq!(animator.angle("fan:a", Some(0)), 0.);
        assert!(!animator.moving(), "a stopped fan needs no frames");
        animator.angle("fan:b", Some(1200));
        assert!(animator.moving());
        animator.begin(0.25, true, false);
        let quarter = animator.angle("fan:b", Some(1200));
        assert!(quarter > 0.);
        // A second pass of the same frame does not advance the rotor
        animator.begin(0.25, true, false);
        assert_eq!(animator.angle("fan:b", Some(1200)), quarter);

        let mut time = 0.25;
        animator.begin(time, true, true);
        while {
            animator.angle("fan:b", Some(1200));
            animator.moving()
        } {
            time += 1. / 60.;
            animator.begin(time, true, true);
            assert!(time < 10., "paused rotors come to rest");
        }
        assert_eq!(animator.angle("fan:b", Some(1200)), 0.);

        animator.begin(time + 1., false, false);
        assert_eq!(animator.angle("fan:b", Some(3000)), 0.);
        assert!(!animator.moving(), "disabled animation is static");
    }

    #[test]
    fn idle_gaps_do_not_jump_a_starting_rotor() {
        let mut animator = FanAnimator::default();
        animator.begin(0., true, false);
        animator.angle("fan:a", Some(0));
        // Ten idle seconds later the fan starts: its first frame has no elapsed time.
        animator.begin(10., true, false);
        assert_eq!(animator.angle("fan:a", Some(2400)), 0.);
        animator.begin(10. + 1. / 60., true, false);
        assert!(animator.angle("fan:a", Some(2400)) < 0.1);
    }

    #[test]
    fn long_gaps_are_clamped() {
        let target = target_omega(Some(3600));
        let mut a = FanSpin::default();
        let mut b = FanSpin::default();
        a.step(target, 5.);
        b.step(target, MAX_DT);
        assert_eq!(a, b);
    }
}
