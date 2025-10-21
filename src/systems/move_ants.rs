use crate::components::Ant;
use crate::constants::{
    ANT_RANDOM_TURN_CHANCE, ANT_SPEED, ANT_TURN_RATE, WINDOW_HEIGHT, WINDOW_WIDTH,
};
use crate::pheromone::PheromoneGrid;
use bevy::prelude::*;
use std::f32::consts::PI;

pub fn move_ants(
    mut ant_query: Query<(&mut Ant, &mut Transform)>,
    time: Res<Time>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
) {
    let half_width = WINDOW_WIDTH as f32 / 2.0;
    let half_height = WINDOW_HEIGHT as f32 / 2.0;
    let min_angle = 30.0_f32.to_radians();
    let delta = time.delta_secs();

    for (mut ant, mut transform) in &mut ant_query {
        if rand::random::<f32>() < ANT_RANDOM_TURN_CHANCE {
            let turn_amount = (rand::random::<f32>() - 0.5) * 2.0 * ANT_TURN_RATE * delta;
            ant.direction = (ant.direction + turn_amount).rem_euclid(2.0 * PI);
        }
        let velocity = Vec2::new(ant.direction.cos(), ant.direction.sin()) * ANT_SPEED;
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
        if let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(pos)
            && let Some(cell) = pheromone_grid.get_mut(grid_x, grid_y)
        {
            if ant.has_food {
                cell.to_food += 1.0;
            } else {
                cell.to_nest += 1.0;
            }
        }
    }
}
