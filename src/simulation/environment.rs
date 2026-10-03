//! Environment state: the simulation clock and its activity factor.
//!
//! The environment stream fills this module (day-night cycle, temperature,
//! resting); [`register`] is the wiring hook its systems grow into.

use bevy::prelude::*;

/// Simulated wall-clock time and the current foraging-activity factor.
#[derive(Resource)]
pub struct SimClock {
    /// Seconds of simulated time elapsed.
    pub t: f32,
    /// Foraging-activity multiplier in `[0, 1]`; stays at `1.0` until the
    /// day-night cycle lands.
    pub activity: f32,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            t: 0.0,
            activity: 1.0,
        }
    }
}

/// Wiring hook for the environment stream: it registers clock/activity systems
/// in [`crate::core::sets::SimSet::Clock`]. Empty until that stream lands.
pub fn register(_app: &mut App) {}
