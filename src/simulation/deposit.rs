//! Pheromone deposit pass, run once per fixed step after movement.
//!
//! The pass has two layers:
//!
//! - **Recruitment** (`ToFood`): only a laden ant lays it (the load is the
//!   signal; an outbound ant with completed trips marks only the home range).
//!   It is scaled by load quality, distance from the nest and how strongly the
//!   followed trail already reads, so trail strength encodes food value.
//! - **Home-range mark** (`ToNest`): every forager lays a weak mark at
//!   [`PHEROMONE_TO_NEST_BASE`]. It is deliberately not a recruitment trail;
//!   gating it on success would make the frozen no-food regression (which
//!   requires a non-empty `ToNest` field without any delivery) impossible.
//!
//! Rows are collected once, accumulated into per-band dense fields in parallel,
//! merged in fixed band order and applied through the 3x3 kernel by
//! [`PheromoneGrid::apply_dense_deposits`]; the whole pass is bit-deterministic
//! and allocation-free per tick in steady state.

use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;

use crate::constants::ant::{
    DEPOSIT_BASE_MULTIPLIER, DEPOSIT_SAMPLES, DEPOSIT_SUCCESS_BONUS, DEPOSIT_SUCCESS_TRIPS_CAP,
};
use crate::constants::pheromone::{
    PHEROMONE_DEPOSIT_RATE, PHEROMONE_DISTANCE_BASE, PHEROMONE_DISTANCE_SCALE,
    PHEROMONE_NEST_PROXIMITY_FLOOR, PHEROMONE_QUALITY_BASE, PHEROMONE_QUALITY_MAX,
    PHEROMONE_QUALITY_SCALE, PHEROMONE_TO_NEST_BASE, PHEROMONE_TRAIL_SUPPRESSION,
};
use crate::constants::world::{
    DENSITY_DEPOSIT_SUPPRESSION, DENSITY_DEPOSIT_SUPPRESSION_FLOOR, GRID_HEIGHT, GRID_WIDTH,
    NEST_RADIUS,
};
use crate::core::grid::world_to_grid;
use crate::pheromone::grid::{ACTIVE_WORDS, CELL_COUNT, PheromoneGrid, PheromoneKind};
use crate::simulation::NestPosition;
use crate::simulation::ant::{Ant, AntPhase};
use crate::simulation::density::AntDensity;
use crate::simulation::movement::sensors::normalized_intensity;

/// Deposit multiplier grows with completed trips so successful foragers
/// reinforce trails more strongly. It never depends on remaining lifetime.
pub fn deposit_multiplier(trips_completed: u32) -> f32 {
    let progress =
        trips_completed.min(DEPOSIT_SUCCESS_TRIPS_CAP) as f32 / DEPOSIT_SUCCESS_TRIPS_CAP as f32;
    DEPOSIT_BASE_MULTIPLIER + DEPOSIT_SUCCESS_BONUS * progress
}

/// Whether the ant has earned the recruitment signal (`is_laden() ||
/// trips_completed > 0`). Naive foragers lay only the weak home-range mark.
///
/// The recruitment gate is applied to the `ToFood` channel only: a laden ant
/// is the one carrying the signal, and the `ToNest` home-range mark is not
/// recruitment, so it stays available to every forager (the frozen no-food
/// regression requires a non-empty `ToNest` field without any delivery).
pub fn is_recruitment_eligible(ant: &Ant) -> bool {
    ant.is_laden() || ant.trips_completed > 0
}

/// Quality scaling of a laden ant's recruitment deposit:
/// `0.5 + 0.5 * clamp(carrying_quality, 0, 2)`, i.e. 0.75 for poor food and
/// 1.0 for average food.
pub fn quality_factor(carrying_quality: f32) -> f32 {
    PHEROMONE_QUALITY_BASE
        + PHEROMONE_QUALITY_SCALE * carrying_quality.clamp(0.0, PHEROMONE_QUALITY_MAX)
}

