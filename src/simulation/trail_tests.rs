//! Headless trail-formation tests and the calibration harness they grew from.
//!
//! The simulation runs the real fixed-step plugin chain (clock, nest geometry,
//! lifecycle, movement, collision, deposit, decay) with `MinimalPlugins`.
//! Determinism comes from two places:
//! every ant owns a seeded [`crate::simulation::ant::AntRng`] and the food grid
//! iterates in a stable order, so a run does not depend on executor threads or
//! hash-map randomization.
//!
//! Besides the straight-corridor regression, the file contains the F17
//! route-choice tests: a double bridge where the shorter of two branches
//! around a wall must win the trail competition, and a move-the-food run where
//! the old trail decays and a new one forms. Both use a fixed forager cohort
//! and a seeded equal starting trail so they stay deterministic and fast.
//!
//! `measure_trails`, `measure_double_bridge` and `measure_food_move` are
//! ignored calibration harnesses; run one with:
//! `cargo test --bin ants measure_trails -- --ignored --nocapture`
//! Env knobs: `TRAIL_SECS`, `TRAIL_CAP`, `TRAIL_FOOD_DX`; `BRIDGE_*` and
//! `MOVE_*` for the route-choice harnesses (see their doc comments).

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

use crate::constants::world::{
    FOOD_X, GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, NEST_RADIUS, NEST_X, NEST_Y, PLAY_AREA_HEIGHT,
    PLAY_AREA_WIDTH,
};
use crate::pheromone::PheromonePlugin;
use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
use crate::simulation::Nest;
use crate::simulation::SimulationPlugin;
use crate::simulation::ant::{Ant, AntPopulation, AntRng, AntSpawner};
use crate::simulation::colony::ColonyStats;
use crate::simulation::environment::{Obstacle, Obstacles};
use crate::simulation::food::{self, FoodGrid};

/// Corridor half-height in world units.
const CORRIDOR_HALF: f32 = 24.0;
/// Control band center in world units (parallel to the nest-food line).
const CONTROL_Y: f32 = 160.0;
/// Regression-test food distance east of the nest, in world units.
///
/// 450 px is a demanding round trip (the broken 30 s lifetime / 40 s energy
/// budget could not complete it) while keeping the test fast enough for the
/// regular suite. The game map itself uses the longer [`FOOD_X`] distance.
const TEST_FOOD_DISTANCE: f32 = 450.0;
/// Regression-test population cap.
const TEST_POPULATION_CAP: usize = 800;
/// Seconds the trail test runs before measuring.
const TRAIL_CHECK_SECS: u32 = 45;
/// Seconds of fixed steps compared by [`deterministic_replay_is_bit_identical`].
const REPLAY_SECS: u32 = 15;
/// Corridor `ToFood` average required to call it a trail (full chain measured
/// ~1.27 after [`TRAIL_CHECK_SECS`]).
const MIN_CORRIDOR_TO_FOOD: f32 = 0.05;
/// Deliveries required by [`TRAIL_CHECK_SECS`] (full chain measured ~65).
const MIN_DELIVERIES: f32 = 10.0;

// Route-choice tests (F17). Both use a fixed forager cohort instead of the
// colony spawn ramp so the population — and therefore the test cost — is
// predictable; the route-choice behavior is what is under test, not the
// economy.
/// Foragers in the fixed cohort of the route-choice tests.
const ROUTE_POPULATION: usize = 120;
/// Seconds the double-bridge test runs before measuring.
const BRIDGE_SECS: u32 = 20;
/// Equal `ToFood` intensity per cell seeded on both branches of the fork.
///
/// This is the Deneubourg initial condition: with both routes started at the
/// same trail strength, the route choice is decided by reinforcement (the
/// shorter branch completes more round trips per second), not by which branch
/// the first successful forager happened to return on.
const BRIDGE_SEED: f32 = 1.0;
/// Double-bridge food distance east of the nest.
const BRIDGE_FOOD_DISTANCE: f32 = 350.0;
/// Double-bridge wall: spans `y in [-230, 70]` at `x = -140`, so the short
/// branch rounds its top end (~76 u detour) and the long branch its bottom end
/// (~236 u).
const BRIDGE_WALL: Obstacle = Obstacle::aabb(Vec2::new(-140.0, -80.0), Vec2::new(12.0, 150.0));
/// Move-the-food geometry: the patch starts far east (A) and moves near the
/// nest mouth (B) on the same bearing, so returning foragers still reach the
/// entrance and the old A trail is not refreshed by the new route.
const MOVE_A_DISTANCE: f32 = 250.0;
const MOVE_B_DISTANCE: f32 = 50.0;
/// Seconds the move-the-food test spends building the A trail.
const MOVE_BUILD_SECS: u32 = 10;
/// Seconds the move-the-food test spends on trail turnover after the move.
const MOVE_TURNOVER_SECS: u32 = 15;
/// Seeded `ToFood` intensity of the initial nest->A trail.
const MOVE_SEED: f32 = 1.0;

