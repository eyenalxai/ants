//! Food pickup, nest dropoff and handling pauses.

use bevy::prelude::*;

use crate::constants::ant::CARRY_AMOUNT;
use crate::constants::world::NEST_RADIUS;
use crate::simulation::Nest;
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
    nest_query: Query<&Transform, With<Nest>>,
    mut food_grid: ResMut<FoodGrid>,
    mut colony: ResMut<ColonyStats>,
    time: Res<Time<Fixed>>,
) {
    let Some(nest_transform) = nest_query.iter().next() else {
        return;
    };
    let nest_pos = Vec2::new(nest_transform.translation.x, nest_transform.translation.y);
    let dt = time.delta_secs();

    for (mut ant, transform) in &mut ant_query {
        if ant.is_handling() {
            continue;
        }

        let ant_pos = Vec2::new(transform.translation.x, transform.translation.y);

        if ant.has_food {
            if ant_pos.distance(nest_pos) < NEST_RADIUS {
                ant.has_food = false;
                ant.phase = AntPhase::Foraging;
                ant.trips_completed = ant.trips_completed.saturating_add(1);
                ant.start_handling();
                colony.record_delivery(CARRY_AMOUNT, dt);
            }

            continue;
        }

        if ant.phase == AntPhase::Nursing {
            continue;
        }

        if let Some(cell) = contact_food(ant_pos, &food_grid) {
            let taken = food_grid.take(cell, CARRY_AMOUNT);

            if taken > 0.0 {
                ant.has_food = true;
                ant.start_handling();
            }
        }
    }
}
