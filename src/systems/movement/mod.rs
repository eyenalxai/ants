mod pheromone_deposit;
mod sensors;
mod steering;
mod wall_collision;

use crate::components::{Ant, Nest};
use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use crate::resources::FoodCells;
use bevy::prelude::*;
use pheromone_deposit::deposit_pheromone;
use sensors::read_sensors;
use steering::apply_steering;
use wall_collision::handle_wall_collision;

pub fn move_ants(
    mut ant_query: Query<(&mut Ant, &mut Transform)>,
    time: Res<Time>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    food_cells: Res<FoodCells>,
    nest_query: Query<&Transform, (With<Nest>, Without<Ant>)>,
) {
    let delta = time.delta_secs();

    let food_pos = if !food_cells.cells.is_empty() {
        let (grid_x, grid_y) = food_cells.cells[0];
        let world_x = grid_x as f32 * GRID_SIZE - WINDOW_WIDTH as f32 / 2.0 + GRID_SIZE / 2.0;
        let world_y = grid_y as f32 * GRID_SIZE - WINDOW_HEIGHT as f32 / 2.0 + GRID_SIZE / 2.0;
        Some(Vec2::new(world_x, world_y))
    } else {
        None
    };

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
            let sensor_readings = read_sensors(&ant, current_pos, &pheromone_grid);
            apply_steering(&mut ant, &sensor_readings, delta);
        }

        let velocity = Vec2::new(ant.direction.cos(), ant.direction.sin()) * ant.speed;
        transform.translation.x += velocity.x * delta;
        transform.translation.y += velocity.y * delta;

        handle_wall_collision(&mut ant, &mut transform);

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        deposit_pheromone(&ant, pos, &mut pheromone_grid, delta);
    }
}
