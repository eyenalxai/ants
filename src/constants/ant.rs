//! Tunables for ant behavior: movement, sensing, energy and lifecycle.

use std::f32::consts::PI;

pub const MAX_ANTS: usize = 30000;
pub const ANT_SPAWN_INTERVAL: f32 = 0.05;
pub const ANT_BATCH_SIZE: usize = 100;
pub const ANT_SPEED: f32 = 50.0;
pub const ANT_SIZE: f32 = 2.0;
pub const ANT_ALPHA: f32 = 0.005;
/// Base lifetime in seconds; variation spans [`ANT_LIFETIME_VARIATION_MIN`]
/// to `min + 1`. Long enough for several nest-food round trips.
pub const ANT_LIFETIME: f32 = 90.0;
pub const ANT_LIFETIME_VARIATION_MIN: f32 = 0.5;
pub const ANT_SPEED_VARIATION_MIN: f32 = 0.5;

pub const ANT_TURN_RATE: f32 = 9.0;
pub const ANT_RANDOM_TURN_CHANCE: f32 = 0.8;
pub const ANT_EXPLORATION_CHANCE: f32 = 0.2;
pub const ANT_PROBABILISTIC_STEERING_CHANCE: f32 = 0.6;
pub const ANT_STEERING_NOISE_FACTOR: f32 = 0.8;
pub const ANT_TURN_INTENSITY_BASE: f32 = 0.6;
pub const ANT_TURN_INTENSITY_SCALE: f32 = 0.4;
/// Jitter fraction of the max turn while approaching sensed food.
pub const FOOD_APPROACH_JITTER_FACTOR: f32 = 0.25;
/// Fraction of a turn step used by the nurses' random wander.
pub const NURSING_RANDOM_TURN_FACTOR: f32 = 0.5;

/// Laden ants move at this fraction of their base speed at a full crop.
///
/// The graded load law is `1 - LOAD_SLOWDOWN * (carrying / crop_capacity)`, so
/// a full load at the median crop capacity reproduces this factor exactly.
pub const CARRY_SPEED_FACTOR: f32 = 0.65;
/// Speed fraction lost at a full crop (`1 - CARRY_SPEED_FACTOR`).
pub const LOAD_SLOWDOWN: f32 = 1.0 - CARRY_SPEED_FACTOR;
/// Food taken from a cell per successful pickup.
pub const CARRY_AMOUNT: f32 = 1.0;
/// Seconds an ant stands still after picking up or dropping off food.
pub const HANDLING_TIME: f32 = 0.5;

/// Energy lost per second while walking; a full tank lasts ~125 s on the move,
/// enough to reach food and carry it back.
pub const ANT_ENERGY_DRAIN_RATE: f32 = 0.008;
/// Extra energy-drain multiplier while carrying food.
pub const ANT_CARRY_ENERGY_DRAIN_FACTOR: f32 = 1.3;
/// Below this fraction an ant abandons foraging and returns to the nest.
pub const ANT_ENERGY_RETURN_THRESHOLD: f32 = 0.25;
/// Fraction of `max_lifetime` spent nursing inside the nest.
pub const ANT_NURSING_LIFETIME_FRACTION: f32 = 0.1;
/// Nurses wander no farther than `NEST_RADIUS * this` from home.
pub const NURSING_LEASH_FACTOR: f32 = 2.0;
/// Nurses move at this fraction of their base speed.
pub const NURSING_SPEED_FACTOR: f32 = 0.5;

/// Pheromone deposit multiplier at zero completed trips.
pub const DEPOSIT_BASE_MULTIPLIER: f32 = 0.6;
/// Extra deposit multiplier earned by experienced ants.
pub const DEPOSIT_SUCCESS_BONUS: f32 = 0.4;
/// Completed trips needed to earn the full success bonus.
pub const DEPOSIT_SUCCESS_TRIPS_CAP: u32 = 3;
/// Sample points along the last movement segment used for deposits.
pub const DEPOSIT_SAMPLES: usize = 3;

/// Home-vector weight when a returning ant follows a strong trail.
pub const ANT_HOME_WEIGHT_BASE: f32 = 0.3;
/// Extra home-vector weight as the followed trail fades.
pub const ANT_HOME_WEIGHT_TRAIL: f32 = 0.7;
/// Half-range of the heading noise (radians) used when returning purely by
/// path integration.
pub const ANT_HOME_HEADING_NOISE: f32 = 0.15;

