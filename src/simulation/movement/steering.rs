//! Ant steering: trail following, path integration, food approach and wandering.

use std::f32::consts::PI;

use bevy::prelude::*;

use crate::constants::ant::*;
use crate::constants::sensor::{NUM_SENSORS, SENSOR_ANGLE};
use crate::constants::world::NEST_RADIUS;
use crate::simulation::ant::Ant;
use crate::simulation::movement::sensors::sensor_offset;

/// Signed shortest difference from `from` to `to`, in `(-PI, PI]`.
pub fn shortest_angle_diff(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(2.0 * PI) - PI
}

/// Turn `ant.direction` toward `target`, capped at `max_turn` radians.
pub fn turn_towards(ant: &mut Ant, target: f32, max_turn: f32) {
    let turn = shortest_angle_diff(ant.direction, target).clamp(-max_turn, max_turn);
    ant.direction = (ant.direction + turn).rem_euclid(2.0 * PI);
}

/// Strongest normalized reading, used as the trail-strength signal in `[0, 1]`.
pub fn trail_strength(readings: &[f32; NUM_SENSORS]) -> f32 {
    readings.iter().copied().fold(0.0, f32::max).clamp(0.0, 1.0)
}

/// World-space unit vector toward the strongest trail side. `None` when no
/// reading carries a usable signal.
pub fn trail_vector(ant: &Ant, readings: &[f32; NUM_SENSORS]) -> Option<Vec2> {
    let mut sum = Vec2::ZERO;

    for (index, &reading) in readings.iter().enumerate() {
        if reading <= 0.0 {
            continue;
        }

        let angle = ant.direction + sensor_offset(index);
        sum += Vec2::new(angle.cos(), angle.sin()) * reading;
    }

    (sum.length_squared() > f32::EPSILON).then(|| sum.normalize())
}

/// Desired heading blending the followed trail with the home vector. The home
/// vector dominates as the trail fades; with no trail at all the ant walks
/// home with a small heading noise.
pub fn homeward_desired(
    ant: &Ant,
    pos: Vec2,
    readings: &[f32; NUM_SENSORS],
    rng: &mut fastrand::Rng,
) -> f32 {
    let to_home = ant.home - pos;
    let home_dir = (to_home.length_squared() > f32::EPSILON).then(|| to_home.normalize());

    let strength = trail_strength(readings);
    let trail = trail_vector(ant, readings);

    match (trail, home_dir) {
        (Some(trail), Some(home)) => {
            let home_weight = ANT_HOME_WEIGHT_BASE + ANT_HOME_WEIGHT_TRAIL * (1.0 - strength);
            let blended = trail * (1.0 - home_weight) + home * home_weight;

            if blended.length_squared() > f32::EPSILON {
                blended.y.atan2(blended.x)
            } else {
                home.y.atan2(home.x)
            }
        }
        (Some(trail), None) => trail.y.atan2(trail.x),
        (None, Some(home)) => {
            let noise = (rng.f32() - 0.5) * 2.0 * ANT_HOME_HEADING_NOISE;
            home.y.atan2(home.x) + noise
        }
        (None, None) => ant.direction,
    }
}

/// Steer a returning ant: path integration blended with the `ToNest` trail,
/// capped at one [`ANT_TURN_RATE`] step.
pub fn steer_homeward(
    ant: &mut Ant,
    pos: Vec2,
    readings: &[f32; NUM_SENSORS],
    delta: f32,
    rng: &mut fastrand::Rng,
) {
    let desired = homeward_desired(ant, pos, readings, rng);
    turn_towards(ant, desired, ANT_TURN_RATE * delta);
}

/// Steer toward a sensed food position with a turn-rate-limited turn.
pub fn steer_towards_food(
    ant: &mut Ant,
    food_pos: Vec2,
    pos: Vec2,
    delta: f32,
    rng: &mut fastrand::Rng,
) {
    let offset = food_pos - pos;

    if offset.length_squared() <= f32::EPSILON {
        return;
    }

    let max_turn = ANT_TURN_RATE * delta;
    let jitter = (rng.f32() - 0.5) * max_turn * 0.25;
    turn_towards(ant, offset.y.atan2(offset.x) + jitter, max_turn);
}

/// Nurses wander near the nest and are pulled home when they drift too far.
pub fn steer_nursing(ant: &mut Ant, pos: Vec2, delta: f32, rng: &mut fastrand::Rng) {
    let leash = NEST_RADIUS * NURSING_LEASH_FACTOR;
    let to_home = ant.home - pos;

    if to_home.length_squared() > leash * leash {
        let noise = (rng.f32() - 0.5) * 2.0 * ANT_HOME_HEADING_NOISE;
        turn_towards(
            ant,
            to_home.y.atan2(to_home.x) + noise,
            ANT_TURN_RATE * delta,
        );
    } else if rng.f32() < ANT_RANDOM_TURN_CHANCE {
        apply_random_turn(ant, delta * 0.5, rng);
    }
}

