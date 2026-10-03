//! Colony lifecycle tuning constants: mortality, corpses, necrophoresis and
//! the post-delivery rest budget.

use crate::constants::ant::{ANT_SPEED, CARRY_SPEED_FACTOR};

/// Background mortality hazard per second for ants outside the nest:
/// `P_die = 1 - exp(-MORTALITY_HAZARD * dt)`.
///
/// ~0.1 %/s, i.e. ~8.6 % over a 90 s median life: predation, desiccation and
/// getting lost are visible without dominating the age-based lifetime. The
/// draw comes from the per-ant [`crate::simulation::ant::AntRng`], so the
/// hazard is fully deterministic.
pub const MORTALITY_HAZARD: f32 = 1e-3;

/// Seconds a corpse stays in the world before it decays and despawns.
pub const CORPSE_TTL: f32 = 120.0;
/// Center-to-center distance at which a foraging ant picks up a corpse.
pub const CORPSE_PICKUP_RADIUS: f32 = 3.0;
/// Distance from the refuse point at which a carried corpse is dropped.
pub const CORPSE_DROP_RADIUS: f32 = 5.0;
/// Walking speed while dragging a corpse (the same penalty as a full crop).
pub const CORPSE_CARRY_SPEED: f32 = ANT_SPEED * CARRY_SPEED_FACTOR;
/// Draw order of corpse sprites: just under the live ants.
pub const CORPSE_Z: f32 = crate::core::layers::Z_ANT - 0.01;

/// Probability that a forager rests after a dropoff.
pub const REST_AFTER: f32 = 0.35;
/// Seconds of rest after a dropoff. The navigation stream's `move_ants` skips
/// ants whose `rest_timer` is positive.
pub const REST_DURATION: f32 = 4.0;
