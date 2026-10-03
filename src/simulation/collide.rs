//! Food pickup, nest dropoff and handling pauses.

use bevy::prelude::*;

use crate::constants::ant::CARRY_AMOUNT;
use crate::constants::lifecycle::{REST_AFTER, REST_DURATION};
use crate::core::grid::pack;
use crate::simulation::ant::{Ant, AntPhase, AntRng};
use crate::simulation::colony::{ColonyStats, NestStore};
use crate::simulation::food::FoodGrid;
use crate::simulation::movement::contact_food;
use crate::simulation::nest::NestGeometry;

/// Pick up food on contact and drop it off at the nest entrance. Both events
/// start a standing [`crate::constants::ant::HANDLING_TIME`] pause; pickup
/// folds into the normal steering path on the next movement step (no instant
/// re-orientation).
///
/// Amounts are honest end to end: pickup stores `min(crop_capacity, remaining)`
/// and the source cell's quality, the dropoff moves that same `carrying` value
/// into the nest store and the delivery counters, and clears the carry. A
/// dropoff may schedule a rest ([`REST_AFTER`] chance, [`REST_DURATION`]
/// seconds) from the ant's own deterministic RNG.
pub fn check_collisions(
    mut ant_query: Query<(&mut Ant, &Transform, Option<&mut AntRng>)>,
    nest_geometry: Res<NestGeometry>,
    mut food_grid: ResMut<FoodGrid>,
    mut colony: ResMut<ColonyStats>,
    mut nest_store: ResMut<NestStore>,
) {
    let entrance = nest_geometry.entrance;
    let entrance_radius_squared = nest_geometry.entrance_radius * nest_geometry.entrance_radius;

    for (mut ant, transform, ant_rng) in &mut ant_query {
        if ant.is_handling() {
            continue;
        }

        let ant_pos = transform.translation.truncate();

        if ant.is_laden() {
            if ant_pos.distance_squared(entrance) < entrance_radius_squared {
                let delivered = ant.deliver();
                nest_store.add(delivered);
                colony.record_delivery(delivered);

                if let Some(mut rng) = ant_rng
                    && rng.0.f32() < REST_AFTER
                {
                    ant.rest_timer = REST_DURATION;
                }
            }

            continue;
        }

        if ant.phase == AntPhase::Nursing {
            continue;
        }

        if let Some(cell) = contact_food(ant_pos, &food_grid) {
            // A full crop is CARRY_AMOUNT-sized; a zero crop falls back to the
            // nominal amount for hand-built ants.
            let request = if ant.crop_capacity > 0.0 {
                ant.crop_capacity
            } else {
                CARRY_AMOUNT
            };
            let taken = food_grid.take(cell, request);

            if taken > 0.0 {
                let quality = food_grid.quality(pack(cell));
                ant.pick_up(taken, quality);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::colony::NEST_STORE_CAP;
    use crate::constants::world::NEST_RADIUS;
    use crate::core::grid::world_to_grid;
    use bevy::ecs::system::RunSystemOnce;

    fn test_world() -> World {
        let mut world = World::new();
        world.insert_resource(NestGeometry {
            entrance: Vec2::ZERO,
            entrance_radius: NEST_RADIUS,
            refuse: Vec2::ZERO,
        });
        world.init_resource::<FoodGrid>();
        world.init_resource::<ColonyStats>();
        world.init_resource::<NestStore>();
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
    fn pickup_is_capped_by_the_crop_capacity_and_records_quality() {
        let mut world = test_world();
        let cell = world_to_grid(Vec2::new(2.0, 0.0)).expect("in bounds");

        let mut grid = FoodGrid::default();
        {
            let mut commands = world.commands();
            grid.set(&mut commands, cell, 50.0);
            grid.set_quality(pack(cell), 1.4);
        }
        world.insert_resource(grid);

        let mut ant = Ant::test_ant(0.0);
        ant.crop_capacity = 0.8;
        let entity = world.spawn((ant, Transform::from_xyz(2.0, 0.0, 0.0))).id();

        world.run_system_once(check_collisions).unwrap();

        let ant = world.get::<Ant>(entity).expect("ant still alive");
        assert!(
            (ant.carrying - 0.8).abs() < 1e-6,
            "the crop capacity must cap the pickup, got {}",
            ant.carrying
        );
        assert!(
            (ant.carrying_quality - 1.4).abs() < 1e-6,
            "the source quality must ride with the load"
        );

        // The cell keeps the rest.
        let remaining = world.resource::<FoodGrid>().amount(cell).unwrap();
        assert!((remaining - 49.2).abs() < 1e-6);
    }

    #[test]
    fn dropoff_delivers_the_stored_carry_amount_and_clears_it() {
        let mut world = test_world();

        let mut ant = Ant::test_ant(0.0);
        ant.pick_up(0.75, 1.0);
        // Pickup starts a handling pause; clear it so the dropoff runs now.
        ant.handling_timer = 0.0;
        let entity = world.spawn((ant, Transform::from_xyz(0.0, 0.0, 0.0))).id();

        world.run_system_once(check_collisions).unwrap();

        let ant = world.get::<Ant>(entity).expect("ant still alive");
        assert!(!ant.has_food);
        assert_eq!(ant.carrying, 0.0);
        assert_eq!(ant.trips_completed, 1);
        assert!(ant.is_handling());

        let stats = world.resource::<ColonyStats>();
        assert!((stats.total_food_delivered - 0.75).abs() < 1e-6);
        assert!((stats.delivered_this_tick - 0.75).abs() < 1e-6);

        let store = world.resource::<NestStore>();
        assert!(
            (store.food() - (crate::constants::colony::NEST_STORE_INITIAL + 0.75)).abs() < 1e-6
        );
    }

    #[test]
    fn partial_pickup_is_delivered_honestly() {
        let mut world = test_world();
        let cell = world_to_grid(Vec2::new(2.0, 0.0)).expect("in bounds");

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

        // Walk the ant to the nest and drop the partial load off.
        world
            .get_mut::<Transform>(entity)
            .expect("ant transform")
            .translation
            .x = 0.0;
        world.get_mut::<Ant>(entity).expect("ant").handling_timer = 0.0;
        world.run_system_once(check_collisions).unwrap();

        let stats = world.resource::<ColonyStats>();
        assert!(
            (stats.total_food_delivered - 0.25).abs() < 1e-6,
            "a partial pickup must deliver exactly what was taken"
        );
    }

    #[test]
    fn dropoff_clamps_at_the_store_cap_but_counts_the_delivery() {
        let mut world = test_world();
        world.insert_resource(NestStore::with_food(NEST_STORE_CAP - 0.5));

        let mut ant = Ant::test_ant(0.0);
        ant.pick_up(0.75, 1.0);
        ant.handling_timer = 0.0;
        world.spawn((ant, Transform::from_xyz(0.0, 0.0, 0.0)));

        world.run_system_once(check_collisions).unwrap();

        let store = world.resource::<NestStore>();
        assert!((store.food() - NEST_STORE_CAP).abs() < 1e-6);
        let stats = world.resource::<ColonyStats>();
        assert!((stats.total_food_delivered - 0.75).abs() < 1e-6);
    }

    #[test]
    fn dropoff_requires_the_entrance_radius() {
        let mut world = test_world();
        world.insert_resource(NestGeometry {
            entrance: Vec2::new(10.0, 0.0),
            entrance_radius: 5.0,
            refuse: Vec2::new(10.0, 0.0),
        });

        // Outside the entrance radius: no delivery.
        let mut far = Ant::test_ant(0.0);
        far.pick_up(0.5, 1.0);
        far.handling_timer = 0.0;
        let far_id = world.spawn((far, Transform::from_xyz(0.0, 0.0, 0.0))).id();

        // Inside the entrance radius: delivery.
        let mut near = Ant::test_ant(0.0);
        near.pick_up(0.5, 1.0);
        near.handling_timer = 0.0;
        let near_id = world
            .spawn((near, Transform::from_xyz(12.0, 0.0, 0.0)))
            .id();

        world.run_system_once(check_collisions).unwrap();

        assert!(
            world.get::<Ant>(far_id).unwrap().is_laden(),
            "an ant outside the entrance radius must keep its load"
        );
        assert!(
            !world.get::<Ant>(near_id).unwrap().is_laden(),
            "an ant inside the entrance radius must deliver"
        );
        assert!((world.resource::<ColonyStats>().total_food_delivered - 0.5).abs() < 1e-6);
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

    #[test]
    fn dropoff_can_schedule_a_deterministic_rest() {
        // With a real AntRng the rest decision is either taken or not, and the
        // timer only ever holds 0 or REST_DURATION.
        let mut world = test_world();
        let mut ant = Ant::test_ant(0.0);
        ant.pick_up(1.0, 1.0);
        let entity = world
            .spawn((
                ant,
                Transform::from_xyz(0.0, 0.0, 0.0),
                AntRng::for_spawn(7),
            ))
            .id();

        world.run_system_once(check_collisions).unwrap();

        let rest = world.get::<Ant>(entity).unwrap().rest_timer;
        assert!(
            rest == 0.0 || (rest - REST_DURATION).abs() < 1e-6,
            "rest timer must be 0 or REST_DURATION, got {rest}"
        );
    }
}
