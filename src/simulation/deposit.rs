//! Serial pheromone deposit pass, run once per fixed step after movement.

use bevy::prelude::*;

use crate::constants::ant::{
    DEPOSIT_BASE_MULTIPLIER, DEPOSIT_SAMPLES, DEPOSIT_SUCCESS_BONUS, DEPOSIT_SUCCESS_TRIPS_CAP,
};
use crate::constants::pheromone::PHEROMONE_DEPOSIT_RATE;
use crate::constants::world::{DENSITY_DEPOSIT_SUPPRESSION, DENSITY_DEPOSIT_SUPPRESSION_FLOOR};
use crate::core::grid::world_to_grid;
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::ant::{Ant, AntPhase};
use crate::simulation::density::AntDensity;

/// Deposit multiplier grows with completed trips so successful foragers
/// reinforce trails more strongly. It never depends on remaining lifetime.
pub fn deposit_multiplier(trips_completed: u32) -> f32 {
    let progress =
        trips_completed.min(DEPOSIT_SUCCESS_TRIPS_CAP) as f32 / DEPOSIT_SUCCESS_TRIPS_CAP as f32;
    DEPOSIT_BASE_MULTIPLIER + DEPOSIT_SUCCESS_BONUS * progress
}

/// Deposit suppression for a density-cell occupancy count,
/// `max(1 / (1 + k * n), floor)`. The floor guarantees that even a very
/// crowded cell — an emerging trail, the nest mouth — still receives deposits.
pub fn density_suppression(occupancy: f32) -> f32 {
    (1.0 / (1.0 + DENSITY_DEPOSIT_SUPPRESSION * occupancy.max(0.0)))
        .max(DENSITY_DEPOSIT_SUPPRESSION_FLOOR)
}

/// Occupancy as seen by the depositing ant. The density grid is rebuilt from
/// all ants, so it always contains the ant itself; a lone ant must deposit a
/// full-strength trace.
pub fn neighbor_occupancy(sampled: u32) -> u32 {
    sampled.saturating_sub(1)
}

/// Lay pheromones at several points along the last movement segment so trails
/// do not develop gaps. Outbound ants write `ToNest`, laden ants write
/// `ToFood`; nurses and handling ants do not deposit.
pub fn deposit_pheromones(
    ant_query: Query<(&Ant, &Transform)>,
    density: Res<AntDensity>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();

    for (ant, transform) in &ant_query {
        if ant.is_handling() || ant.phase == AntPhase::Nursing {
            continue;
        }

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        let (sin, cos) = ant.direction.sin_cos();
        let start = pos - Vec2::new(cos, sin) * ant.speed * dt;

        let multiplier = deposit_multiplier(ant.trips_completed);
        let per_sample = PHEROMONE_DEPOSIT_RATE * dt * multiplier / DEPOSIT_SAMPLES as f32;
        let (to_food, to_nest) = if ant.has_food { (1.0, 0.0) } else { (0.0, 1.0) };

        // The density grid counts the depositing ant itself; a lone ant must
        // lay a full-strength trace, so subtract self before suppressing.
        let others = neighbor_occupancy(density.sample(pos)) as f32;
        let suppression = density_suppression(others);

        for sample_index in 0..DEPOSIT_SAMPLES {
            let t = (sample_index as f32 + 0.5) / DEPOSIT_SAMPLES as f32;
            let sample_pos = start.lerp(pos, t);
            let Some(cell) = world_to_grid(sample_pos) else {
                continue;
            };

            let amount = per_sample * suppression;
            pheromone_grid.add_kernel(cell, amount * to_food, amount * to_nest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    #[test]
    fn deposit_multiplier_grows_with_trips_and_caps() {
        assert!((deposit_multiplier(0) - DEPOSIT_BASE_MULTIPLIER).abs() < EPS);

        let first = deposit_multiplier(1);
        let second = deposit_multiplier(2);
        let third = deposit_multiplier(3);

        assert!(first > DEPOSIT_BASE_MULTIPLIER);
        assert!(second > first);
        assert!((third - (DEPOSIT_BASE_MULTIPLIER + DEPOSIT_SUCCESS_BONUS)).abs() < EPS);
        assert!((deposit_multiplier(99) - third).abs() < EPS);
    }

    #[test]
    fn density_suppression_decreases_monotonically_to_a_floor() {
        assert!((density_suppression(0.0) - 1.0).abs() < EPS);

        let mut previous = density_suppression(0.0);
        for occupancy in 1..=50 {
            let current = density_suppression(occupancy as f32);
            assert!(
                current <= previous,
                "suppression must not rise at occupancy {occupancy}"
            );
            assert!(current >= DENSITY_DEPOSIT_SUPPRESSION_FLOOR - EPS);
            previous = current;
        }

        // Extreme crowding never silences deposits completely.
        assert!((density_suppression(10_000.0) - DENSITY_DEPOSIT_SUPPRESSION_FLOOR).abs() < EPS);
    }

    #[test]
    fn neighbor_occupancy_excludes_the_depositing_ant() {
        assert_eq!(neighbor_occupancy(0), 0);
        assert_eq!(neighbor_occupancy(1), 0, "a lone ant sees no neighbours");
        assert_eq!(neighbor_occupancy(5), 4);
        // A lone ant therefore deposits at full strength.
        assert!((density_suppression(neighbor_occupancy(1) as f32) - 1.0).abs() < EPS);
    }
}