/// Where [`build_app`] puts the food patch.
pub enum FoodSetup {
    /// The real game patch at [`FOOD_X`], [`crate::constants::world::FOOD_Y`].
    Real,
    /// A 3x3 patch centered on this world position.
    Custom(Vec2),
    /// No food at all (sanity run).
    None,
}

pub fn build_app(food: FoodSetup) -> App {
    let mut app = App::new();

    app.add_plugins(MinimalPlugins)
        .add_plugins(PheromonePlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 64.0,
        )));

    SimulationPlugin::add_fixed_step_systems(&mut app);

    // Nest entity (normally spawned by `spawn_world`, which needs render assets).
    app.world_mut()
        .spawn((Nest, Transform::from_xyz(NEST_X, NEST_Y, 0.0)));

    match food {
        FoodSetup::Real => {
            app.world_mut()
                .run_system_once(food::setup_food_patch)
                .expect("food patch setup");
        }
        FoodSetup::Custom(center) => {
            app.world_mut()
                .run_system_once(move |mut food: ResMut<FoodGrid>, mut commands: Commands| {
                    let origin = crate::core::grid::world_to_grid(center)
                        .expect("food center must be in bounds");
                    for dy in 0..3 {
                        for dx in 0..3 {
                            food.set(
                                &mut commands,
                                origin + UVec2::new(dx, dy),
                                crate::constants::world::INITIAL_FOOD_AMOUNT,
                            );
                        }
                    }
                })
                .expect("food patch setup");
        }
        FoodSetup::None => {}
    }

    app
}

/// Simulate `seconds` of fixed steps, optionally stopping new spawns once the
/// population reaches `population_cap`.
pub fn run_seconds(app: &mut App, seconds: u32, population_cap: Option<usize>) {
    for _ in 0..seconds * 64 {
        app.update();

        if let Some(cap) = population_cap
            && app.world().resource::<AntPopulation>().count() >= cap
        {
            app.world_mut()
                .resource_mut::<AntSpawner>()
                .timer
                .set_duration(Duration::from_secs(3600));
        }
    }
}

/// Average `kind` intensity over the straight nest-food corridor at `y_center`.
pub fn corridor_avg(grid: &PheromoneGrid, kind: PheromoneKind, y_center: f32, x_end: f32) -> f32 {
    let mut sum = 0.0;
    let mut count = 0u32;

    for y in 0..GRID_HEIGHT as u32 {
        let world_y = y as f32 * GRID_SIZE - PLAY_AREA_HEIGHT / 2.0 + GRID_SIZE / 2.0;
        if (world_y - y_center).abs() > CORRIDOR_HALF {
            continue;
        }

        for x in 0..GRID_WIDTH as u32 {
            let world_x = x as f32 * GRID_SIZE - PLAY_AREA_WIDTH / 2.0 + GRID_SIZE / 2.0;
            if world_x < NEST_X || world_x > x_end {
                continue;
            }

            sum += grid.sample(UVec2::new(x, y), kind);
            count += 1;
        }
    }

    if count == 0 { 0.0 } else { sum / count as f32 }
}

pub fn grid_totals(grid: &PheromoneGrid) -> (f32, f32) {
    let mut to_food = 0.0;
    let mut to_nest = 0.0;

    for y in 0..GRID_HEIGHT as u32 {
        for x in 0..GRID_WIDTH as u32 {
            if let Some(cell) = grid.get(UVec2::new(x, y)) {
                to_food += cell.to_food;
                to_nest += cell.to_nest;
            }
        }
    }

    (to_food, to_nest)
}

/// Total `kind` intensity over the world-space rectangle `min..max`
/// (inclusive cell centres).
///
/// Used by the route-choice tests, which compare trail mass on two branches of
/// a fork instead of the straight nest-food corridor.
pub fn region_mass(grid: &PheromoneGrid, kind: PheromoneKind, min: Vec2, max: Vec2) -> f32 {
    let mut sum = 0.0;

    for y in 0..GRID_HEIGHT as u32 {
        let world_y = y as f32 * GRID_SIZE - PLAY_AREA_HEIGHT / 2.0 + GRID_SIZE / 2.0;
        if world_y < min.y || world_y > max.y {
            continue;
        }

        for x in 0..GRID_WIDTH as u32 {
            let world_x = x as f32 * GRID_SIZE - PLAY_AREA_WIDTH / 2.0 + GRID_SIZE / 2.0;
            if world_x < min.x || world_x > max.x {
                continue;
            }

            sum += grid.sample(UVec2::new(x, y), kind);
        }
    }

    sum
}

