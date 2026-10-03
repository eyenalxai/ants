//! Food pickup, nest dropoff and handling pauses.

use bevy::prelude::*;

use crate::constants::ant::CARRY_AMOUNT;
use crate::constants::world::NEST_RADIUS;
use crate::simulation::NestPosition;
use crate::simulation::ant::{Ant, AntPhase};
use crate::simulation::colony::ColonyStats;
use crate::simulation::food::FoodGrid;
use crate::simulation::movement::contact_food;

/// Pick up food on contact and drop it off at the nest. Both events start a
/// standing [`crate::constants::ant::HANDLING_TIME`] pause; pickup folds into
/// the normal steering path on the next movement step (no instant
/// re-orientation).
pub fn check_collisions(
    mut ant_query: Query<(&mut Ant, &Transform)>,
    nest_position: Res<NestPosition>,
    mut food_grid: ResMut<FoodGrid>,
    mut colony: ResMut<ColonyStats>,
    time: Res<Time<Fixed>>,
) {
    let nest_pos = nest_position.0;
    let nest_radius_squared = NEST_RADIUS * NEST_RADIUS;
    let dt = time.delta_secs();

    for (mut ant, transform) in &mut ant_query {
        if ant.is_handling() {
            continue;
        }

        let ant_pos = Vec2::new(transform.translation.x, transform.translation.y);

        if ant.has_food {
            if ant_pos.distance_squared(nest_pos) < nest_radius_squared {
                let delivered = ant.carrying.max(0.0);
                ant.carrying = 0.0;
                ant.has_food = false;
                ant.phase = AntPhase::Foraging;
                ant.trips_completed = ant.trips_completed.saturating_add(1);
                ant.start_handling();
                colony.record_delivery(delivered, dt);
            }

            continue;
        }

        if ant.phase == AntPhase::Nursing {
            continue;
        }

        if let Some(cell) = contact_food(ant_pos, &food_grid) {
            let taken = food_grid.take(cell, CARRY_AMOUNT);

            if taken > 0.0 {
                ant.carrying = taken;
                ant.has_food = true;
                ant.start_handling();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::grid::world_to_grid;
    use bevy::ecs::system::RunSystemOnce;

    fn test_world() -> World {
        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.insert_resource(NestPosition(Vec2::ZERO));
        world.init_resource::<FoodGrid>();
        world.init_resource::<ColonyStats>();
        world
    }

    #[test]
    fn pickup_stores_the_amount_actually_taken() {
        let mut world = test_world();
        let cell = world_to_grid(Vec2::new(2.0, 0.0)).expect("in bounds");

        // Only a quarter of a carry amount is left in the cell.
        let mut grid = FoodGrid::default();
        {
            let mut commands = world.commands();
            grid.set(&mut commands, cell, 0.25);
        }
        world.insert_resource(grid);

        let entity = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(2.0, 0.0, 0.0)))
            .id();

        world.run_system_once(check_collisions).unwrap();

        let ant = world.get::<Ant>(entity).expect("ant still alive");
        assert!(ant.has_food);
        assert_eq!(ant.carrying, 0.25, "the real taken amount must be stored");
    }

    #[test]
    fn dropoff_delivers_the_stored_carry_amount_and_clears_it() {
        let mut world = test_world();

        let mut ant = Ant::test_ant(0.0);
        ant.has_food = true;
        ant.carrying = 0.75;
        let entity = world.spawn((ant, Transform::from_xyz(0.0, 0.0, 0.0))).id();

        world.run_system_once(check_collisions).unwrap();

        let ant = world.get::<Ant>(entity).expect("ant still alive");
        assert!(!ant.has_food);
        assert_eq!(ant.carrying, 0.0);
        assert_eq!(ant.trips_completed, 1);
        assert!(ant.is_handling());

        let stats = world.resource::<ColonyStats>();
        assert!((stats.total_food_delivered - 0.75).abs() < 1e-6);
    }

    #[test]
    fn nursing_ants_do_not_pick_up_food() {
        let mut world = test_world();
        let cell = world_to_grid(Vec2::new(2.0, 0.0)).expect("in bounds");

        let mut grid = FoodGrid::default();
        {
            let mut commands = world.commands();
            grid.set(&mut commands, cell, 1.0);
        }
        world.insert_resource(grid);

        let mut ant = Ant::test_ant(0.0);
        ant.phase = AntPhase::Nursing;
        let entity = world.spawn((ant, Transform::from_xyz(2.0, 0.0, 0.0))).id();

        world.run_system_once(check_collisions).unwrap();

        let ant = world.get::<Ant>(entity).expect("ant still alive");
        assert!(!ant.has_food);
        assert_eq!(ant.carrying, 0.0);
        assert_eq!(
            world.resource::<FoodGrid>().amount(cell),
            Some(1.0),
            "nurse must leave the food untouched"
        );
    }
}
