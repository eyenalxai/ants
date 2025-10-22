use crate::components::{Ant, Nest};
use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use crate::resources::FoodCells;
use bevy::prelude::*;
use std::f32::consts::PI;

pub fn check_collisions(
    mut ant_query: Query<(&mut Ant, &Transform)>,
    nest_query: Query<&Transform, With<Nest>>,
    mut food_cells: ResMut<FoodCells>,
    pheromone_grid: Res<PheromoneGrid>,
) {
    let Some(nest_transform) = nest_query.iter().next() else {
        return;
    };
    let nest_pos = Vec2::new(nest_transform.translation.x, nest_transform.translation.y);
    let nest_radius = NEST_SIZE / 2.0;

    for (mut ant, transform) in &mut ant_query {
        let ant_pos = Vec2::new(transform.translation.x, transform.translation.y);

        if !ant.has_food
            && let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(ant_pos)
            && food_cells.cells.contains_key(&(grid_x, grid_y))
        {
            if let Some(amount) = food_cells.cells.get_mut(&(grid_x, grid_y)) {
                *amount -= FOOD_DEPLETION_RATE;
            }
            ant.has_food = true;
            ant.direction = find_best_direction(&ant, ant_pos, &pheromone_grid);
        } else if ant.has_food && ant_pos.distance(nest_pos) < nest_radius {
            ant.has_food = false;
            ant.direction = find_best_direction(&ant, ant_pos, &pheromone_grid);
        }
    }
}

fn find_best_direction(ant: &Ant, current_pos: Vec2, pheromone_grid: &PheromoneGrid) -> f32 {
    let mut total_intensity = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;

    let angle_step = (2.0 * PI) / FULL_SCAN_SENSORS as f32;

    for i in 0..FULL_SCAN_SENSORS {
        let check_angle = i as f32 * angle_step;
        let (sin, cos) = check_angle.sin_cos();

        let sensor_pos = current_pos + Vec2::new(cos * SENSOR_DISTANCE, sin * SENSOR_DISTANCE);

        if let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(sensor_pos)
            && let Some(pheromone) = pheromone_grid.get(grid_x, grid_y)
        {
            let intensity = if ant.has_food {
                pheromone.to_nest
            } else {
                pheromone.to_food
            };

            if intensity > 0.01 {
                total_intensity += intensity;
                weighted_x += cos * intensity;
                weighted_y += sin * intensity;
            }
        }
    }

    if total_intensity > 0.01 {
        weighted_y.atan2(weighted_x)
    } else {
        (ant.direction + PI).rem_euclid(2.0 * PI)
    }
}