/// `(laden near corridor, foragers near corridor, total ants)`.
pub fn ants_near_corridor(world: &mut World) -> (usize, usize, usize) {
    let mut laden = 0;
    let mut foragers = 0;
    let mut total = 0;

    let mut query = world.query::<(&Ant, &Transform)>();
    for (ant, transform) in query.iter(world) {
        total += 1;
        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        let in_corridor = (NEST_X..=FOOD_X).contains(&pos.x) && pos.y.abs() <= CORRIDOR_HALF;

        if in_corridor {
            if ant.has_food {
                laden += 1;
            } else {
                foragers += 1;
            }
        }
    }

    (laden, foragers, total)
}

pub fn food_remaining(world: &mut World) -> f32 {
    let food = world.resource::<FoodGrid>();
    food.iter().map(|(_, amount)| amount.max(0.0)).sum()
}

/// One ant's observable state for the determinism replay, keyed by entity
/// index (generational IDs are world-local and must not be compared).
#[derive(PartialEq, Debug)]
struct AntSnapshot {
    index: u32,
    x: f32,
    y: f32,
    direction: f32,
    energy: f32,
    age: f32,
    carrying: f32,
    trips_completed: u32,
    phase: u8,
}

fn ant_snapshots(world: &mut World) -> Vec<AntSnapshot> {
    let mut query = world.query::<(Entity, &Ant, &Transform)>();
    let mut snapshots: Vec<AntSnapshot> = query
        .iter(world)
        .map(|(entity, ant, transform)| AntSnapshot {
            index: entity.index_u32(),
            x: transform.translation.x,
            y: transform.translation.y,
            direction: ant.direction,
            energy: ant.energy,
            age: ant.age,
            carrying: ant.carrying,
            trips_completed: ant.trips_completed,
            phase: ant.phase as u8,
        })
        .collect();

    snapshots.sort_by_key(|snapshot| snapshot.index);
    snapshots
}

/// Emergent trail regression: with the real fixed-step chain, a colony must
/// carry food home and build a `ToFood` corridor toward the food, clearly
/// separated from a symmetric control band.
#[test]
fn emergent_trail_forms_between_nest_and_food() {
    let food_x = NEST_X + TEST_FOOD_DISTANCE;
    let mut app = build_app(FoodSetup::Custom(Vec2::new(food_x, NEST_Y)));
    run_seconds(&mut app, TRAIL_CHECK_SECS, Some(TEST_POPULATION_CAP));

    let deliveries = app.world().resource::<ColonyStats>().total_food_delivered;
    let grid = app.world().resource::<PheromoneGrid>();
    let corridor_food = corridor_avg(grid, PheromoneKind::ToFood, NEST_Y, food_x);
    let control_food = corridor_avg(grid, PheromoneKind::ToFood, CONTROL_Y, food_x);

    assert!(
        deliveries >= MIN_DELIVERIES,
        "expected at least {MIN_DELIVERIES} deliveries after {TRAIL_CHECK_SECS}s, got {deliveries}"
    );
    assert!(
        corridor_food >= MIN_CORRIDOR_TO_FOOD,
        "expected a ToFood corridor (>= {MIN_CORRIDOR_TO_FOOD}), got {corridor_food} \
         (control {control_food})"
    );
    assert!(
        corridor_food >= control_food + MIN_CORRIDOR_TO_FOOD,
        "corridor ToFood {corridor_food} must clearly exceed control {control_food}"
    );
}

/// Sanity run: without food nothing deposits `ToFood`, so no corridor can form
/// even though ants are walking and laying `ToNest`.
#[test]
fn no_food_forms_no_to_food_corridor() {
    let mut app = build_app(FoodSetup::None);
    run_seconds(&mut app, 15, Some(TEST_POPULATION_CAP));

    let deliveries = app.world().resource::<ColonyStats>().total_food_delivered;
    let grid = app.world().resource::<PheromoneGrid>();
    let corridor_food = corridor_avg(grid, PheromoneKind::ToFood, NEST_Y, FOOD_X);
    let control_food = corridor_avg(grid, PheromoneKind::ToFood, CONTROL_Y, FOOD_X);
    let (_, nest_total) = grid_totals(grid);

    assert_eq!(deliveries, 0.0, "no food means no deliveries");
    assert_eq!(corridor_food, 0.0, "no ToFood may appear without food");
    assert_eq!(control_food, 0.0, "control band must stay empty too");
    assert!(
        nest_total > 0.0,
        "ants should still walk and deposit ToNest"
    );
}

