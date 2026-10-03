//! Colony food-economy tunables: the nest store, metered refill, nurse upkeep
//! and the recruitment gate.
//!
//! The store closes the foraging loop: deliveries are the only income, refills
//! and nursing are the only expenses. A colony that stops foraging drains the
//! store, recruitment closes and the population shrinks.

/// Maximum food held by the nest store. Deliveries beyond the cap are still
/// counted in [`crate::simulation::colony::ColonyStats`], they just overflow.
pub const NEST_STORE_CAP: f32 = 1000.0;
/// Bootstrap food in the nest at startup, before the first delivery arrives.
/// Enough to run the initial nurses and refill early returners, small enough
/// that a colony which never forages starves.
pub const NEST_STORE_INITIAL: f32 = 150.0;

/// Food consumed per energy unit refilled inside the nest.
pub const ENERGY_REFILL_COST: f32 = 0.1;
/// Energy units restored per second while an ant refills in the nest. The
/// refill is a per-second rate, so behavior does not depend on the tick rate.
pub const ENERGY_REFILL_RATE: f32 = 0.5;
/// Energy above this counts as a full tank; avoids a permanent one-ULP
/// shortfall when the last refill step lands just below `1.0`.
pub const ENERGY_FULL_EPS: f32 = 1e-3;

/// Food per second consumed by each nursing ant. Nurses do not forage, so the
/// brood is a pure expense until it matures.
pub const NURSE_UPKEEP_PER_ANT: f32 = 0.0015;

/// Store level at which recruitment is fully open. Below it the spawn batch is
/// scaled linearly with the store; at zero the colony stops recruiting.
pub const RECRUIT_THRESHOLD: f32 = 25.0;