/// Nest-distance scaling of a laden ant's recruitment deposit:
/// `0.3 + 0.7 * clamp(dist_to_nest / route_length, 0, 1)`, so more pheromone
/// is laid near the food and less near the nest (Czaczkes et al. 2024).
///
/// `route_memory` is the nest-relative vector to the last food pickup. When it
/// is `None` (the ant has not discovered food yet, or navigation has not set
/// it) the factor falls back to the documented neutral `1.0`, which keeps the
/// first laden return trip at full bootstrap strength.
pub fn distance_factor(route_memory: Option<Vec2>, pos: Vec2, nest_pos: Vec2) -> f32 {
    match route_memory {
        Some(route) if route.length_squared() > f32::EPSILON => {
            let progress = (pos - nest_pos).length() / route.length();
            PHEROMONE_DISTANCE_BASE + PHEROMONE_DISTANCE_SCALE * progress.clamp(0.0, 1.0)
        }
        _ => 1.0,
    }
}

/// Suppression of a deposit on a route the ant can already follow:
/// `1 / (1 + k * followed_strength)`, so an established trail is not
/// double-marked and reinforcement spreads to unmarked ground.
pub fn trail_suppression(followed_strength: f32) -> f32 {
    1.0 / (1.0 + PHEROMONE_TRAIL_SUPPRESSION * followed_strength.clamp(0.0, 1.0))
}

/// Local strength `[0, 1)` of the channel an ant with this follow mode reads,
/// on the same perception curve the sensors use.
pub fn followed_strength(follows_to_nest: bool, pos: Vec2, pheromone_grid: &PheromoneGrid) -> f32 {
    let kind = if follows_to_nest {
        PheromoneKind::ToNest
    } else {
        PheromoneKind::ToFood
    };

    world_to_grid(pos).map_or(0.0, |cell| {
        normalized_intensity(pheromone_grid.sample(cell, kind))
    })
}

/// [`followed_strength`] for a live [`Ant`].
pub fn ant_followed_strength(ant: &Ant, pos: Vec2, pheromone_grid: &PheromoneGrid) -> f32 {
    followed_strength(ant.follows_to_nest(), pos, pheromone_grid)
}

/// Multiplier of a laden ant's recruitment deposit from raw inputs:
/// success x quality x distance. The first laden return trip has
/// `trips_completed == 0` and no route memory, so it keeps the full bootstrap
/// strength.
pub fn laden_multiplier(
    trips_completed: u32,
    carrying_quality: f32,
    route_memory: Option<Vec2>,
    pos: Vec2,
    nest_pos: Vec2,
) -> f32 {
    deposit_multiplier(trips_completed)
        * quality_factor(carrying_quality)
        * distance_factor(route_memory, pos, nest_pos)
}