/// Determinism contract (README): two apps that execute the same fixed-step
/// sequence must produce bit-identical ant state, food totals, pheromone
/// totals and colony counters. Sorted by entity index because generational
/// IDs are world-local.
#[test]
fn deterministic_replay_is_bit_identical() {
    let food_center = Vec2::new(NEST_X + TEST_FOOD_DISTANCE, NEST_Y);
    let mut first = build_app(FoodSetup::Custom(food_center));
    let mut second = build_app(FoodSetup::Custom(food_center));

    run_seconds(&mut first, REPLAY_SECS, Some(TEST_POPULATION_CAP));
    run_seconds(&mut second, REPLAY_SECS, Some(TEST_POPULATION_CAP));

    let first_ants = ant_snapshots(first.world_mut());
    let second_ants = ant_snapshots(second.world_mut());
    assert!(
        !first_ants.is_empty(),
        "the replay must have ants to compare"
    );
    assert_eq!(
        first_ants, second_ants,
        "ant state diverged between identical runs"
    );

    assert_eq!(
        food_remaining(first.world_mut()),
        food_remaining(second.world_mut()),
        "food totals diverged between identical runs"
    );

    let (first_food, first_nest) = grid_totals(first.world().resource::<PheromoneGrid>());
    let (second_food, second_nest) = grid_totals(second.world().resource::<PheromoneGrid>());
    assert_eq!(
        (first_food, first_nest),
        (second_food, second_nest),
        "pheromone totals diverged between identical runs"
    );

    assert_eq!(
        first.world().resource::<ColonyStats>().total_food_delivered,
        second
            .world()
            .resource::<ColonyStats>()
            .total_food_delivered,
        "delivery totals diverged between identical runs"
    );
    assert_eq!(
        first.world().resource::<AntPopulation>().count(),
        second.world().resource::<AntPopulation>().count(),
        "population diverged between identical runs"
    );
}

/// Branch polylines of the double bridge, from the nest mouth around each end
/// of [`BRIDGE_WALL`] to the food.
fn bridge_branch_paths(food_x: f32) -> ([Vec2; 4], [Vec2; 4]) {
    let start = Vec2::new(NEST_X + NEST_RADIUS, NEST_Y);
    let short = [
        start,
        Vec2::new(-160.0, 76.0),
        Vec2::new(-120.0, 76.0),
        Vec2::new(food_x, NEST_Y),
    ];
    let long = [
        start,
        Vec2::new(-160.0, -236.0),
        Vec2::new(-120.0, -236.0),
        Vec2::new(food_x, NEST_Y),
    ];

    (short, long)
}

/// F17 double bridge: both branches of the fork start at the same `ToFood`
/// strength, so the route choice is decided by reinforcement. The short branch
/// (around the top end of the wall, ~76 u detour) completes more round trips
/// per second than the long branch (~236 u detour), so after [`BRIDGE_SECS`]
/// it must carry more trail and more traffic.
#[test]
fn short_branch_wins_the_double_bridge() {
    let food = Vec2::new(NEST_X + BRIDGE_FOOD_DISTANCE, NEST_Y);
    let mut app = build_app(FoodSetup::Custom(food));
    app.insert_resource(Obstacles::new(vec![BRIDGE_WALL]));

    let (short_path, long_path) = bridge_branch_paths(food.x);
    {
        let mut grid = app.world_mut().resource_mut::<PheromoneGrid>();
        seed_branch_trail(&mut grid, &short_path, BRIDGE_SEED);
        seed_branch_trail(&mut grid, &long_path, BRIDGE_SEED);
    }

    spawn_forager_cohort(&mut app, ROUTE_POPULATION);
    run_seconds(&mut app, BRIDGE_SECS, None);

    let grid = app.world().resource::<PheromoneGrid>();

    // Branch regions east of the wall (x > -128): everything above the nest
    // line can only have come around the short top end, everything below it
    // around the long bottom end.
    let short_mass = region_mass(
        grid,
        PheromoneKind::ToFood,
        Vec2::new(-128.0, 1.0),
        Vec2::new(food.x, 160.0),
    );
    let long_mass = region_mass(
        grid,
        PheromoneKind::ToFood,
        Vec2::new(-128.0, -PLAY_AREA_HEIGHT / 2.0),
        Vec2::new(food.x, -1.0),
    );

    // Equal-size cross-sections through each branch just east of the wall:
    // the traffic on each route, independent of branch length (measured
    // ~38 short vs ~3 long after 20 s).
    let short_cross = region_mass(
        grid,
        PheromoneKind::ToFood,
        Vec2::new(-70.0, 10.0),
        Vec2::new(-50.0, 70.0),
    );
    let long_cross = region_mass(
        grid,
        PheromoneKind::ToFood,
        Vec2::new(-70.0, -150.0),
        Vec2::new(-50.0, -90.0),
    );

    assert!(
        long_cross > 0.0,
        "the long branch must remain a used route, got {long_cross}"
    );
    assert!(
        short_cross > 2.0 * long_cross,
        "the short branch must carry more ToFood traffic: short {short_cross}, long {long_cross}"
    );

    // The long branch is longer, so it can hold more total mass even when it is
    // used less; compare trail per unit length instead (measured ~0.92 short vs
    // ~0.72 long after 20 s).
    let short_len: f32 = short_path.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let long_len: f32 = long_path.windows(2).map(|w| (w[1] - w[0]).length()).sum();

    assert!(
        short_mass / short_len > long_mass / long_len,
        "the short branch must accumulate more ToFood trail per unit length: \
         short {:.3}/u, long {:.3}/u",
        short_mass / short_len,
        long_mass / long_len
    );
}

