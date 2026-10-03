//! Headless trail-formation tests and the calibration harness they grew from.
//!
//! The simulation runs the real fixed-step plugin chain (clock, nest geometry,
//! lifecycle, movement, collision, deposit, decay) with `MinimalPlugins`.
//! Determinism comes from two places:
//! every ant owns a seeded [`crate::simulation::ant::AntRng`] and the food grid
//! iterates in a stable order, so a run does not depend on executor threads or
//! hash-map randomization.
//!
//! `measure_trails` is an ignored calibration harness; run it with:
//! `cargo test --bin ants measure_trails -- --ignored --nocapture`
//! Env knobs: `TRAIL_SECS`, `TRAIL_CAP`, `TRAIL_FOOD_DX`.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

use crate::constants::world::{
    FOOD_X, GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, NEST_X, NEST_Y, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH,
};
use crate::pheromone::PheromonePlugin;
use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
use crate::simulation::Nest;
use crate::simulation::SimulationPlugin;
use crate::simulation::ant::{Ant, AntPopulation, AntSpawner};
use crate::simulation::colony::ColonyStats;
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
