mod pheromone_deposit;
mod sensors;
mod steering;
mod wall_collision;

use crate::components::Ant;
use crate::constants::SENSOR_DISTANCE;
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
) {
    let delta = time.delta_secs();

    for (mut ant, mut transform) in &mut ant_query {
        let current_pos = Vec2::new(transform.translation.x, transform.translation.y);

        if !ant.has_food {
            if let Some(closest_food) =
                find_closest_food_in_range(&current_pos, &food_cells, &pheromone_grid)
            {
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

        let velocity = Vec2::new(ant.direction.cos(), ant.direction.sin()) * ant.speed;
        transform.translation.x += velocity.x * delta;
        transform.translation.y += velocity.y * delta;

        handle_wall_collision(&mut ant, &mut transform);

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        deposit_pheromone(&ant, pos, &mut pheromone_grid, delta);
    }
}

fn find_closest_food_in_range(
    ant_pos: &Vec2,
    food_cells: &FoodCells,
    _pheromone_grid: &PheromoneGrid,
) -> Option<Vec2> {
    let mut closest_food: Option<(Vec2, f32)> = None;

    for &food_world_pos in &food_cells.world_positions {
        let distance = ant_pos.distance(food_world_pos);

        if distance <= SENSOR_DISTANCE {
            if let Some((_, closest_dist)) = closest_food {
                if distance < closest_dist {
                    closest_food = Some((food_world_pos, distance));
                }
            } else {
                closest_food = Some((food_world_pos, distance));
            }
        }
    }

    closest_food.map(|(pos, _)| pos)
}