/// `ToFood` mass in a window around a patch site, used by the move-the-food
/// test (the B window includes the nest mouth, the A window is far from it).
fn patch_trail(app: &App, x: f32) -> f32 {
    region_mass(
        app.world().resource::<PheromoneGrid>(),
        PheromoneKind::ToFood,
        Vec2::new(x - 60.0, -30.0),
        Vec2::new(x + 20.0, 30.0),
    )
}

/// F17 move the food: after a trail has formed to patch A, moving the patch to
/// B must let the A trail decay while a B trail forms, and deliveries must
/// resume from the new source within [`MOVE_TURNOVER_SECS`].
#[test]
fn moving_the_food_moves_the_trail() {
    let a = Vec2::new(NEST_X + MOVE_A_DISTANCE, NEST_Y);
    let b = Vec2::new(NEST_X + MOVE_B_DISTANCE, NEST_Y);
    let mut app = build_app(FoodSetup::Custom(a));
    app.insert_resource(Obstacles::empty());

    {
        let start = Vec2::new(NEST_X + NEST_RADIUS, NEST_Y);
        let mut grid = app.world_mut().resource_mut::<PheromoneGrid>();
        seed_branch_trail(&mut grid, &[start, a], MOVE_SEED);
    }

    spawn_forager_cohort(&mut app, ROUTE_POPULATION);
    run_seconds(&mut app, MOVE_BUILD_SECS, None);

    let a_before = patch_trail(&app, a.x);
    let b_before = patch_trail(&app, b.x);
    let deliveries_before = app.world().resource::<ColonyStats>().total_food_delivered;

    move_food_patch(&mut app, a, b);
    run_seconds(&mut app, MOVE_TURNOVER_SECS, None);

    let a_after = patch_trail(&app, a.x);
    let b_after = patch_trail(&app, b.x);
    let deliveries_after = app.world().resource::<ColonyStats>().total_food_delivered;

    assert!(
        a_before > 0.0,
        "the A trail must have formed before the move, got {a_before}"
    );
    assert!(
        a_after < 0.5 * a_before,
        "the A trail must decay after the move: {a_before} -> {a_after}"
    );
    assert!(
        b_after > 5.0 * b_before,
        "the B trail must form after the move: {b_before} -> {b_after}"
    );
    assert!(
        deliveries_after > deliveries_before,
        "foragers must resume deliveries from B: {deliveries_before} -> {deliveries_after}"
    );
}