/// [`laden_multiplier`] for a live [`Ant`].
pub fn laden_deposit_multiplier(ant: &Ant, pos: Vec2, nest_pos: Vec2) -> f32 {
    laden_multiplier(
        ant.trips_completed,
        ant.carrying_quality,
        ant.route_memory,
        pos,
        nest_pos,
    )
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

/// Nest-proximity suppression for outbound ants: `clamp(d / 2R, 0, 1)` with a
/// bootstrap floor, so no recruitment pool forms at the nest mouth.
///
/// The density tax is confined to the nest disc (where traffic really piles
/// up); outside it, positive reinforcement is restored and busy routes are
/// rewarded instead of globally penalized.
pub fn nest_proximity_suppression(pos: Vec2, nest_pos: Vec2, occupancy: u32) -> f32 {
    let dist = (pos - nest_pos).length();
    let proximity = (dist / (2.0 * NEST_RADIUS))
        .clamp(0.0, 1.0)
        .max(PHEROMONE_NEST_PROXIMITY_FLOOR);

    if dist < NEST_RADIUS {
        proximity * density_suppression(neighbor_occupancy(occupancy) as f32)
    } else {
        proximity
    }
}

/// Multiplier of the weak `ToNest` home-range mark.
pub fn home_range_multiplier(pos: Vec2, nest_pos: Vec2, occupancy: u32) -> f32 {
    PHEROMONE_TO_NEST_BASE * nest_proximity_suppression(pos, nest_pos, occupancy)
}

/// One ant's deposit inputs, collected before the parallel accumulation so
/// every band reads the same immutable snapshot.
#[derive(Clone, Copy)]
struct AntDepositRow {
    prev_pos: Vec2,
    pos: Vec2,
    laden: bool,
    follows_to_nest: bool,
    trips_completed: u32,
    carrying_quality: f32,
    route_memory: Option<Vec2>,
}

/// Dense deposit field of one band plus the cells the band wrote, so the merge
/// only visits touched cells.
#[derive(Default)]
struct BandScratch {
    to_food: Vec<f32>,
    to_nest: Vec<f32>,
    touched: Vec<u32>,
}

/// Reused deposit buffers. `Local` keeps them alive across ticks, so the
/// deposit pass allocates nothing per tick in steady state.
#[derive(Default)]
pub struct DepositScratch {
    rows: Vec<AntDepositRow>,
    bands: Vec<BandScratch>,
    merged_food: Vec<f32>,
    merged_nest: Vec<f32>,
    merged_touched: Vec<u32>,
    band_bits: Vec<u64>,
}

impl DepositScratch {
    /// Start a tick: clear the row list and make sure every buffer is sized
    /// for `bands` bands. The dense fields were zeroed by the previous merge.
    fn prepare(&mut self, bands: usize) {
        self.rows.clear();
        self.merged_touched.clear();

        if self.merged_food.len() != CELL_COUNT {
            self.merged_food.resize(CELL_COUNT, 0.0);
            self.merged_nest.resize(CELL_COUNT, 0.0);
        }

        while self.bands.len() < bands {
            self.bands.push(BandScratch::default());
        }
        self.bands.truncate(bands);

        for band in &mut self.bands {
            if band.to_food.len() != CELL_COUNT {
                band.to_food.resize(CELL_COUNT, 0.0);
                band.to_nest.resize(CELL_COUNT, 0.0);
            }
        }

        let words = bands * ACTIVE_WORDS;

        if self.band_bits.len() != words {
            self.band_bits.resize(words, 0);
        }
    }
}

/// Number of row bands: one per compute thread (capped at the grid height),
/// or one when no compute pool exists.
fn deposit_bands() -> usize {
    ComputeTaskPool::try_get().map_or(1, |pool| pool.thread_num().min(GRID_HEIGHT))
}

/// Accumulate one band's rows into its private dense field. Reads only
/// immutable state (`grid`, `density`), so bands never race.
fn accumulate_band(
    rows: &[AntDepositRow],
    band: &mut BandScratch,
    grid: &PheromoneGrid,
    density: &AntDensity,
    nest_pos: Vec2,
    per_sample: f32,
) {
    for row in rows {
        let suppression = trail_suppression(followed_strength(row.follows_to_nest, row.pos, grid));

        let (to_food, to_nest) = if row.laden {
            // Recruitment trail: a laden ant carries the signal, so it is
            // always `is_recruitment_eligible`. Scaled by load value and by
            // how far from the nest the ant already is.
            (
                laden_multiplier(
                    row.trips_completed,
                    row.carrying_quality,
                    row.route_memory,
                    row.pos,
                    nest_pos,
                ) * suppression,
                0.0,
            )
        } else {
            // Home-range mark: weak, nest-suppressed and never gated on
            // success (it is not recruitment; see the module docs).
            (
                0.0,
                home_range_multiplier(row.pos, nest_pos, density.sample(row.pos)) * suppression,
            )
        };

        if to_food <= 0.0 && to_nest <= 0.0 {
            continue;
        }

        // Ants move well under one 4-unit cell per tick, so the sample points
        // usually share a cell. Accumulating straight into the band field
        // folds duplicates for free; the kernel is applied once per cell in
        // the parallel pass below.
        for sample_index in 0..DEPOSIT_SAMPLES {
            let t = (sample_index as f32 + 0.5) / DEPOSIT_SAMPLES as f32;
            let sample_pos = row.prev_pos.lerp(row.pos, t);

            if let Some(cell) = world_to_grid(sample_pos) {
                let index = cell.y as usize * GRID_WIDTH + cell.x as usize;

                if band.to_food[index] == 0.0 && band.to_nest[index] == 0.0 {
                    band.touched.push(index as u32);
                }

                band.to_food[index] += per_sample * to_food;
                band.to_nest[index] += per_sample * to_nest;
            }
        }
    }
}

/// Merge band fields into the shared dense field in fixed band order, which
/// keeps the per-cell summation order (and therefore the result)
/// deterministic no matter how the scheduler interleaved the bands. Band
/// fields are zeroed again for the next tick.
fn merge_bands(
    bands: &mut [BandScratch],
    merged_food: &mut [f32],
    merged_nest: &mut [f32],
    merged_touched: &mut Vec<u32>,
) {
    for band in bands {
        for &touched in &band.touched {
            let index = touched as usize;

            if merged_food[index] == 0.0 && merged_nest[index] == 0.0 {
                merged_touched.push(touched);
            }

            merged_food[index] += band.to_food[index];
            merged_nest[index] += band.to_nest[index];
            band.to_food[index] = 0.0;
            band.to_nest[index] = 0.0;
        }

        band.touched.clear();
    }
}

/// Lay pheromones along the ant's real movement segment (`prev_pos -> pos`, so
/// wall bounces cannot rotate the deposited kernel). Laden ants write the
/// value-scaled `ToFood` recruitment trail; every forager writes the weak
/// `ToNest` home-range mark; nurses and handling ants do not deposit.
///
/// Rows are collected once, then accumulated by fixed chunks in parallel
/// (per-band dense fields) and merged in fixed band order, so the result is
/// bit-deterministic and the per-tick cost is bounded by the slowest band.
pub fn deposit_pheromones(
    ant_query: Query<(&Ant, &Transform)>,
    density: Res<AntDensity>,
    nest_position: Res<NestPosition>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    time: Res<Time<Fixed>>,
    mut scratch: Local<DepositScratch>,
) {
    let dt = time.delta_secs();
    let bands = deposit_bands();
    scratch.prepare(bands);

    let DepositScratch {
        rows,
        bands: band_scratch,
        merged_food,
        merged_nest,
        merged_touched,
        band_bits,
    } = &mut *scratch;

    for (ant, transform) in &ant_query {
        if ant.is_handling() || ant.phase == AntPhase::Nursing {
            continue;
        }

        rows.push(AntDepositRow {
            prev_pos: ant.prev_pos,
            pos: Vec2::new(transform.translation.x, transform.translation.y),
            laden: ant.is_laden(),
            follows_to_nest: ant.follows_to_nest(),
            trips_completed: ant.trips_completed,
            carrying_quality: ant.carrying_quality,
            route_memory: ant.route_memory,
        });
    }

    if rows.is_empty() {
        return;
    }

    let nest_pos = nest_position.0;
    let per_sample = PHEROMONE_DEPOSIT_RATE * dt / DEPOSIT_SAMPLES as f32;
    let chunk = rows.len().div_ceil(bands).max(1);
    let grid: &PheromoneGrid = &pheromone_grid;
    let density: &AntDensity = &density;

    if let Some(pool) = ComputeTaskPool::try_get() {
        pool.scope(|scope| {
            for (band_rows, band) in rows.chunks(chunk).zip(band_scratch.iter_mut()) {
                scope.spawn(async move {
                    accumulate_band(band_rows, band, grid, density, nest_pos, per_sample);
                });
            }
        });
    } else {
        // No compute pool (bare test worlds): the same bands run serially.
        for (band_rows, band) in rows.chunks(chunk).zip(band_scratch.iter_mut()) {
            accumulate_band(band_rows, band, grid, density, nest_pos, per_sample);
        }
    }

    merge_bands(band_scratch, merged_food, merged_nest, merged_touched);

    pheromone_grid.apply_dense_deposits(merged_food, merged_nest, band_bits);

    for &touched in merged_touched.iter() {
        let index = touched as usize;
        merged_food[index] = 0.0;
        merged_nest[index] = 0.0;
    }

    merged_touched.clear();
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
    fn recruitment_is_gated_on_a_load_or_a_completed_trip() {
        let mut ant = Ant::test_ant(0.0);
        assert!(!is_recruitment_eligible(&ant));

        ant.trips_completed = 1;
        assert!(is_recruitment_eligible(&ant));

        let mut laden = Ant::test_ant(0.0);
        laden.carrying = 0.5;
        assert!(is_recruitment_eligible(&laden));
    }

    /// Observable contract of the gate: a naive forager lays no `ToFood`
    /// recruitment signal (only the weak `ToNest` home-range mark), while a
    /// laden ant lays `ToFood` and no home-range mark.
    #[test]
    fn naive_foragers_lay_only_the_home_range_mark() {
        use crate::pheromone::grid::PheromoneKind;
        use bevy::ecs::system::RunSystemOnce;

        let dt = 1.0 / 64.0;
        let nest = Vec2::new(-100.0, 0.0);
        let naive_pos = Vec2::new(50.0, 10.0);
        let laden_pos = Vec2::new(50.0, 20.0);

        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.insert_resource(PheromoneGrid::new());
        world.insert_resource(NestPosition(nest));

        let mut naive = Ant::test_ant(0.0);
        naive.prev_pos = naive_pos - Vec2::new(1.0, 0.0);
        world.spawn((naive, Transform::from_xyz(naive_pos.x, naive_pos.y, 0.0)));

        let mut laden = Ant::test_ant(0.0);
        laden.carrying = 1.0;
        laden.has_food = true;
        laden.prev_pos = laden_pos - Vec2::new(1.0, 0.0);
        world.spawn((laden, Transform::from_xyz(laden_pos.x, laden_pos.y, 0.0)));

        let mut density = AntDensity::new();
        density.add(naive_pos);
        density.add(laden_pos);
        world.insert_resource(density);

        world
            .resource_mut::<Time<Fixed>>()
            .advance_by(Duration::from_secs_f32(dt));
        world.run_system_once(deposit_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        let naive_cell = world_to_grid(naive_pos).unwrap();
        let laden_cell = world_to_grid(laden_pos).unwrap();

        assert_eq!(
            grid.sample(naive_cell, PheromoneKind::ToFood),
            0.0,
            "a naive forager must not recruit"
        );
        assert!(
            grid.sample(naive_cell, PheromoneKind::ToNest) > 0.0,
            "the home-range mark must still be laid"
        );
        assert!(
            grid.sample(laden_cell, PheromoneKind::ToFood) > 0.0,
            "a laden ant must recruit"
        );
        assert_eq!(
            grid.sample(laden_cell, PheromoneKind::ToNest),
            0.0,
            "a laden ant does not lay the home-range mark"
        );
    }

    #[test]
    fn quality_factor_scales_from_poor_to_rich_food() {
        assert!((quality_factor(1.0) - 1.0).abs() < EPS);
        assert!((quality_factor(0.5) - 0.75).abs() < EPS);
        assert!((quality_factor(2.0) - 1.5).abs() < EPS);
        // Out-of-range quality is clamped, never amplified without bound.
        assert!((quality_factor(100.0) - quality_factor(PHEROMONE_QUALITY_MAX)).abs() < EPS);
        assert!((quality_factor(-3.0) - PHEROMONE_QUALITY_BASE).abs() < EPS);
    }

    #[test]
    fn distance_factor_grows_from_nest_to_food_and_defaults_to_neutral() {
        let nest = Vec2::ZERO;
        let route = Some(Vec2::new(100.0, 0.0));

        assert!((distance_factor(route, nest, nest) - PHEROMONE_DISTANCE_BASE).abs() < EPS);
        assert!((distance_factor(route, Vec2::new(100.0, 0.0), nest) - 1.0).abs() < EPS);
        assert!((distance_factor(route, Vec2::new(50.0, 0.0), nest) - 0.65).abs() < EPS);
        // Beyond the remembered route the factor saturates at 1.
        assert!((distance_factor(route, Vec2::new(400.0, 0.0), nest) - 1.0).abs() < EPS);
        // No route memory keeps the bootstrap trip at full strength.
        assert!((distance_factor(None, Vec2::new(30.0, 0.0), nest) - 1.0).abs() < EPS);
        assert!((distance_factor(Some(Vec2::ZERO), nest, nest) - 1.0).abs() < EPS);
    }

    #[test]
    fn trail_suppression_falls_with_strength() {
        assert!((trail_suppression(0.0) - 1.0).abs() < EPS);
        assert!((trail_suppression(1.0) - 1.0 / 3.0).abs() < EPS);
        assert!(trail_suppression(0.5) > trail_suppression(1.0));
        // Out-of-range strength cannot invert the suppression.
        assert!((trail_suppression(-1.0) - 1.0).abs() < EPS);
        assert!((trail_suppression(5.0) - 1.0 / 3.0).abs() < EPS);
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

    #[test]
    fn nest_proximity_suppression_rises_from_the_nest_and_floors() {
        let nest = Vec2::ZERO;

        // Inside the nest the floor and the density tax apply.
        let inside = nest_proximity_suppression(nest, nest, 1);
        assert!((inside - PHEROMONE_NEST_PROXIMITY_FLOOR).abs() < EPS);

        // On the rim the ramp is half strength (lone ant, no density tax).
        assert!(
            (nest_proximity_suppression(Vec2::new(NEST_RADIUS, 0.0), nest, 1) - 0.5).abs() < EPS
        );

        // Twice the radius and beyond: no suppression.
        assert!(
            (nest_proximity_suppression(Vec2::new(2.0 * NEST_RADIUS, 0.0), nest, 1) - 1.0).abs()
                < EPS
        );
        assert!(
            (nest_proximity_suppression(Vec2::new(10.0 * NEST_RADIUS, 0.0), nest, 1) - 1.0).abs()
                < EPS
        );

        // Crowding inside the nest never silences the bootstrap deposit.
        let crowded = nest_proximity_suppression(nest, nest, 10_000);
        assert!(crowded > 0.0);
        assert!(crowded < inside);
    }

    #[test]
    fn home_range_mark_is_weak_and_nest_suppressed() {
        let nest = Vec2::ZERO;
        let far = Vec2::new(4.0 * NEST_RADIUS, 0.0);

        assert!(
            (home_range_multiplier(far, nest, 1) - PHEROMONE_TO_NEST_BASE).abs() < EPS,
            "outside the nest the mark is just the low base"
        );
        assert!(
            home_range_multiplier(nest, nest, 1) < PHEROMONE_TO_NEST_BASE * 0.2,
            "inside the nest the mark must be strongly suppressed"
        );
        assert!(
            home_range_multiplier(nest, nest, 1) > 0.0,
            "bootstrap floor"
        );
    }

    /// The dense pass must produce the same grid as the naive per-sample path
    /// (within float tolerance) for a mix of laden, experienced and naive ants
    /// that share cells and straddle cell boundaries.
    #[test]
    fn dense_deposit_matches_the_naive_per_sample_grid() {
        use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};
        use bevy::ecs::system::RunSystemOnce;

        let dt = 1.0 / 64.0;
        let nest = Vec2::new(-100.0, 0.0);
        let mut specs = Vec::new();

        for index in 0..24 {
            let pos = Vec2::new(-60.0 + index as f32 * 5.0, 30.0 - index as f32 * 2.5);
            specs.push((
                index as f32 * 0.37,
                20.0 + index as f32,
                pos,
                index % 3 == 0,
                (index % 4) as u32,
                index as f32 * 0.1 + 0.5,
                (index % 2 == 0).then_some(Vec2::new(180.0, 12.0)),
            ));
        }

        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.insert_resource(PheromoneGrid::new());
        world.insert_resource(NestPosition(nest));

        let mut density = AntDensity::new();
        for (index, (direction, speed, pos, laden, trips, quality, route)) in
            specs.iter().enumerate()
        {
            let mut ant = Ant::test_ant(*direction);
            ant.speed = *speed;
            ant.carrying = if *laden { 1.0 } else { 0.0 };
            ant.has_food = *laden;
            ant.trips_completed = *trips;
            ant.carrying_quality = *quality;
            ant.route_memory = *route;
            ant.prev_pos = *pos - Vec2::new(direction.cos(), direction.sin()) * speed * dt;
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

        // Snapshot the inputs the system will read (the pre-deposit grid and
        // the density grid) before running it, so the oracle sees the same
        // followed-trail strengths and occupancy.
        let mut inputs = Vec::with_capacity(specs.len());
        {
            let grid = world.resource::<PheromoneGrid>();
            let density = world.resource::<AntDensity>();

            for (direction, speed, pos, laden, trips, quality, route) in &specs {
                let mut ant = Ant::test_ant(*direction);
                ant.carrying = if *laden { 1.0 } else { 0.0 };
                ant.has_food = *laden;
                ant.trips_completed = *trips;
                ant.carrying_quality = *quality;
                ant.route_memory = *route;
                ant.prev_pos = *pos - Vec2::new(direction.cos(), direction.sin()) * speed * dt;

                let suppression = trail_suppression(ant_followed_strength(&ant, *pos, grid));
                let (to_food, to_nest) = if ant.is_laden() {
                    (
                        laden_deposit_multiplier(&ant, *pos, nest) * suppression,
                        0.0,
                    )
                } else {
                    (
                        0.0,
                        home_range_multiplier(*pos, nest, density.sample(*pos)) * suppression,
                    )
                };

                inputs.push((ant.prev_pos, *pos, to_food, to_nest));
            }
        }

        world.run_system_once(deposit_pheromones).unwrap();

        // Re-run the same arithmetic one sample at a time into a fresh grid.
        let deposited = world.resource::<PheromoneGrid>();
        let mut expected = PheromoneGrid::new();

        for (prev_pos, pos, to_food, to_nest) in inputs {
            let per_sample = PHEROMONE_DEPOSIT_RATE * dt / DEPOSIT_SAMPLES as f32;

            for sample_index in 0..DEPOSIT_SAMPLES {
                let t = (sample_index as f32 + 0.5) / DEPOSIT_SAMPLES as f32;
                let sample_pos = prev_pos.lerp(pos, t);

                if let Some(cell) = world_to_grid(sample_pos) {
                    expected.add_kernel(cell, per_sample * to_food, per_sample * to_nest);
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
                    (got.to_food - want.to_food).abs() < 1e-6
                        && (got.to_nest - want.to_nest).abs() < 1e-6,
                    "cell {cell:?}: got {got:?}, want {want:?}"
                );

                if want.to_food > 0.0 || want.to_nest > 0.0 {
                    populated += 1;
                }
            }
        }

        assert!(populated > 0, "the equivalence test must deposit something");
    }

    /// When every sample shares a cell the dense pass convolves exactly once,
    /// so the grid mass equals the full per-tick deposit.
    #[test]
    fn dense_deposit_preserves_mass_when_samples_share_a_cell() {
        use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};
        use crate::pheromone::grid::PheromoneKind;
        use bevy::ecs::system::RunSystemOnce;

        let dt = 1.0 / 64.0;
        let pos = Vec2::new(10.0, 10.0);
        let mut ant = Ant::test_ant(0.0);
        ant.speed = 0.0;
        ant.carrying = 1.0;
        ant.has_food = true;
        ant.prev_pos = pos;

        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.insert_resource(PheromoneGrid::new());
        world.insert_resource(NestPosition(Vec2::new(-100.0, 0.0)));

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
            .map(|cell| grid.sample(cell, PheromoneKind::ToFood))
            .sum();

        // Average quality, no route memory and no followed trail: the full
        // base multiplier arrives in the grid.
        let expected = PHEROMONE_DEPOSIT_RATE * dt * DEPOSIT_BASE_MULTIPLIER;
        assert!(
            (total - expected).abs() < 1e-5,
            "mass should be {expected}, got {total}"
        );
    }

    /// The F10 suppression must let a busy trail grow while keeping the nest
    /// mouth from pooling: an outbound ant's mark is strongest outside 2R.
    #[test]
    fn outbound_deposits_are_suppressed_only_near_the_nest() {
        let nest = Vec2::new(-100.0, 0.0);
        let near = home_range_multiplier(nest, nest, 1);
        let rim = home_range_multiplier(nest + Vec2::new(NEST_RADIUS, 0.0), nest, 1);
        let outside = home_range_multiplier(nest + Vec2::new(4.0 * NEST_RADIUS, 0.0), nest, 1);

        assert!(near < rim && rim < outside);
        assert!((outside - PHEROMONE_TO_NEST_BASE).abs() < EPS);
    }

    /// Ignored micro-benchmark of the dense deposit path at 30k ants.
    /// Run with:
    /// `cargo test --release --locked --bin ants dense_deposit_micro_bench -- --ignored --nocapture`
    #[test]
    #[ignore = "perf micro-benchmark, run on demand"]
    fn dense_deposit_micro_bench_30k() {
        use crate::constants::ant::ANT_SPEED;
        use std::time::Instant;

        const ANTS: usize = 30_000;
        const TICKS: u32 = 40;

        let dt = 1.0 / 64.0;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        let world = app.world_mut();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.insert_resource(PheromoneGrid::new());
        world.insert_resource(NestPosition(Vec2::new(-100.0, 0.0)));

        let mut density = AntDensity::new();
        let mut rng = fastrand::Rng::with_seed(0xD3_D0_5E_ED);

        for index in 0..ANTS {
            let mut ant = Ant::test_ant(rng.f32() * std::f32::consts::TAU);
            ant.speed = ANT_SPEED;
            ant.carrying = if index % 5 == 0 { 1.0 } else { 0.0 };
            ant.has_food = ant.carrying > 0.0;
            ant.trips_completed = (index % 8) as u32;
            ant.carrying_quality = 0.5 + rng.f32();
            let pos = Vec2::new(rng.f32() * 700.0 - 350.0, rng.f32() * 500.0 - 250.0);
            ant.prev_pos =
                pos - Vec2::new(ant.direction.cos(), ant.direction.sin()) * ANT_SPEED * dt;
            density.add(pos);
            world.spawn((ant, Transform::from_xyz(pos.x, pos.y, 0.0)));
        }

        world.insert_resource(density);
        world
            .resource_mut::<Time<Fixed>>()
            .advance_by(Duration::from_secs_f32(dt));

        let id = world.register_system(deposit_pheromones);

        for _ in 0..3 {
            world.run_system(id).unwrap();
        }

        let start = Instant::now();

        for _ in 0..TICKS {
            world.run_system(id).unwrap();
        }

        let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(TICKS);
        println!(
            "dense deposit micro-bench: {ANTS} ants, {ms:.3} ms/tick, {} bands, {} compute threads",
            deposit_bands(),
            ComputeTaskPool::get().thread_num()
        );
    }
}
