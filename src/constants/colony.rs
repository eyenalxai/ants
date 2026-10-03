//! Colony food-economy tunables: the nest store, metered refill, nurse upkeep,
//! the recruitment gate and the queen/brood food costs.
//!
//! The store closes the foraging loop: deliveries are the only income, refills,
//! nursing, egg laying and larval feeding are the only expenses. A colony that
//! stops foraging drains the store, recruitment closes and the population
//! shrinks.

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

// --- Queen and brood economy (F8) ---------------------------------------------
//
// The queen replaces the old direct adult recruitment: she lays eggs while the
// store is above [`QUEEN_LAYING_THRESHOLD`], each egg costs [`EGG_FOOD_COST`]
// at laying and each larva draws [`LARVA_FOOD_PER_SEC`] from the store while it
// grows. Pupae need no food.

/// Eggs the queen lays per second at full activity and a fully open store.
pub const QUEEN_EGG_RATE: f32 = 4.0;
/// Store level above which the queen lays at all. Below it the colony is
/// starving and reproduction stops.
pub const QUEEN_LAYING_THRESHOLD: f32 = RECRUIT_THRESHOLD;
/// Store level at which the queen reaches her full [`QUEEN_EGG_RATE`]. Between
/// the threshold and this level the rate scales linearly with the store, so a
/// lean colony lays fewer eggs instead of none.
pub const QUEEN_LAYING_FULL_STORE: f32 = 100.0;
/// Food spent from the nest store to lay one egg.
pub const EGG_FOOD_COST: f32 = 0.05;
/// Food a larva consumes per second. A larva whose payment the store cannot
/// cover stalls until food returns.
pub const LARVA_FOOD_PER_SEC: f32 = 0.02;

// --- Individual variation (F7) -------------------------------------------------
//
// Every range below is drawn uniformly from the ant's own seeded `AntRng` at
// spawn, so a run stays bit-identical while individuals differ in behavior.
// The navigation stream consumes `pi_bias`, `sensor_gain` and
// `explore_tendency`; the colony stream consumes `forage_threshold` and
// `crop_capacity`.

/// Minimum magnitude of the fixed path-integration heading error, degrees.
pub const PI_BIAS_MIN_DEG: f32 = 5.0;
/// Maximum magnitude of the fixed path-integration heading error, degrees.
pub const PI_BIAS_MAX_DEG: f32 = 10.0;
/// Minimum individual exploration multiplier.
pub const EXPLORE_TENDENCY_MIN: f32 = 0.5;
/// Maximum individual exploration multiplier.
pub const EXPLORE_TENDENCY_MAX: f32 = 1.5;
/// Minimum individual sensor gain.
pub const SENSOR_GAIN_MIN: f32 = 0.7;
/// Maximum individual sensor gain.
pub const SENSOR_GAIN_MAX: f32 = 1.3;
/// Minimum task-allocation response threshold.
pub const FORAGE_THRESHOLD_MIN: f32 = 0.3;
/// Maximum task-allocation response threshold.
pub const FORAGE_THRESHOLD_MAX: f32 = 0.9;
/// Minimum individual crop capacity (full loads are `CARRY_AMOUNT`-sized).
pub const CROP_CAPACITY_MIN: f32 = 0.8;
/// Maximum individual crop capacity.
pub const CROP_CAPACITY_MAX: f32 = 1.2;

// --- Flexible task allocation (F7) --------------------------------------------

/// Share of the colony that should be foraging; below it the forager shortage
/// term of the stimulus rises toward 1.
pub const TARGET_FORAGER_FRACTION: f32 = 0.7;
/// Nurse share below which a satiated colony may push foragers back to
/// nursing.
pub const REVERSION_NURSE_RATIO: f32 = 0.15;
/// Stimulus below which reversion is allowed (satiated, no forager shortage).
pub const REVERSION_STIMULUS: f32 = 0.15;
/// Per-second probability that an eligible forager reverts to nursing.
pub const FORAGER_REVERSION_RATE: f32 = 0.005;