/// Ignored measurement harness used to pick the regression thresholds.
#[test]
#[ignore = "measurement harness, run on demand"]
fn measure_trails() {
    let food_dx: Option<f32> = std::env::var("TRAIL_FOOD_DX")
        .ok()
        .and_then(|value| value.parse().ok());
    let food_center = food_dx.map(|dx| Vec2::new(NEST_X + dx, NEST_Y));
    let food_x = food_center.map_or(FOOD_X, |center| center.x);
    let food = match food_center {
        Some(center) => FoodSetup::Custom(center),
        None => FoodSetup::Real,
    };
    let mut app = build_app(food);

    let total_secs: u32 = std::env::var("TRAIL_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(180);
    let cap: Option<usize> = std::env::var("TRAIL_CAP")
        .ok()
        .and_then(|value| value.parse().ok());
    let mut first_delivery_secs: Option<f32> = None;

    for second in 1..=total_secs {
        run_seconds(&mut app, 1, cap);

        let delivered = app.world().resource::<ColonyStats>().total_food_delivered;

        if first_delivery_secs.is_none() && delivered > 0.0 {
            first_delivery_secs = Some(second as f32);
        }

        if second % 5 != 0 {
            continue;
        }

        let (laden, foragers, total) = ants_near_corridor(app.world_mut());
        let population = app.world().resource::<AntPopulation>().count();
        let remaining = food_remaining(app.world_mut());
        let grid = app.world().resource::<PheromoneGrid>();
        let corridor_food = corridor_avg(grid, PheromoneKind::ToFood, NEST_Y, food_x);
        let corridor_nest = corridor_avg(grid, PheromoneKind::ToNest, NEST_Y, food_x);
        let control_food = corridor_avg(grid, PheromoneKind::ToFood, CONTROL_Y, food_x);
        let (food_total, nest_total) = grid_totals(grid);

        println!(
            "t={second:3}s pop={population:5} deliveries={delivered:6.0} food_left={remaining:7.1} \
             corridor_food={corridor_food:7.3} control_food={control_food:7.3} \
             corridor_nest={corridor_nest:7.3} mass_food={food_total:9.1} mass_nest={nest_total:9.1} \
             ants_near={{laden:{laden:4}, out:{foragers:4}}} of {total}"
        );
    }

    println!("first delivery at: {first_delivery_secs:?}s");
}

/// Counts ants in the two branch regions `(short, long)`.
fn branch_ant_counts(world: &mut World, short: (Vec2, Vec2), long: (Vec2, Vec2)) -> (usize, usize) {
    let mut short_count = 0;
    let mut long_count = 0;
    let mut query = world.query::<&Transform>();

    for transform in query.iter(world) {
        let pos = transform.translation.truncate();
        let in_region = |(min, max): (Vec2, Vec2)| {
            pos.x >= min.x && pos.x <= max.x && pos.y >= min.y && pos.y <= max.y
        };

        if in_region(short) {
            short_count += 1;
        }
        if in_region(long) {
            long_count += 1;
        }
    }

    (short_count, long_count)
}

/// Seed `intensity` of `ToFood` on every grid cell along a polyline path.
///
/// Test setup only: it gives both branches of a fork the same starting trail,
/// so the route choice is decided by reinforcement (the shorter branch
/// completes more round trips per second) instead of by which branch the first
/// successful forager happened to return on.
fn seed_branch_trail(grid: &mut PheromoneGrid, path: &[Vec2], intensity: f32) {
    let mut cells = std::collections::HashSet::new();

    for window in path.windows(2) {
        let segment = window[1] - window[0];
        let length = segment.length();
        let steps = (length / (GRID_SIZE * 0.5)).ceil() as usize;

        for step in 0..=steps {
            let t = if steps == 0 {
                0.0
            } else {
                step as f32 / steps as f32
            };

            if let Some(cell) = crate::core::grid::world_to_grid(window[0] + segment * t) {
                cells.insert(cell);
            }
        }
    }

    for cell in cells {
        grid.add(cell, intensity, 0.0);
    }
}

/// Spawn `count` foragers at the nest entrance and stop the colony spawner.
///
/// The route-choice tests use a fixed cohort instead of the store-gated spawn
/// ramp so the population (and therefore the test cost) is predictable. The
/// per-ant draws mirror `ant::spawn_ants`: a seeded stream per ant, a uniform
/// heading and a uniform position over the entrance disc.
fn spawn_forager_cohort(app: &mut App, count: usize) {
    use crate::constants::ant::{ANT_LIFETIME, ANT_SPEED};
    use crate::constants::world::NEST_RADIUS;

    app.world_mut()
        .resource_mut::<AntSpawner>()
        .timer
        .set_duration(Duration::from_secs(3600));

    let nest_pos = Vec2::new(NEST_X, NEST_Y);
    let entrance = nest_pos + Vec2::new(NEST_RADIUS, 0.0);

    for index in 0..count {
        let seed = index as u64;
        let mut rng = AntRng::for_spawn(seed).0;
        let heading = rng.f32() * std::f32::consts::TAU;
        let spawn_angle = rng.f32() * std::f32::consts::TAU;
        let spawn_radius = NEST_RADIUS * rng.f32().sqrt();
        let spawn_pos = entrance + Vec2::new(spawn_angle.cos(), spawn_angle.sin()) * spawn_radius;

        let mut ant = Ant::test_ant(heading);
        ant.home = nest_pos;
        ant.max_lifetime = ANT_LIFETIME;
        ant.base_speed = ANT_SPEED;

        app.world_mut().spawn((
            ant,
            AntRng::for_spawn(seed),
            Transform::from_xyz(spawn_pos.x, spawn_pos.y, 0.0),
        ));
    }
}

/// Ignored calibration harness for the double-bridge route choice. Prints the
/// `ToFood` mass on the short and long branch every 5 s. Env knobs:
/// `BRIDGE_SECS`, `BRIDGE_CAP`, `BRIDGE_FOOD_DX`, `BRIDGE_WALL_CY`,
/// `BRIDGE_WALL_HY`, `BRIDGE_SEED`.
#[test]
#[ignore = "measurement harness, run on demand"]
fn measure_double_bridge() {
    let secs: u32 = std::env::var("BRIDGE_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let cap: Option<usize> = std::env::var("BRIDGE_CAP")
        .ok()
        .and_then(|value| value.parse().ok());
    let food_dx: f32 = std::env::var("BRIDGE_FOOD_DX")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(350.0);
    let wall_cy: f32 = std::env::var("BRIDGE_WALL_CY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(-80.0);
    let wall_hy: f32 = std::env::var("BRIDGE_WALL_HY")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(150.0);
    let seed: f32 = std::env::var("BRIDGE_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0);
    let fixed: usize = std::env::var("BRIDGE_FIXED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);

    let wall = Obstacle::aabb(Vec2::new(-140.0, wall_cy), Vec2::new(12.0, wall_hy));
    let food_x = NEST_X + food_dx;
    let mut app = build_app(FoodSetup::Custom(Vec2::new(food_x, NEST_Y)));
    app.insert_resource(Obstacles::new(vec![wall]));

    if seed > 0.0 {
        // Short = around the top end of the wall, long = around the bottom
        // end, matching the default `BRIDGE_WALL` (cy = -80).
        let short_y = wall_cy + wall_hy + 6.0;
        let long_y = wall_cy - wall_hy - 6.0;
        let start = Vec2::new(NEST_X + crate::constants::world::NEST_RADIUS, NEST_Y);
        let short_path = [
            start,
            Vec2::new(-160.0, short_y),
            Vec2::new(-120.0, short_y),
            Vec2::new(food_x, NEST_Y),
        ];
        let long_path = [
            start,
            Vec2::new(-160.0, long_y),
            Vec2::new(-120.0, long_y),
            Vec2::new(food_x, NEST_Y),
        ];

        let mut grid = app.world_mut().resource_mut::<PheromoneGrid>();
        seed_branch_trail(&mut grid, &short_path, seed);
        seed_branch_trail(&mut grid, &long_path, seed);
    }

    if fixed > 0 {
        spawn_forager_cohort(&mut app, fixed);
    }

    // East of the wall: everything above the nest line can only have come
    // around the short (top) end, everything below it around the long (bottom)
    // end.
    let short_min = Vec2::new(-128.0, 1.0);
    let short_max = Vec2::new(food_x, PLAY_AREA_HEIGHT / 2.0);
    let long_min = Vec2::new(-128.0, -PLAY_AREA_HEIGHT / 2.0);
    let long_max = Vec2::new(food_x, -1.0);
    let short_region = (short_min, short_max);
    let long_region = (long_min, long_max);

    let mut first_delivery_secs: Option<f32> = None;

    for second in 1..=secs {
        run_seconds(&mut app, 1, cap);

        let deliveries = app.world().resource::<ColonyStats>().total_food_delivered;

        if first_delivery_secs.is_none() && deliveries > 0.0 {
            first_delivery_secs = Some(second as f32);
        }

        if second % 5 != 0 {
            continue;
        }

        let grid = app.world().resource::<PheromoneGrid>();
        let short = region_mass(grid, PheromoneKind::ToFood, short_min, short_max);
        let long = region_mass(grid, PheromoneKind::ToFood, long_min, long_max);
        // Equal-size cross-sections that both branches fully cross (short at
        // y ~ 38 and long at y ~ -118 at x = -60), so the seed contributes the
        // same starting mass to both and the ratio measures traffic.
        let short_cross = region_mass(
            grid,
            PheromoneKind::ToFood,
            Vec2::new(-70.0, 10.0),
            Vec2::new(-50.0, 70.0),
        );
        let long_cross = region_mass(
            grid,
            PheromoneKind::ToFood,
            Vec2::new(-70.0, -150.0),
            Vec2::new(-50.0, -90.0),
        );
        let short_nest = region_mass(grid, PheromoneKind::ToNest, short_min, short_max);
        let long_nest = region_mass(grid, PheromoneKind::ToNest, long_min, long_max);
        let (food_total, nest_total) = grid_totals(grid);
        let population = app.world().resource::<AntPopulation>().count();
        let (short_ants, long_ants) = branch_ant_counts(app.world_mut(), short_region, long_region);

        println!(
            "t={second:3}s pop={population:5} deliveries={deliveries:6.0} \
             short={short:9.2} long={long:9.2} ratio={:6.2} \
             cross={short_cross:8.2}/{long_cross:8.2} cross_ratio={:6.2} \
             short_nest={short_nest:8.1} long_nest={long_nest:8.1} \
             ants={short_ants:4}/{long_ants:4} mass_food={food_total:9.1} mass_nest={nest_total:9.1}",
            short / long.max(1e-6),
            short_cross / long_cross.max(1e-6),
        );
    }

    println!("first delivery at: {first_delivery_secs:?}s");
}

/// Move a 3x3 food patch from `from` to `to` (both patch centres).
///
/// Used by the move-the-food test: the old patch is removed and an identical
/// one is placed at the new site in the same tick, so foragers must discover
/// the new source and the old trail has to decay on its own.
fn move_food_patch(app: &mut App, from: Vec2, to: Vec2) {
    let from_origin = crate::core::grid::world_to_grid(from).expect("from patch in bounds");
    let to_origin = crate::core::grid::world_to_grid(to).expect("to patch in bounds");

    app.world_mut()
        .run_system_once(move |mut food: ResMut<FoodGrid>, mut commands: Commands| {
            for dy in 0..3 {
                for dx in 0..3 {
                    let offset = UVec2::new(dx, dy);
                    food.remove(&mut commands, from_origin + offset);
                    food.set(
                        &mut commands,
                        to_origin + offset,
                        crate::constants::world::INITIAL_FOOD_AMOUNT,
                    );
                }
            }
        })
        .expect("food move");
}

/// Ignored calibration harness for the move-the-food trail turnover. Env knobs:
/// `MOVE_A_SECS`, `MOVE_B_SECS`, `MOVE_FIXED`, `MOVE_SEED`, `MOVE_A_DX`,
/// `MOVE_B_DX`.
#[test]
#[ignore = "measurement harness, run on demand"]
fn measure_food_move() {
    let a_secs: u32 = std::env::var("MOVE_A_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(12);
    let b_secs: u32 = std::env::var("MOVE_B_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(25);
    let fixed: usize = std::env::var("MOVE_FIXED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(120);
    let seed: f32 = std::env::var("MOVE_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0);
    let a_dx: f32 = std::env::var("MOVE_A_DX")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(250.0);
    let b_dx: f32 = std::env::var("MOVE_B_DX")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50.0);

    let a = Vec2::new(NEST_X + a_dx, NEST_Y);
    let b = Vec2::new(NEST_X + b_dx, NEST_Y);

    let mut app = build_app(FoodSetup::Custom(a));
    app.insert_resource(Obstacles::empty());

    if seed > 0.0 {
        let start = Vec2::new(NEST_X + crate::constants::world::NEST_RADIUS, NEST_Y);
        let path = [start, a];
        let mut grid = app.world_mut().resource_mut::<PheromoneGrid>();
        seed_branch_trail(&mut grid, &path, seed);
    }

    if fixed > 0 {
        spawn_forager_cohort(&mut app, fixed);
    }

    // A trail mass in a window around each patch site (the B window includes
    // the nest mouth, the A window is far from it).
    let near = |grid: &PheromoneGrid, x: f32| {
        region_mass(
            grid,
            PheromoneKind::ToFood,
            Vec2::new(x - 60.0, -30.0),
            Vec2::new(x + 20.0, 30.0),
        )
    };

    println!("--- trail to A for {a_secs}s ---");
    for second in 1..=a_secs {
        run_seconds(&mut app, 1, None);
        let grid = app.world().resource::<PheromoneGrid>();
        let deliveries = app.world().resource::<ColonyStats>().total_food_delivered;
        println!(
            "t={second:3}s A_near={:9.2} B_near={:9.2} deliveries={deliveries}",
            near(grid, a.x),
            near(grid, b.x)
        );
    }

    move_food_patch(&mut app, a, b);
    println!("--- food moved from {a:?} to {b:?} ---");

    let a_before = near(app.world().resource::<PheromoneGrid>(), a.x);
    let b_before = near(app.world().resource::<PheromoneGrid>(), b.x);
    let deliveries_before = app.world().resource::<ColonyStats>().total_food_delivered;

    for second in 1..=b_secs {
        run_seconds(&mut app, 1, None);
        let grid = app.world().resource::<PheromoneGrid>();
        let deliveries = app.world().resource::<ColonyStats>().total_food_delivered;
        println!(
            "t=+{second:3}s A_near={:9.2} ({:5.2}x) B_near={:9.2} ({:5.2}x) deliveries={deliveries}",
            near(grid, a.x),
            near(grid, a.x) / a_before.max(1e-6),
            near(grid, b.x),
            near(grid, b.x) / b_before.max(1e-6),
        );
    }

    println!(
        "A before={a_before:.3}, B before={b_before:.3}, deliveries before={deliveries_before}, after={}",
        app.world().resource::<ColonyStats>().total_food_delivered
    );
}
