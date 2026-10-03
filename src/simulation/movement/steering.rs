use std::f32::consts::PI;

use crate::constants::ant::*;
use crate::constants::sensor::{NUM_SENSORS, SENSOR_ANGLE};
use crate::simulation::ant::Ant;

/// Turn the ant toward the strongest sensor reading (or wander randomly).
pub fn apply_steering(ant: &mut Ant, sensor_readings: &[f32; NUM_SENSORS], delta: f32) {
    let total_intensity: f32 = sensor_readings.iter().sum();

    if total_intensity > 0.01 && fastrand::f32() > ANT_EXPLORATION_CHANCE {
        let use_probabilistic = fastrand::f32() < ANT_PROBABILISTIC_STEERING_CHANCE;

        let target_direction = if use_probabilistic {
            calculate_probabilistic_direction(ant, sensor_readings, total_intensity)
        } else {
            calculate_weighted_direction(ant, sensor_readings)
        };

        apply_turn_towards(ant, target_direction, total_intensity, delta);
    } else if fastrand::f32() < ANT_RANDOM_TURN_CHANCE {
        apply_random_turn(ant, delta);
    }
}

/// Absolute world direction of sensor `index`, matching the angles used when
/// the readings were taken.
fn sensor_angle(ant: &Ant, index: usize) -> f32 {
    let sensor_step = (2.0 * SENSOR_ANGLE) / (NUM_SENSORS - 1) as f32;
    ant.direction + (-SENSOR_ANGLE + index as f32 * sensor_step)
}

fn calculate_probabilistic_direction(
    ant: &Ant,
    sensor_readings: &[f32; NUM_SENSORS],
    total_intensity: f32,
) -> f32 {
    let random_value = fastrand::f32() * total_intensity;
    let mut cumulative = 0.0;
    let mut chosen_angle = 0.0;

    for (index, &intensity) in sensor_readings.iter().enumerate() {
        cumulative += intensity;
        if cumulative >= random_value {
            chosen_angle = sensor_angle(ant, index);
            break;
        }
    }

    let noise = (fastrand::f32() - 0.5) * SENSOR_ANGLE * ANT_STEERING_NOISE_FACTOR;
    chosen_angle + noise
}

fn calculate_weighted_direction(ant: &Ant, sensor_readings: &[f32; NUM_SENSORS]) -> f32 {
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;

    for (index, &intensity) in sensor_readings.iter().enumerate() {
        let angle = sensor_angle(ant, index);
        weighted_x += angle.cos() * intensity;
        weighted_y += angle.sin() * intensity;
    }

    weighted_y.atan2(weighted_x)
}

fn apply_turn_towards(ant: &mut Ant, target_direction: f32, total_intensity: f32, delta: f32) {
    let angle_diff = (target_direction - ant.direction).rem_euclid(2.0 * PI);
    let shortest_angle = if angle_diff > PI {
        angle_diff - 2.0 * PI
    } else {
        angle_diff
    };

    let intensity_strength = (total_intensity / 10.0).min(1.0);
    let exploration_factor = 1.0 - (intensity_strength * ANT_EXPLORATION_STRENGTH_BASE);
    let random_offset =
        (fastrand::f32() - 0.5) * 2.0 * ANT_PHEROMONE_FOLLOW_RANDOMNESS * PI * exploration_factor;

    let randomized_angle = shortest_angle + random_offset;
    let max_turn = ANT_TURN_RATE
        * delta
        * (ANT_TURN_INTENSITY_BASE + intensity_strength * ANT_TURN_INTENSITY_SCALE);
    let turn_amount = randomized_angle.clamp(-max_turn, max_turn);
    ant.direction = (ant.direction + turn_amount).rem_euclid(2.0 * PI);
}

fn apply_random_turn(ant: &mut Ant, delta: f32) {
    let turn_amount = (fastrand::f32() - 0.5) * 2.0 * ANT_TURN_RATE * delta;
    ant.direction = (ant.direction + turn_amount).rem_euclid(2.0 * PI);
}