/// Turn the ant toward the strongest pheromone reading (or wander randomly).
/// Readings are already gated per sensor by `SENSOR_SIGNAL_THRESHOLD`, so any
/// non-zero total is a usable signal.
pub fn apply_steering(
    ant: &mut Ant,
    sensor_readings: &[f32; NUM_SENSORS],
    delta: f32,
    rng: &mut fastrand::Rng,
) {
    let total_intensity: f32 = sensor_readings.iter().sum();
    let intensity_strength = (total_intensity / NUM_SENSORS as f32).min(1.0);
    // A strong signal suppresses exploration so a busy trail is followed
    // tightly; with no signal the ant explores at the full base rate.
    let explore_chance = ANT_EXPLORATION_CHANCE * (1.0 - intensity_strength);

    if total_intensity > 0.0 && rng.f32() > explore_chance {
        let use_probabilistic = rng.f32() < ANT_PROBABILISTIC_STEERING_CHANCE;

        let target_direction = if use_probabilistic {
            probabilistic_direction(ant, sensor_readings, total_intensity, rng)
        } else {
            weighted_direction(ant, sensor_readings)
        };

        let max_turn = ANT_TURN_RATE
            * delta
            * (ANT_TURN_INTENSITY_BASE + intensity_strength * ANT_TURN_INTENSITY_SCALE);
        turn_towards(ant, target_direction, max_turn);
    } else if rng.f32() < ANT_RANDOM_TURN_CHANCE {
        apply_random_turn(ant, delta, rng);
    }
}

fn probabilistic_direction(
    ant: &Ant,
    sensor_readings: &[f32; NUM_SENSORS],
    total_intensity: f32,
    rng: &mut fastrand::Rng,
) -> f32 {
    let random_value = rng.f32() * total_intensity;
    let mut cumulative = 0.0;
    let mut chosen_angle = ant.direction;

    for (index, &intensity) in sensor_readings.iter().enumerate() {
        cumulative += intensity;
        if cumulative >= random_value {
            chosen_angle = ant.direction + sensor_offset(index);
            break;
        }
    }

    let noise = (rng.f32() - 0.5) * SENSOR_ANGLE * ANT_STEERING_NOISE_FACTOR;
    chosen_angle + noise
}

fn weighted_direction(ant: &Ant, sensor_readings: &[f32; NUM_SENSORS]) -> f32 {
    let mut weighted = Vec2::ZERO;

    for (index, &intensity) in sensor_readings.iter().enumerate() {
        let angle = ant.direction + sensor_offset(index);
        weighted += Vec2::new(angle.cos(), angle.sin()) * intensity;
    }

    if weighted.length_squared() > f32::EPSILON {
        weighted.y.atan2(weighted.x)
    } else {
        ant.direction
    }
}

fn apply_random_turn(ant: &mut Ant, delta: f32, rng: &mut fastrand::Rng) {
    let turn_amount = (rng.f32() - 0.5) * 2.0 * ANT_TURN_RATE * delta;
    ant.direction = (ant.direction + turn_amount).rem_euclid(2.0 * PI);
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    #[test]
    fn shortest_angle_wraps_correctly() {
        assert!((shortest_angle_diff(0.0, PI / 2.0) - PI / 2.0).abs() < EPS);
        assert!((shortest_angle_diff(0.0, -PI / 2.0) + PI / 2.0).abs() < EPS);
        assert!((shortest_angle_diff(0.1, 2.0 * PI - 0.1) + 0.2).abs() < EPS);
        assert!((shortest_angle_diff(2.0 * PI - 0.1, 0.1) - 0.2).abs() < EPS);
    }

    #[test]
    fn turn_is_capped_at_max_turn() {
        let mut ant = Ant::test_ant(0.0);
        turn_towards(&mut ant, PI / 2.0, 0.1);
        assert!((ant.direction - 0.1).abs() < EPS);

        let mut ant = Ant::test_ant(0.0);
        turn_towards(&mut ant, -PI / 2.0, 10.0);
        assert!((ant.direction - (2.0 * PI - PI / 2.0)).abs() < EPS);
    }

    #[test]
    fn homeward_steering_prefers_home_when_there_is_no_trail() {
        let mut ant = Ant::test_ant(0.0);
        ant.home = Vec2::new(0.0, 100.0);
        let mut rng = fastrand::Rng::with_seed(0);

        let readings = [0.0; NUM_SENSORS];
        for _ in 0..200 {
            steer_homeward(&mut ant, Vec2::ZERO, &readings, 1.0 / 64.0, &mut rng);
        }

        // Path integration should have turned the ant roughly northwards.
        let (sin, cos) = ant.direction.sin_cos();
        assert!(sin > 0.9, "ant should head home, got {sin} (cos {cos})");
    }
}