/// World units of travel per radian of path-integration drift noise. The
/// per-tick accumulation is `(rng - 0.5) * distance / PI_DRIFT_DISTANCE`, so a
/// 670 u round trip accumulates ~0.17 rad RMS of error.
pub const PI_DRIFT_DISTANCE: f32 = 40.0;
/// Absolute clamp on [`crate::simulation::ant::Ant::pi_drift`] in radians
/// (~34°). Keeps homing sane even if a nest visit does not reset the drift.
pub const PI_DRIFT_MAX: f32 = 0.6;
/// Probability that an outbound forager with a route memory follows it when
/// no food is sensed and the trail is weak.
pub const ROUTE_MEMORY_USE: f32 = 0.5;
/// Trail strength below which a route memory may be used instead of trail
/// following.
pub const ROUTE_MEMORY_TRAIL_MAX: f32 = 0.02;
/// Jitter fraction of the max turn while following a route memory.
pub const ROUTE_MEMORY_JITTER_FACTOR: f32 = 0.25;

/// Short-range food olfaction range in world units.
pub const FOOD_SENSE_RANGE: f32 = 16.0;
/// Half-angle of the forward food-sensing cone (`±90°`).
pub const FOOD_SENSE_HALF_ANGLE: f32 = PI / 2.0;
/// Centre-to-centre contact distance required to pick food up.
pub const FOOD_PICKUP_RADIUS: f32 = ANT_SIZE * 1.5;

// Ant–ant contact and lane formation (navigation stream).
/// Fine contact-grid cell size in world units.
pub const CONTACT_CELL_SIZE: f32 = 2.0;
/// Centre-to-centre distance at which two ants are in physical contact.
pub const CONTACT_RADIUS: f32 = ANT_SIZE * 1.5;
/// Maximum occupied cells inspected per ant and tick; bounds the cost of the
/// contact pass independently of local density.
pub const CONTACT_NEIGHBORS_MAX: usize = 4;
/// Fraction of the overlap each ant corrects positionally per tick.
pub const CONTACT_CORRECTION_SPLIT: f32 = 0.5;
/// Global scale of contact avoidance turns, as a fraction of `ANT_TURN_RATE`.
pub const CONTACT_TURN_FACTOR: f32 = 0.6;
/// Heading dot product below which an encounter counts as head-on.
pub const CONTACT_HEAD_ON_DOT: f32 = -0.5;
/// Lateral-turn weight for a laden ant yielding in a head-on encounter.
/// Laden ants are the less manoeuvrable stream, so they yield less.
pub const CONTACT_LADEN_YIELD: f32 = 0.4;
/// Lateral-turn weight for an outbound ant yielding in a head-on encounter.
/// The laden/outbound asymmetry biases lane assignment.
pub const CONTACT_OUTBOUND_YIELD: f32 = 0.8;
/// Lateral-turn weight when both ants carry the same load state.
pub const CONTACT_SAME_KIND_YIELD: f32 = 0.6;
/// Lateral-turn weight for side (non-head-on) contacts: turn away from the
/// neighbour's side.
pub const CONTACT_SIDE_TURN_FACTOR: f32 = 0.4;
/// Distances below this count as coincident; the push direction then falls
/// back to the ant's right (keep-right) instead of a noisy unit vector.
pub const CONTACT_MIN_DISTANCE: f32 = 0.01;

// Antennal casting, U-turns and sensor noise (navigation stream).
/// Relative per-ant sensor noise half-range:
/// `reading *= sensor_gain * (1 + (rng - 0.5) * SENSOR_NOISE)`.
pub const SENSOR_NOISE: f32 = 0.2;
/// Seconds without any trail signal before antennal casting starts.
pub const CAST_AFTER: f32 = 0.5;
/// Length of one casting window in seconds; one full ±sweep cycle fits in the
/// window, so successive windows cannot accumulate a net turn.
pub const CAST_DURATION: f32 = 0.4;
/// Casting windows per second (a window starts every `1 / CAST_HZ` seconds).
pub const CAST_HZ: f32 = 2.0;
/// Sweep amplitude as a fraction of `SENSOR_ANGLE`.
pub const CAST_SWEEP_FRACTION: f32 = 1.0;
/// Distance from a wall within which casting bends toward the wall tangent.
pub const CAST_WALL_MARGIN: f32 = 8.0;
/// Strength of the wall-tangent pull while casting, in turn-rate units.
pub const CAST_WALL_TANGENT_FACTOR: f32 = 1.0;
/// Probability of a U-turn when a followed trail drops to zero.
pub const U_TURN_CHANCE: f32 = 0.5;
/// Delay between losing the trail and the U-turn, in seconds.
pub const U_TURN_DELAY: f32 = 0.15;
/// Half-range of the U-turn angle jitter, in radians.
pub const U_TURN_JITTER: f32 = 0.1;

/// Maximum extra spawn-batch multiplier contributed by recent deliveries.
pub const ANT_DELIVERY_BOOST: f32 = 1.5;
/// Time constant of the food-delivery rate EMA, in seconds.
pub const ANT_DELIVERY_EMA_TAU: f32 = 5.0;
/// Delivery rate (food/s) that saturates the recruitment boost.
pub const ANT_DELIVERY_REFERENCE_RATE: f32 = 20.0;
