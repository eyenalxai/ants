//! Environment state: the simulation clock and its activity factor.
//!
//! [`SimClock::t`] advances one fixed step at a time in
//! [`crate::core::sets::SimSet::Clock`] and [`SimClock::activity`] follows a
//! smoothed day-night sinusoid over [`DAY_LENGTH`] with a floor of
//! [`ACTIVITY_MIN`]. The cycle starts at peak activity, so the short
//! regression runs (45 s trail check, 15 s determinism replay) see essentially
//! full activity while a longer session experiences dusk and dawn.
//!
//! Temperature is deliberately not modelled yet: activity is a pure,
//! deterministic function of `t` (no randomness, no hidden state), which keeps
//! replays bit-identical. It is the simple, documented version of F14.
//!
//! Obstacles and terrain (F16) are staged for a later milestone. When they
//! land, the obstacle data (`Obstacles` resource: circles/AABBs) will live in
//! a new `simulation/obstacles.rs` module owned by this stream, with its
//! tuning in [`crate::constants::environment`]; collision response stays in
//! the movement stream and sensor occlusion in the pheromone stream, because
//! they own those files. Nothing here should grow F16 behavior before that
//! milestone.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::constants::environment::{ACTIVITY_MIN, DAY_LENGTH};
use crate::core::sets::SimSet;

/// Simulated wall-clock time and the current foraging-activity factor.
#[derive(Resource)]
pub struct SimClock {
    /// Seconds of simulated time elapsed.
    pub t: f32,
    /// Foraging-activity multiplier in `[ACTIVITY_MIN, 1.0]`, recomputed from
    /// [`SimClock::t`] every fixed step.
    pub activity: f32,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            t: 0.0,
            // The cycle starts at its peak, so `default()` matches
            // `activity_at(0.0)` and the clock is neutral (1.0) until the first
            // fixed step.
            activity: 1.0,
        }
    }
}

/// Foraging activity at simulated time `t` seconds.
///
/// A raised cosine over [`DAY_LENGTH`] starting at peak (`t = 0` is noon,
/// `t = DAY_LENGTH / 2` is midnight), shaped by a smoothstep so activity
/// plateaus near its peak and near the floor instead of swinging linearly
/// through the middle. The result is always in `[ACTIVITY_MIN, 1.0]` and is
/// exactly periodic with [`DAY_LENGTH`].
///
/// `rem_euclid` folds `t` into one cycle, so precision does not degrade as
/// the session grows and the function stays periodic for arbitrarily large
/// `t`.
pub fn activity_at(t: f32) -> f32 {
    let phase = (t / DAY_LENGTH).rem_euclid(1.0);
    let wave = 0.5 + 0.5 * (TAU * phase).cos();
    let smoothed = wave * wave * (3.0 - 2.0 * wave);

    ACTIVITY_MIN + (1.0 - ACTIVITY_MIN) * smoothed
}

/// Advance [`SimClock::t`] by one fixed step and recompute
/// [`SimClock::activity`] from it.
///
/// Reading [`Time<Fixed>`] keeps the clock stepped exactly once per simulation
/// tick (64 Hz), independent of frame rate, and pauses with the simulation.
fn advance_sim_clock(time: Res<Time<Fixed>>, mut clock: ResMut<SimClock>) {
    clock.t += time.delta_secs();
    clock.activity = activity_at(clock.t);
}

/// Wiring hook for the environment stream: registers the clock/activity system
/// in [`SimSet::Clock`], the first set of the fixed-step chain.
pub fn register(app: &mut App) {
    app.add_systems(FixedUpdate, advance_sim_clock.in_set(SimSet::Clock));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    /// Duration of one 64 Hz fixed step.
    const STEP: f32 = 1.0 / 64.0;

    #[test]
    fn activity_is_bounded_by_the_floor_and_the_peak() {
        let samples = 2048;
        let mut min = f32::MAX;
        let mut max = f32::MIN;

        // Two full cycles sampled densely enough to hit both extrema.
        for i in 0..samples {
            let t = 2.0 * DAY_LENGTH * i as f32 / samples as f32;
            let activity = activity_at(t);

            assert!(
                (ACTIVITY_MIN..=1.0).contains(&activity),
                "activity {activity} out of [ACTIVITY_MIN, 1] at t = {t}"
            );

            min = min.min(activity);
            max = max.max(activity);
        }

        assert!(
            (min - ACTIVITY_MIN).abs() < 1e-4,
            "the floor must be reached, got min {min}"
        );
        assert!(
            (max - 1.0).abs() < 1e-4,
            "the peak must be reached, got max {max}"
        );
    }

    #[test]
    fn activity_is_periodic_and_a_full_cycle_returns_to_the_start() {
        assert!(
            (activity_at(DAY_LENGTH) - activity_at(0.0)).abs() < 1e-6,
            "one full cycle must return to the start value"
        );

        for i in 0..=64 {
            let t = DAY_LENGTH * i as f32 / 64.0;
            let next = t + DAY_LENGTH;

            assert!(
                (activity_at(t) - activity_at(next)).abs() < 1e-4,
                "activity at {t} and {next} must match"
            );
        }
    }

    #[test]
    fn clock_advances_in_fixed_steps_and_tracks_activity() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                STEP,
            )))
            .init_resource::<SimClock>();
        register(&mut app);

        let before = app.world().resource::<SimClock>().t;
        for _ in 0..64 {
            app.update();
        }

        let clock = app.world().resource::<SimClock>();
        assert!(clock.t > before, "t must advance every fixed step");
        assert!(
            (clock.t - 1.0).abs() < 0.05,
            "64 steps at 64 Hz should advance t by ~1 s, got {}",
            clock.t
        );
        assert!(
            (clock.activity - activity_at(clock.t)).abs() < 1e-6,
            "activity must be recomputed from t, got {} for t {}",
            clock.activity,
            clock.t
        );
    }
}
