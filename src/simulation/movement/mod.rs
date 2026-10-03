//! Per-ant steering and movement, including crowd avoidance and wall bounces.

pub mod sensors;
pub mod steering;
pub mod wall;

use bevy::prelude::*;

use crate::constants::ant::{
    ANT_TURN_RATE, CARRY_SPEED_FACTOR, FOOD_PICKUP_RADIUS, FOOD_SENSE_HALF_ANGLE, FOOD_SENSE_RANGE,
    NURSING_SPEED_FACTOR,
};
use crate::constants::world::{
    DENSITY_AHEAD_DISTANCE, DENSITY_AVOIDANCE_TURN_FACTOR, DENSITY_SIDE_DISTANCE, GRID_SIZE,
};
use crate::core::grid::{grid_to_world, world_to_grid};
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::ant::{Ant, AntPhase, AntRng};
use crate::simulation::density::{AntDensity, crowd_response};
use crate::simulation::food::FoodGrid;
use sensors::read_sensors;
use steering::{
    apply_steering, shortest_angle_diff, steer_homeward, steer_nursing, steer_towards_food,
};
use wall::handle_wall_collision;

/// Advance every ant: sense, steer, avoid crowding, move and bounce off walls.
///
/// Pheromone deposit is a separate serial pass in
/// [`crate::simulation::deposit`].
pub fn move_ants(
    mut ant_query: Query<(&mut Ant, &mut Transform, Option<&mut AntRng>)>,
    time: Res<Time<Fixed>>,
    pheromone_grid: Res<PheromoneGrid>,
    food_grid: Res<FoodGrid>,
    density: Res<AntDensity>,
) {
    let delta = time.delta_secs();

    ant_query
        .par_iter_mut()
        .for_each(|(mut ant, mut transform, ant_rng)| {
            // Ants spawned by `spawn_ants` always carry an `AntRng`; the
            // fallback keeps hand-built test ants stepping with a stable
            // stream instead of the thread-local generator.
            let mut fallback = fastrand::Rng::with_seed(0);
            let mut ant_rng = ant_rng;
            let rng: &mut fastrand::Rng = match ant_rng.as_mut() {
                Some(rng) => &mut rng.0,
                None => &mut fallback,
            };

            step_ant(
                &mut ant,
                &mut transform,
                rng,
                &pheromone_grid,
                &food_grid,
                &density,
                delta,
            );
        });
}

/// One ant's fixed step: phase steering, crowd response and movement.
fn step_ant(
    ant: &mut Ant,
    transform: &mut Transform,
    rng: &mut fastrand::Rng,
    pheromone_grid: &PheromoneGrid,
    food_grid: &FoodGrid,
    density: &AntDensity,
    delta: f32,
) {
    let current_pos = Vec2::new(transform.translation.x, transform.translation.y);

    if ant.is_handling() {
        ant.speed = 0.0;
        return;
    }

    let mut target_speed = ant.base_speed;

    match ant.phase {
        AntPhase::Nursing => {
            target_speed *= NURSING_SPEED_FACTOR;
            steer_nursing(ant, current_pos, delta, rng);
        }
        AntPhase::Returning => {
            if ant.has_food {
                target_speed *= CARRY_SPEED_FACTOR;
            }
            let readings = read_sensors(ant, current_pos, pheromone_grid);
            steer_homeward(ant, current_pos, &readings, delta, rng);
        }
        AntPhase::Foraging if ant.has_food => {
            target_speed *= CARRY_SPEED_FACTOR;
            let readings = read_sensors(ant, current_pos, pheromone_grid);
            steer_homeward(ant, current_pos, &readings, delta, rng);
        }
        AntPhase::Foraging => {
            if let Some(food_pos) = sense_food(ant, current_pos, food_grid) {
                steer_towards_food(ant, food_pos, current_pos, delta, rng);
            } else {
                let readings = read_sensors(ant, current_pos, pheromone_grid);
                apply_steering(ant, &readings, delta, rng);
            }
        }
    }

    // Nurses wander inside the crowded nest without reacting to it.
    if ant.phase != AntPhase::Nursing {
        let (speed_factor, turn_sign) = sample_crowding(ant, current_pos, density);

        if turn_sign != 0.0 {
            ant.direction = (ant.direction
                + turn_sign * ANT_TURN_RATE * delta * DENSITY_AVOIDANCE_TURN_FACTOR)
                .rem_euclid(std::f32::consts::TAU);
        }

        target_speed *= speed_factor;
    }

    ant.speed = target_speed;
    let (sin, cos) = ant.direction.sin_cos();
    transform.translation.x += cos * target_speed * delta;
    transform.translation.y += sin * target_speed * delta;

    handle_wall_collision(ant, transform);
}

