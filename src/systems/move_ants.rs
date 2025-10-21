use crate::components::{Ant, Food, Nest};
use crate::constants::{
    ANT_EXPLORATION_CHANCE, ANT_PHEROMONE_FOLLOW_RANDOMNESS, ANT_RANDOM_TURN_CHANCE, ANT_TURN_RATE,
    PHEROMONE_DEPOSIT_RATE, PHEROMONE_MAX_INTENSITY, WINDOW_HEIGHT, WINDOW_WIDTH,
};
use crate::pheromone::PheromoneGrid;
use bevy::prelude::*;
use std::f32::consts::PI;

const SENSOR_DISTANCE: f32 = 10.0;
const SENSOR_ANGLE: f32 = PI / 4.0;
const NUM_SENSORS: usize = 5;
const TARGET_LOCK_DISTANCE: f32 = 40.0;

pub fn move_ants(
    mut ant_query: Query<(&mut Ant, &mut Transform), Without<Food>>,
    time: Res<Time>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    food_query: Query<&Transform, (With<Food>, Without<Ant>)>,
    nest_query: Query<&Transform, (With<Nest>, Without<Ant>)>,
) {
    let half_width = WINDOW_WIDTH as f32 / 2.0;
    let half_height = WINDOW_HEIGHT as f32 / 2.0;
    let min_angle = 30.0_f32.to_radians();
    let delta = time.delta_secs();

    let food_pos = food_query
        .iter()
        .next()
        .map(|t| Vec2::new(t.translation.x, t.translation.y));
    let nest_pos = nest_query
        .iter()
        .next()
        .map(|t| Vec2::new(t.translation.x, t.translation.y));

    for (mut ant, mut transform) in &mut ant_query {
        let current_pos = Vec2::new(transform.translation.x, transform.translation.y);

        let target_pos = if ant.has_food { nest_pos } else { food_pos };

        let should_lock_target = if let Some(target) = target_pos {
            current_pos.distance(target) < TARGET_LOCK_DISTANCE
        } else {
            false
        };

        if should_lock_target && let Some(target) = target_pos {
            let to_target = target - current_pos;
            let target_angle = to_target.y.atan2(to_target.x);
            ant.direction = target_angle;
        } else {
            let mut sensor_readings = Vec::with_capacity(NUM_SENSORS);
            let mut total_intensity = 0.0;

            for i in 0..NUM_SENSORS {
                let angle_offset =
                    -SENSOR_ANGLE + (i as f32 / (NUM_SENSORS - 1) as f32) * (2.0 * SENSOR_ANGLE);
                let check_angle = ant.direction + angle_offset;

                let sensor_pos = current_pos
                    + Vec2::new(
                        check_angle.cos() * SENSOR_DISTANCE,
                        check_angle.sin() * SENSOR_DISTANCE,
                    );

                let intensity = if let Some((grid_x, grid_y)) =
                    pheromone_grid.world_to_grid(sensor_pos)
                    && let Some(pheromone) = pheromone_grid.get(grid_x, grid_y)
                {
                    if ant.has_food {
                        pheromone.to_nest
                    } else {
                        pheromone.to_food
                    }
                } else {
                    0.0
                };

                total_intensity += intensity;
                sensor_readings.push((check_angle, intensity));
            }

            if total_intensity > 0.01 && rand::random::<f32>() > ANT_EXPLORATION_CHANCE {
                let use_probabilistic = rand::random::<f32>() < 0.6;

                let target_direction = if use_probabilistic {
                    let random_value = rand::random::<f32>() * total_intensity;
                    let mut cumulative = 0.0;
                    let mut chosen_angle = ant.direction;

                    for (angle, intensity) in &sensor_readings {
                        cumulative += intensity;
                        if cumulative >= random_value {
                            chosen_angle = *angle;
                            break;
                        }
                    }

                    let noise = (rand::random::<f32>() - 0.5) * SENSOR_ANGLE * 0.8;
                    chosen_angle + noise
                } else {
                    let mut weighted_x = 0.0;
                    let mut weighted_y = 0.0;

                    for (angle, intensity) in &sensor_readings {
                        weighted_x += angle.cos() * intensity;
                        weighted_y += angle.sin() * intensity;
                    }

                    weighted_y.atan2(weighted_x)
                };

                let angle_diff = (target_direction - ant.direction).rem_euclid(2.0 * PI);
                let shortest_angle = if angle_diff > PI {
                    angle_diff - 2.0 * PI
                } else {
                    angle_diff
                };

                let intensity_strength = (total_intensity / 10.0).min(1.0);
                let exploration_factor = 1.0 - (intensity_strength * 0.6);
                let random_offset = (rand::random::<f32>() - 0.5)
                    * 2.0
                    * ANT_PHEROMONE_FOLLOW_RANDOMNESS
                    * PI
                    * exploration_factor;

                let randomized_angle = shortest_angle + random_offset;
                let max_turn = ANT_TURN_RATE * delta * (0.6 + intensity_strength * 0.4);
                let turn_amount = randomized_angle.clamp(-max_turn, max_turn);
                ant.direction = (ant.direction + turn_amount).rem_euclid(2.0 * PI);
            } else if rand::random::<f32>() < ANT_RANDOM_TURN_CHANCE {
                let turn_amount = (rand::random::<f32>() - 0.5) * 2.0 * ANT_TURN_RATE * delta;
                ant.direction = (ant.direction + turn_amount).rem_euclid(2.0 * PI);
            }
        }

        let velocity = Vec2::new(ant.direction.cos(), ant.direction.sin()) * ant.speed;
        transform.translation.x += velocity.x * delta;
        transform.translation.y += velocity.y * delta;

        if transform.translation.x > half_width || transform.translation.x < -half_width {
            if transform.translation.x > half_width {
                transform.translation.x = half_width;
            } else {
                transform.translation.x = -half_width;
            }

            let mut new_direction = PI - ant.direction;

            let angle_to_horizontal = (new_direction.rem_euclid(2.0 * PI) - PI).abs();
            if angle_to_horizontal < min_angle {
                if new_direction.sin() > 0.0 {
                    new_direction = PI - min_angle;
                } else {
                    new_direction = PI + min_angle;
                }
            }

            ant.direction = new_direction.rem_euclid(2.0 * PI);
        }

        if transform.translation.y > half_height || transform.translation.y < -half_height {
            if transform.translation.y > half_height {
                transform.translation.y = half_height;
            } else {
                transform.translation.y = -half_height;
            }

            let mut new_direction = (2.0 * PI) - ant.direction;

            let normalized = new_direction.rem_euclid(2.0 * PI);
            let angle_to_horizontal = normalized.min((2.0 * PI) - normalized);
            if angle_to_horizontal < min_angle {
                if new_direction.cos() > 0.0 {
                    new_direction = min_angle;
                } else {
                    new_direction = PI - min_angle;
                }
            }

            ant.direction = new_direction.rem_euclid(2.0 * PI);
        }

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        if let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(pos) {
            let youth_factor = (ant.lifetime / ant.max_lifetime).max(0.0);
            let youth_multiplier = 0.2 + (youth_factor * youth_factor * 0.8);
            let deposit_amount = PHEROMONE_DEPOSIT_RATE * delta * youth_multiplier;

            if ant.has_food {
                pheromone_grid.add_pheromone(
                    grid_x,
                    grid_y,
                    deposit_amount,
                    0.0,
                    PHEROMONE_MAX_INTENSITY,
                );
            } else {
                pheromone_grid.add_pheromone(
                    grid_x,
                    grid_y,
                    0.0,
                    deposit_amount,
                    PHEROMONE_MAX_INTENSITY,
                );
            }
        }
    }
}
