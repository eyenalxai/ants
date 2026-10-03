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

        // Ants move well under one 4-unit cell per tick, so most of the
        // DEPOSIT_SAMPLES points share a cell. Accumulate per distinct cell
        // and apply one kernel per cell: the kernel is linear in amount, so
        // this is the naive per-sample result modulo float associativity, with
        // up to DEPOSIT_SAMPLES times fewer kernel applications.
        let mut samples = [(UVec2::ZERO, 0.0f32); DEPOSIT_SAMPLES];
        let mut distinct = 0;

        for sample_index in 0..DEPOSIT_SAMPLES {
            let t = (sample_index as f32 + 0.5) / DEPOSIT_SAMPLES as f32;
            let sample_pos = start.lerp(pos, t);
            let Some(cell) = world_to_grid(sample_pos) else {
                continue;
            };

            let amount = per_sample * suppression;

            if let Some(sample) = samples[..distinct]
                .iter_mut()
                .find(|(other, _)| *other == cell)
            {
                sample.1 += amount;
            } else {
                samples[distinct] = (cell, amount);
                distinct += 1;
            }
        }

        for (cell, amount) in &samples[..distinct] {
            pheromone_grid.add_kernel(*cell, amount * to_food, amount * to_nest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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

    /// The deduped pass must produce the same grid as the old per-sample path
    /// (within float tolerance) for a mix of ants that share cells and ants
    /// that straddle cell boundaries.
    #[test]
    fn deduped_deposit_matches_the_naive_per_sample_grid() {
        use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};
        use bevy::ecs::system::RunSystemOnce;

        let dt = 1.0 / 64.0;
        let mut specs = Vec::new();

        for index in 0..24 {
            specs.push((
                index as f32 * 0.37,
                20.0 + index as f32,
                Vec2::new(-60.0 + index as f32 * 5.0, 30.0 - index as f32 * 2.5),
            ));
        }

        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.insert_resource(PheromoneGrid::new());

        let mut density = AntDensity::new();
        for (index, (direction, speed, pos)) in specs.iter().enumerate() {
            let mut ant = Ant::test_ant(*direction);
            ant.speed = *speed;
            ant.has_food = index % 2 == 0;
            ant.trips_completed = (index % 4) as u32;
            world.spawn((ant, Transform::from_xyz(pos.x, pos.y, 0.0)));

            density.add(*pos);
            if index % 3 == 0 {
                // Extra neighbours so suppression actually bites.
                density.add(*pos + Vec2::new(2.0, 2.0));
            }
        }
        world.insert_resource(density);

        // Bare worlds do not advance `Time<Fixed>`; give the system the same
        // 1/64 s step the real chain uses.
        world
            .resource_mut::<Time<Fixed>>()
            .advance_by(Duration::from_secs_f32(dt));
        world.run_system_once(deposit_pheromones).unwrap();

        // Re-run the same arithmetic one sample at a time into a fresh grid.
        let mut ant_data = Vec::new();
        {
            let mut query = world.query::<(&Ant, &Transform)>();
            for (ant, transform) in query.iter(&world) {
                ant_data.push((
                    ant.direction,
                    ant.speed,
                    ant.has_food,
                    ant.trips_completed,
                    Vec2::new(transform.translation.x, transform.translation.y),
                ));
            }
        }

        let deposited = world.resource::<PheromoneGrid>();
        let density = world.resource::<AntDensity>();
        let mut expected = PheromoneGrid::new();

        for (direction, speed, has_food, trips, pos) in ant_data {
            let (sin, cos) = direction.sin_cos();
            let start = pos - Vec2::new(cos, sin) * speed * dt;
            let multiplier = deposit_multiplier(trips);
            let per_sample = PHEROMONE_DEPOSIT_RATE * dt * multiplier / DEPOSIT_SAMPLES as f32;
            let (to_food, to_nest) = if has_food { (1.0, 0.0) } else { (0.0, 1.0) };
            let suppression = density_suppression(neighbor_occupancy(density.sample(pos)) as f32);

            for sample_index in 0..DEPOSIT_SAMPLES {
                let t = (sample_index as f32 + 0.5) / DEPOSIT_SAMPLES as f32;
                let sample_pos = start.lerp(pos, t);
                if let Some(cell) = world_to_grid(sample_pos) {
                    let amount = per_sample * suppression;
                    expected.add_kernel(cell, amount * to_food, amount * to_nest);
                }
            }
        }

        let mut populated = 0;

        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                let cell = UVec2::new(x, y);
                let got = deposited.get(cell).copied().unwrap_or_default();
                let want = expected.get(cell).copied().unwrap_or_default();

                assert!(
                    (got.to_food - want.to_food).abs() < 1e-4
                        && (got.to_nest - want.to_nest).abs() < 1e-4,
                    "cell {cell:?}: got {got:?}, want {want:?}"
                );

                if want.to_food > 0.0 || want.to_nest > 0.0 {
                    populated += 1;
                }
            }
        }

        assert!(populated > 0, "the equivalence test must deposit something");
    }

    /// When every sample shares a cell the deduped pass applies exactly one
    /// kernel, so the grid mass equals the full per-tick deposit.
    #[test]
    fn deduped_deposit_preserves_mass_when_samples_share_a_cell() {
        use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};
        use crate::pheromone::grid::PheromoneKind;
        use bevy::ecs::system::RunSystemOnce;

        let dt = 1.0 / 64.0;
        let pos = Vec2::new(10.0, 10.0);
        let mut ant = Ant::test_ant(0.0);
        ant.speed = 1.0;

        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.insert_resource(PheromoneGrid::new());

        let mut density = AntDensity::new();
        density.add(pos);
        world.insert_resource(density);
        world.spawn((ant, Transform::from_xyz(pos.x, pos.y, 0.0)));

        world
            .resource_mut::<Time<Fixed>>()
            .advance_by(Duration::from_secs_f32(dt));
        world.run_system_once(deposit_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        let total: f32 = (0..GRID_HEIGHT as u32)
            .flat_map(|y| (0..GRID_WIDTH as u32).map(move |x| UVec2::new(x, y)))
            .map(|cell| grid.sample(cell, PheromoneKind::ToNest))
            .sum();

        let expected = PHEROMONE_DEPOSIT_RATE * dt * DEPOSIT_BASE_MULTIPLIER;
        assert!(
            (total - expected).abs() < 1e-4,
            "mass should be {expected}, got {total}"
        );
    }
}