/// Probe the density grid ahead of the ant and to both sides.
fn sample_crowding(ant: &Ant, pos: Vec2, density: &AntDensity) -> (f32, f32) {
    let (sin, cos) = ant.direction.sin_cos();
    let forward = Vec2::new(cos, sin);
    let left = Vec2::new(-sin, cos);

    let ahead_pos = pos + forward * DENSITY_AHEAD_DISTANCE;
    let ahead = density.sample(ahead_pos);
    let left_count = density.sample(ahead_pos + left * DENSITY_SIDE_DISTANCE);
    let right_count = density.sample(ahead_pos - left * DENSITY_SIDE_DISTANCE);

    crowd_response(ahead, left_count, right_count)
}

/// Nearest food cell inside the short forward cone, if any.
///
/// `FoodGrid::nearest_within` supplies the closest local candidate; only when
/// that candidate is empty or outside the cone do we fall back to scanning the
/// (small) set of local cells for the nearest in-cone one.
fn sense_food(ant: &Ant, ant_pos: Vec2, food_grid: &FoodGrid) -> Option<Vec2> {
    let center = world_to_grid(ant_pos)?;
    // +1 cell covers the ant's offset from its cell centre.
    let radius_cells = (FOOD_SENSE_RANGE / GRID_SIZE).ceil() as i32 + 1;
    let (nearest_cell, _) = food_grid.nearest_within(center, radius_cells)?;

    if let Some((food_pos, _)) = in_cone(ant, ant_pos, nearest_cell, food_grid) {
        return Some(food_pos);
    }

    let mut best: Option<(Vec2, f32)> = None;
    for (cell, _amount) in food_grid.iter() {
        if let Some((food_pos, distance)) = in_cone(ant, ant_pos, cell, food_grid)
            && best.is_none_or(|(_, best_distance)| distance < best_distance)
        {
            best = Some((food_pos, distance));
        }
    }

    best.map(|(food_pos, _)| food_pos)
}

/// Filter one candidate cell through the pickup amount, range and forward
/// cone; returns its position and distance when it passes.
fn in_cone(ant: &Ant, ant_pos: Vec2, cell: UVec2, food_grid: &FoodGrid) -> Option<(Vec2, f32)> {
    if !food_grid.amount(cell).is_some_and(|amount| amount > 0.0) {
        return None;
    }

    let food_pos = grid_to_world(cell);
    let offset = food_pos - ant_pos;
    let distance = offset.length();

    if distance > FOOD_SENSE_RANGE || distance <= f32::EPSILON {
        return None;
    }

    let angle = offset.y.atan2(offset.x);
    if shortest_angle_diff(ant.direction, angle).abs() > FOOD_SENSE_HALF_ANGLE {
        return None;
    }

    Some((food_pos, distance))
}

/// Nearest food cell within [`FOOD_PICKUP_RADIUS`] of the ant, if any.
pub(crate) fn contact_food(ant_pos: Vec2, food_grid: &FoodGrid) -> Option<UVec2> {
    let center = world_to_grid(ant_pos)?;
    // +1 cell covers the ant's offset from its cell centre.
    let radius_cells = (FOOD_PICKUP_RADIUS / GRID_SIZE).ceil() as i32 + 1;
    let (cell, _) = food_grid.nearest_within(center, radius_cells)?;

    let close_enough = ant_pos.distance(grid_to_world(cell)) <= FOOD_PICKUP_RADIUS;
    let has_food = food_grid.amount(cell).is_some_and(|amount| amount > 0.0);

    (close_enough && has_food).then_some(cell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::world::INITIAL_FOOD_AMOUNT;

    fn food_grid_with(cell: UVec2) -> FoodGrid {
        let mut grid = FoodGrid::default();
        let mut world = World::new();
        let mut commands = world.commands();
        grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
        grid
    }

    #[test]
    fn food_is_only_sensed_in_the_forward_cone() {
        let ant = Ant::test_ant(0.0);
        let ahead = world_to_grid(Vec2::new(6.0, 0.0)).expect("in bounds");
        let grid = food_grid_with(ahead);
        assert!(sense_food(&ant, Vec2::ZERO, &grid).is_some());

        let behind = world_to_grid(Vec2::new(-6.0, 0.0)).expect("in bounds");
        let grid = food_grid_with(behind);
        assert!(sense_food(&ant, Vec2::ZERO, &grid).is_none());
    }

    #[test]
    fn food_is_only_sensed_within_range() {
        let ant = Ant::test_ant(0.0);
        let far = world_to_grid(Vec2::new(FOOD_SENSE_RANGE + GRID_SIZE, 0.0)).expect("in bounds");
        let grid = food_grid_with(far);
        assert!(sense_food(&ant, Vec2::ZERO, &grid).is_none());
    }

    #[test]
    fn pickup_needs_contact_and_remaining_food() {
        let contact = world_to_grid(Vec2::new(2.0, 0.0)).expect("in bounds");
        let grid = food_grid_with(contact);
        assert_eq!(contact_food(Vec2::ZERO, &grid), Some(contact));

        let far = world_to_grid(Vec2::new(20.0, 0.0)).expect("in bounds");
        let grid = food_grid_with(far);
        assert_eq!(contact_food(Vec2::ZERO, &grid), None);
    }
}
