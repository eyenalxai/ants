//! Tunables for ant behavior: movement, sensing, energy and lifecycle.

use std::f32::consts::PI;

pub const MAX_ANTS: usize = 30000;
pub const ANT_SPAWN_INTERVAL: f32 = 0.05;
pub const ANT_BATCH_SIZE: usize = 100;
pub const ANT_SPEED: f32 = 50.0;
pub const ANT_SIZE: f32 = 2.0;
pub const ANT_ALPHA: f32 = 0.005;
pub const ANT_LIFETIME: f32 = 30.0;
pub const ANT_LIFETIME_VARIATION_MIN: f32 = 0.5;
pub const ANT_SPEED_VARIATION_MIN: f32 = 0.5;

pub const ANT_TURN_RATE: f32 = 9.0;
pub const ANT_RANDOM_TURN_CHANCE: f32 = 0.8;
pub const ANT_EXPLORATION_CHANCE: f32 = 0.2;
pub const ANT_PROBABILISTIC_STEERING_CHANCE: f32 = 0.6;
pub const ANT_STEERING_NOISE_FACTOR: f32 = 0.8;
pub const ANT_TURN_INTENSITY_BASE: f32 = 0.6;
pub const ANT_TURN_INTENSITY_SCALE: f32 = 0.4;

/// Laden ants move at this fraction of their base speed.
pub const CARRY_SPEED_FACTOR: f32 = 0.65;
/// Food taken from a cell per successful pickup.
pub const CARRY_AMOUNT: f32 = 1.0;
/// Seconds an ant stands still after picking up or dropping off food.
pub const HANDLING_TIME: f32 = 0.5;

/// Energy lost per second while walking; a full tank lasts ~40 s on the move.
pub const ANT_ENERGY_DRAIN_RATE: f32 = 0.025;
/// Extra energy-drain multiplier while carrying food.
pub const ANT_CARRY_ENERGY_DRAIN_FACTOR: f32 = 1.5;
/// Below this fraction an ant abandons foraging and returns to the nest.
pub const ANT_ENERGY_RETURN_THRESHOLD: f32 = 0.25;
/// Fraction of `max_lifetime` spent nursing inside the nest.
pub const ANT_NURSING_LIFETIME_FRACTION: f32 = 0.25;
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

/// Short-range food olfaction range in world units.
pub const FOOD_SENSE_RANGE: f32 = 10.0;
/// Half-angle of the forward food-sensing cone.
pub const FOOD_SENSE_HALF_ANGLE: f32 = PI / 3.0;
/// Centre-to-centre contact distance required to pick food up.
pub const FOOD_PICKUP_RADIUS: f32 = ANT_SIZE * 1.5;

/// Maximum extra spawn-batch multiplier contributed by recent deliveries.
pub const ANT_DELIVERY_BOOST: f32 = 1.5;
/// Time constant of the food-delivery rate EMA, in seconds.
pub const ANT_DELIVERY_EMA_TAU: f32 = 5.0;
/// Delivery rate (food/s) that saturates the recruitment boost.
pub const ANT_DELIVERY_REFERENCE_RATE: f32 = 20.0;
