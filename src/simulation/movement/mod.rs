pub mod sensors;
pub mod steering;
pub mod wall;

use bevy::prelude::*;

use crate::constants::sensor::SENSOR_DISTANCE;
use crate::core::grid::grid_to_world;
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::ant::Ant;
use crate::simulation::food::FoodGrid;
use sensors::read_sensors;
use steering::apply_steering;
use wall::handle_wall_collision;

/// Advance every ant: steer, move and bounce off the walls.
///
/// Pheromone deposit is a separate serial pass in
/// [`crate::simulation::deposit`].
pub fn move_ants(
    mut ant_query: Query<(&mut Ant, &mut Transform)>,
    time: Res<Time<Fixed>>,
    pheromone_grid: Res<PheromoneGrid>,
    food_grid: Res<FoodGrid>,
) {
    let delta = time.delta_secs();

    ant_query
        .par_iter_mut()
        .for_each(|(mut ant, mut transform)| {
            let current_pos = Vec2::new(transform.translation.x, transform.translation.y);

            if !ant.has_food {
                if let Some(closest_food) = find_closest_food_in_range(&current_pos, &food_grid) {
                    let to_food = closest_food - current_pos;
                    ant.direction = to_food.y.atan2(to_food.x);
                } else {
                    let sensor_readings = read_sensors(&ant, current_pos, &pheromone_grid);
                    apply_steering(&mut ant, &sensor_readings, delta);
                }
            } else {
                let sensor_readings = read_sensors(&ant, current_pos, &pheromone_grid);
                apply_steering(&mut ant, &sensor_readings, delta);
            }

            let (sin, cos) = ant.direction.sin_cos();
            let velocity = Vec2::new(cos, sin) * ant.speed;
            transform.translation.x += velocity.x * delta;
            transform.translation.y += velocity.y * delta;

            handle_wall_collision(&mut ant, &mut transform);
        });
}

/// Nearest food cell center within [`SENSOR_DISTANCE`] of the ant.
fn find_closest_food_in_range(ant_pos: &Vec2, food_grid: &FoodGrid) -> Option<Vec2> {
    let mut closest_food: Option<(Vec2, f32)> = None;

    for (cell, _amount) in food_grid.iter() {
        let food_pos = grid_to_world(cell);
        let distance = ant_pos.distance(food_pos);

        if distance <= SENSOR_DISTANCE
            && closest_food.is_none_or(|(_, closest_distance)| distance < closest_distance)
        {
            closest_food = Some((food_pos, distance));
        }
    }

    closest_food.map(|(pos, _)| pos)
}
