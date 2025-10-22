mod sensors;
mod steering;
mod wall_collision;

use crate::components::Ant;
use crate::constants::SENSOR_DISTANCE;
use crate::pheromone::PheromoneGrid;
use crate::resources::FoodCells;
use bevy::prelude::*;
use sensors::read_sensors;
use std::sync::Mutex;
use steering::apply_steering;
use wall_collision::handle_wall_collision;

struct PheromoneDeposit {
    grid_x: usize,
    grid_y: usize,
    to_food: f32,
    to_nest: f32,
}

pub fn move_ants(
    mut ant_query: Query<(&mut Ant, &mut Transform)>,
    time: Res<Time>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    food_cells: Res<FoodCells>,
) {
    let delta = time.delta_secs();
    let deposits = Mutex::new(Vec::new());

    ant_query
        .par_iter_mut()
        .for_each(|(mut ant, mut transform)| {
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

            let (sin, cos) = ant.direction.sin_cos();
            let velocity = Vec2::new(cos, sin) * ant.speed;
            transform.translation.x += velocity.x * delta;
            transform.translation.y += velocity.y * delta;

            handle_wall_collision(&mut ant, &mut transform);

            let pos = Vec2::new(transform.translation.x, transform.translation.y);
            if let Some(deposit) = calculate_pheromone_deposit(&ant, pos, &pheromone_grid, delta) {
                deposits.lock().unwrap().push(deposit);
            }
        });

    let deposits = deposits.into_inner().unwrap();
    apply_pheromone_deposits(&mut pheromone_grid, deposits);
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

fn calculate_pheromone_deposit(
    ant: &Ant,
    pos: Vec2,
    pheromone_grid: &PheromoneGrid,
    delta: f32,
) -> Option<PheromoneDeposit> {
    use crate::constants::*;

    if let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(pos) {
        let youth_factor = (ant.lifetime / ant.max_lifetime).max(0.0);
        let youth_multiplier =
            ANT_YOUTH_DEPOSIT_MIN + (youth_factor * youth_factor * ANT_YOUTH_DEPOSIT_MAX);
        let deposit_amount = PHEROMONE_DEPOSIT_RATE * delta * youth_multiplier;

        if ant.has_food {
            Some(PheromoneDeposit {
                grid_x,
                grid_y,
                to_food: deposit_amount,
                to_nest: 0.0,
            })
        } else {
            Some(PheromoneDeposit {
                grid_x,
                grid_y,
                to_food: 0.0,
                to_nest: deposit_amount,
            })
        }
    } else {
        None
    }
}

fn apply_pheromone_deposits(pheromone_grid: &mut PheromoneGrid, deposits: Vec<PheromoneDeposit>) {
    use crate::constants::PHEROMONE_MAX_INTENSITY;

    for deposit in deposits {
        pheromone_grid.add_pheromone(
            deposit.grid_x,
            deposit.grid_y,
            deposit.to_food,
            deposit.to_nest,
            PHEROMONE_MAX_INTENSITY,
        );
    }
}
