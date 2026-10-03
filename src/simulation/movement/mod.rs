//! Per-ant steering and movement, including crowd avoidance, contact
//! separation and wall bounces.

pub mod sensors;
pub mod steering;
pub mod wall;

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::constants::ant::{
    ANT_TURN_RATE, FOOD_PICKUP_RADIUS, FOOD_SENSE_HALF_ANGLE, FOOD_SENSE_RANGE, LOAD_SLOWDOWN,
    NURSING_SPEED_FACTOR, ROUTE_MEMORY_TRAIL_MAX, ROUTE_MEMORY_USE,
};
use crate::constants::world::{
    DENSITY_AHEAD_DISTANCE, DENSITY_AVOIDANCE_TURN_FACTOR, DENSITY_SIDE_DISTANCE, GRID_SIZE,
    NEST_RADIUS,
};
use crate::core::grid::{grid_to_world, world_to_grid};
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::ant::{Ant, AntPhase, AntRng};
use crate::simulation::density::{AntDensity, crowd_response};
use crate::simulation::environment::SimClock;
use crate::simulation::food::FoodGrid;
use sensors::read_sensors;
use steering::{
    accumulate_pi_drift, apply_sensor_noise, apply_steering, remember_food, steer_homeward,
    steer_nursing, steer_towards_food, steer_towards_route_memory, trail_strength,
};
use wall::handle_wall_collision;

/// The dot-product cone test in [`in_cone`] is exact only for a `PI/2`
/// half-angle, where `cos(half_angle) == 0`.
const _: () = assert!(FOOD_SENSE_HALF_ANGLE == FRAC_PI_2);

/// Read-only per-tick inputs shared by every ant's step.
struct StepContext<'a> {
    pheromone_grid: &'a PheromoneGrid,
    food_grid: &'a FoodGrid,
    density: &'a AntDensity,
    /// Foraging-activity multiplier from [`SimClock`], in `[0, 1]`.
    activity: f32,
    delta: f32,
}

/// Advance every ant: sense, steer, avoid crowding, move, then separate from
/// physical contacts and bounce off walls.
///
/// Pheromone deposit is a separate serial pass in
/// [`crate::simulation::deposit`]. Every ant must carry an [`AntRng`]; the
/// per-ant stream is what makes the parallel pass deterministic.
///
/// Contact separation runs as a second phase on the post-move positions, so
/// the correction resolves real overlaps instead of the previous tick's
/// geometry (which would let fast head-on ants cross).
pub fn move_ants(
    mut ant_query: Query<(Entity, &mut Ant, &mut Transform, &mut AntRng)>,
    mut density: ResMut<AntDensity>,
    time: Res<Time<Fixed>>,
    pheromone_grid: Res<PheromoneGrid>,
    food_grid: Res<FoodGrid>,
    clock: Res<SimClock>,
) {
    let delta = time.delta_secs();
    let activity = clock.activity.clamp(0.0, 1.0);

    // Phase 1: steering and locomotion.
    {
        let context = StepContext {
            pheromone_grid: &pheromone_grid,
            food_grid: &food_grid,
            density: &density,
            activity,
            delta,
        };

        ant_query
            .par_iter_mut()
            .for_each(|(_, mut ant, mut transform, mut ant_rng)| {
                step_ant(&mut ant, &mut transform, &mut ant_rng.0, &context);
            });
    }

    // Phase 2: rebuild the fine contact grid from the post-move positions and
    // apply the separation response.
    density.clear_contacts();

    for (entity, ant, transform, _) in &ant_query {
        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        density.add_contact(entity.index_u32(), pos, ant.direction, ant.is_laden());
    }

    let contact = &*density;

    ant_query
        .par_iter_mut()
        .for_each(|(entity, mut ant, mut transform, _)| {
            apply_contact(entity.index_u32(), &mut ant, &mut transform, contact, delta);
        });
}

/// One ant's fixed step: phase steering, crowd response and movement.
///
/// Physical contact separation runs after all ants have moved (phase 2 of
/// [`move_ants`]), so this step only needs the coarse density grid.
fn step_ant(
    ant: &mut Ant,
    transform: &mut Transform,
    rng: &mut fastrand::Rng,
    context: &StepContext,
) {
    let delta = context.delta;
    let current_pos = Vec2::new(transform.translation.x, transform.translation.y);

    // A newly laden ant is still standing on the food it just picked up (the
    // pickup starts a handling pause), so this is the pickup site.
    if ant.is_laden() && ant.route_memory.is_none() {
        ant.route_memory = Some(current_pos - ant.home);
    }

    // The nest resets path integration. The biology stream also clears the
    // drift on a nest visit; doing it here keeps homing sane until then.
    if current_pos.distance_squared(ant.home) <= NEST_RADIUS * NEST_RADIUS {
        ant.pi_drift = 0.0;
    }

    if ant.is_handling() || ant.rest_timer > 0.0 {
        ant.speed = 0.0;
        return;
    }

    let mut target_speed = ant.base_speed * context.activity;

    match ant.phase {
        AntPhase::Nursing => {
            target_speed *= NURSING_SPEED_FACTOR;
            steer_nursing(ant, current_pos, delta, rng);
        }
        AntPhase::Returning => {
            if ant.is_laden() {
                target_speed *= load_speed_factor(ant);
            }
            let mut readings = read_sensors(ant, current_pos, context.pheromone_grid);
            apply_sensor_noise(&mut readings, ant, rng);
            steer_homeward(ant, current_pos, &readings, delta, rng);
        }
        AntPhase::Foraging if ant.is_laden() => {
            target_speed *= load_speed_factor(ant);
            let mut readings = read_sensors(ant, current_pos, context.pheromone_grid);
            apply_sensor_noise(&mut readings, ant, rng);
            steer_homeward(ant, current_pos, &readings, delta, rng);
        }
        AntPhase::Foraging => {
            if let Some(food_pos) = sense_food(ant, current_pos, context.food_grid) {
                remember_food(ant, food_pos);
                steer_towards_food(ant, food_pos, current_pos, delta, rng);
            } else {
                let mut readings = read_sensors(ant, current_pos, context.pheromone_grid);
                apply_sensor_noise(&mut readings, ant, rng);

                let weak_trail = trail_strength(&readings) < ROUTE_MEMORY_TRAIL_MAX;
                let used_memory = weak_trail
                    && ant.route_memory.is_some()
                    && rng.f32() < ROUTE_MEMORY_USE
                    && steer_towards_route_memory(ant, current_pos, context.food_grid, delta, rng);

                if used_memory {
                    ant.lost_time = 0.0;
                } else {
                    apply_steering(ant, current_pos, &readings, delta, context.activity, rng);
                }
            }
        }
    }

    // Nurses wander inside the crowded nest without reacting to it.
    if ant.phase != AntPhase::Nursing {
        let (speed_factor, turn_sign) = sample_crowding(ant, current_pos, context.density);

        if turn_sign != 0.0 {
            ant.direction = (ant.direction
                + turn_sign * ANT_TURN_RATE * delta * DENSITY_AVOIDANCE_TURN_FACTOR)
                .rem_euclid(TAU);
        }

        target_speed *= speed_factor;
    }

    ant.speed = target_speed;
    accumulate_pi_drift(ant, target_speed * delta, rng);

    let (sin, cos) = ant.direction.sin_cos();
    transform.translation.x += cos * target_speed * delta;
    transform.translation.y += sin * target_speed * delta;

    handle_wall_collision(ant, transform);
}

/// Phase-2 contact separation: push out of an overlapping neighbour and turn
/// away (keep-right on head-on encounters). Positions here are post-move, so
/// the split overlap correction restores the contact distance exactly.
fn apply_contact(
    entity_index: u32,
    ant: &mut Ant,
    transform: &mut Transform,
    density: &AntDensity,
    delta: f32,
) {
    let pos = Vec2::new(transform.translation.x, transform.translation.y);

    let Some((correction, turn)) =
        density.contact_response(entity_index, pos, ant.direction, ant.is_laden())
    else {
        return;
    };

    ant.direction = (ant.direction + turn * ANT_TURN_RATE * delta).rem_euclid(TAU);
    transform.translation.x += correction.x;
    transform.translation.y += correction.y;

    handle_wall_collision(ant, transform);
}

/// Graded load–speed law: `1 - LOAD_SLOWDOWN * (carrying / crop_capacity)`.
///
/// A full load at the median crop capacity reproduces
/// [`crate::constants::ant::CARRY_SPEED_FACTOR`].
fn load_speed_factor(ant: &Ant) -> f32 {
    let capacity = ant.crop_capacity.max(f32::EPSILON);
    let load = (ant.carrying / capacity).clamp(0.0, 1.0);
    1.0 - LOAD_SLOWDOWN * load
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
/// The query is bounded to cells within [`FOOD_SENSE_RANGE`] of the ant (plus
/// one cell of slack for the ant's offset from its cell centre); there is no
/// global fallback scan. The nearest in-cone cell is selected directly, which
/// is equivalent to the old "nearest overall, then nearest in-cone" two-step
/// because a cell that is nearest overall and in cone is also nearest among
/// the in-cone cells.
fn sense_food(ant: &Ant, ant_pos: Vec2, food_grid: &FoodGrid) -> Option<Vec2> {
    let center = world_to_grid(ant_pos)?;
    // +1 cell covers the ant's offset from its cell centre.
    let radius_cells = (FOOD_SENSE_RANGE / GRID_SIZE).ceil() as i32 + 1;
    let (sin, cos) = ant.direction.sin_cos();
    let direction = Vec2::new(cos, sin);
    let (cell, _) = food_grid.nearest_within_matching(center, radius_cells, |cell| {
        in_cone(ant_pos, direction, cell, food_grid).is_some()
    })?;

    in_cone(ant_pos, direction, cell, food_grid).map(|(food_pos, _)| food_pos)
}

/// Filter one candidate cell through the pickup amount, range and forward
/// cone; returns its position and distance when it passes.
///
/// The cone test is the dot-product form of `|angle| <= PI/2` (exact for the
/// current half-angle): `direction` is computed once per query, so no `atan2`
/// or angle wrap runs per candidate cell.
fn in_cone(
    ant_pos: Vec2,
    direction: Vec2,
    cell: UVec2,
    food_grid: &FoodGrid,
) -> Option<(Vec2, f32)> {
    if !food_grid.amount(cell).is_some_and(|amount| amount > 0.0) {
        return None;
    }

    let food_pos = grid_to_world(cell);
    let offset = food_pos - ant_pos;
    let distance = offset.length();

    if distance > FOOD_SENSE_RANGE || distance <= f32::EPSILON {
        return None;
    }

    if offset.dot(direction) < 0.0 {
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

    let pickup_squared = FOOD_PICKUP_RADIUS * FOOD_PICKUP_RADIUS;
    let close_enough = ant_pos.distance_squared(grid_to_world(cell)) <= pickup_squared;
    let has_food = food_grid.amount(cell).is_some_and(|amount| amount > 0.0);

    (close_enough && has_food).then_some(cell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ant::{ANT_SPEED, CARRY_SPEED_FACTOR, CONTACT_RADIUS};
    use crate::constants::world::INITIAL_FOOD_AMOUNT;
    use crate::simulation::density::rebuild_ant_density;
    use bevy::time::TimeUpdateStrategy;
    use std::f32::consts::PI;
    use std::time::Duration;

    const EPS: f32 = 1e-4;

    fn food_grid_with(cells: impl IntoIterator<Item = UVec2>) -> FoodGrid {
        let mut grid = FoodGrid::default();
        let mut world = World::new();
        let mut commands = world.commands();
        for cell in cells {
            grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
        }
        grid
    }

    /// Minimal headless app that runs the real density rebuild and movement
    /// systems, without the pheromone plugin.
    fn movement_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Time::<Fixed>::from_hz(64.0))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / 64.0,
            )))
            .init_resource::<PheromoneGrid>()
            .init_resource::<FoodGrid>()
            .init_resource::<AntDensity>()
            .init_resource::<SimClock>()
            .add_systems(FixedUpdate, (rebuild_ant_density, move_ants).chain());
        app
    }

    fn spawn_ant(app: &mut App, ant: Ant, spawn: u64, pos: Vec2) -> Entity {
        app.world_mut()
            .spawn((
                ant,
                AntRng::for_spawn(spawn),
                Transform::from_xyz(pos.x, pos.y, 0.0),
            ))
            .id()
    }

    /// The first update primes the fixed clock; the second runs one step.
    fn run_one_step(app: &mut App) {
        app.update();
        app.update();
    }

    fn ant_speed(app: &App, entity: Entity) -> f32 {
        app.world().get::<Ant>(entity).expect("ant").speed
    }

    fn ant_pos(app: &App, entity: Entity) -> Vec2 {
        app.world()
            .get::<Transform>(entity)
            .expect("transform")
            .translation
            .truncate()
    }

    #[test]
    fn food_is_only_sensed_in_the_forward_cone() {
        let ant = Ant::test_ant(0.0);
        let ahead = world_to_grid(Vec2::new(6.0, 0.0)).expect("in bounds");
        let grid = food_grid_with([ahead]);
        assert!(sense_food(&ant, Vec2::ZERO, &grid).is_some());

        let behind = world_to_grid(Vec2::new(-6.0, 0.0)).expect("in bounds");
        let grid = food_grid_with([behind]);
        assert!(sense_food(&ant, Vec2::ZERO, &grid).is_none());
    }

    #[test]
    fn food_is_only_sensed_within_range() {
        let ant = Ant::test_ant(0.0);
        let far = world_to_grid(Vec2::new(FOOD_SENSE_RANGE + GRID_SIZE, 0.0)).expect("in bounds");
        let grid = food_grid_with([far]);
        assert!(sense_food(&ant, Vec2::ZERO, &grid).is_none());
    }

    #[test]
    fn sense_food_finds_in_cone_food_behind_a_closer_out_of_cone_cell() {
        let ant = Ant::test_ant(0.0);
        let behind = world_to_grid(Vec2::new(-4.0, 0.0)).expect("in bounds");
        let ahead = world_to_grid(Vec2::new(8.0, 0.0)).expect("in bounds");
        let grid = food_grid_with([behind, ahead]);

        assert_eq!(
            sense_food(&ant, Vec2::ZERO, &grid),
            Some(grid_to_world(ahead)),
            "the nearer cell is outside the cone, so the in-cone cell must win"
        );
    }

    #[test]
    fn sense_food_prefers_the_nearest_in_cone_cell() {
        let ant = Ant::test_ant(0.0);
        let near = world_to_grid(Vec2::new(8.0, 0.0)).expect("in bounds");
        let far = world_to_grid(Vec2::new(12.0, 0.0)).expect("in bounds");
        let grid = food_grid_with([far, near]);

        assert_eq!(
            sense_food(&ant, Vec2::ZERO, &grid),
            Some(grid_to_world(near))
        );
    }

    #[test]
    fn pickup_needs_contact_and_remaining_food() {
        let contact = world_to_grid(Vec2::new(2.0, 0.0)).expect("in bounds");
        let grid = food_grid_with([contact]);
        assert_eq!(contact_food(Vec2::ZERO, &grid), Some(contact));

        let far = world_to_grid(Vec2::new(20.0, 0.0)).expect("in bounds");
        let grid = food_grid_with([far]);
        assert_eq!(contact_food(Vec2::ZERO, &grid), None);
    }

    #[test]
    fn load_speed_is_graded_and_resting_ants_do_not_move() {
        let mut app = movement_app();
        let specs = [
            (0.0_f32, 0.0_f32), // (carrying, rest_timer)
            (0.5, 0.0),
            (1.0, 0.0),
            (1.0, 0.5), // resting after a delivery
        ];
        let entities: Vec<Entity> = specs
            .iter()
            .enumerate()
            .map(|(index, &(carrying, rest_timer))| {
                let mut ant = Ant::test_ant(0.0);
                ant.carrying = carrying;
                ant.has_food = carrying > 0.0;
                ant.rest_timer = rest_timer;
                // Far apart so crowding and contact cannot interfere.
                let pos = Vec2::new(-300.0 + index as f32 * 100.0, 0.0);
                spawn_ant(&mut app, ant, index as u64, pos)
            })
            .collect();

        run_one_step(&mut app);

        assert!(
            (ant_speed(&app, entities[0]) - ANT_SPEED).abs() < EPS,
            "an unladen ant moves at full speed, got {}",
            ant_speed(&app, entities[0])
        );
        assert!(
            (ant_speed(&app, entities[1]) - ANT_SPEED * (1.0 - LOAD_SLOWDOWN * 0.5)).abs() < EPS,
            "half a crop should halve the slowdown, got {}",
            ant_speed(&app, entities[1])
        );
        assert!(
            (ant_speed(&app, entities[2]) - ANT_SPEED * CARRY_SPEED_FACTOR).abs() < EPS,
            "a full crop reproduces CARRY_SPEED_FACTOR, got {}",
            ant_speed(&app, entities[2])
        );
        assert_eq!(
            ant_speed(&app, entities[3]),
            0.0,
            "resting ants stand still"
        );
        assert_eq!(
            ant_pos(&app, entities[3]),
            Vec2::new(0.0, 0.0),
            "a resting ant must not move"
        );
    }

    #[test]
    fn activity_scales_movement_speed() {
        let mut app = movement_app();
        app.world_mut().resource_mut::<SimClock>().activity = 0.5;

        let mut ant = Ant::test_ant(0.0);
        ant.phase = AntPhase::Nursing;
        let entity = spawn_ant(&mut app, ant, 0, Vec2::new(-300.0, 0.0));

        run_one_step(&mut app);

        let expected = ANT_SPEED * NURSING_SPEED_FACTOR * 0.5;
        assert!(
            (ant_speed(&app, entity) - expected).abs() < EPS,
            "activity 0.5 should halve the nursing speed, got {}",
            ant_speed(&app, entity)
        );
    }

    /// One headless race: for each lateral offset, a memory ant and a
    /// no-memory ant start symmetrically around the food axis, so both are
    /// equidistant from the patch at `(150, 0)`. Returns `(with_memory_ticks,
    /// without_memory_ticks)` summed over the pairs; a pair that never senses
    /// the food contributes the cap.
    #[test]
    fn route_memory_shortens_rediscovery() {
        const PAIR_OFFSETS: [f32; 4] = [30.0, 90.0, 150.0, 210.0];
        const CAP: usize = 64 * 20;

        let food_pos = Vec2::new(150.0, 0.0);
        let mut app = movement_app();
        *app.world_mut().resource_mut::<FoodGrid>() =
            food_grid_with([world_to_grid(food_pos).expect("food in bounds")]);

        let mut racers = Vec::new();

        for (index, &offset) in PAIR_OFFSETS.iter().enumerate() {
            let mut with_memory = Ant::test_ant(PI);
            with_memory.route_memory = Some(food_pos);
            let with_memory = spawn_ant(
                &mut app,
                with_memory,
                index as u64 * 2,
                Vec2::new(0.0, offset),
            );

            let without_memory = Ant::test_ant(PI);
            let without_memory = spawn_ant(
                &mut app,
                without_memory,
                index as u64 * 2 + 1,
                Vec2::new(0.0, -offset),
            );

            racers.push((with_memory, without_memory));
        }

        let mut first_sense = vec![[None::<usize>; 2]; racers.len()];

        for tick in 1..=CAP {
            app.update();

            for (pair, senses) in racers.iter().zip(first_sense.iter_mut()) {
                for (slot, entity) in [pair.0, pair.1].into_iter().enumerate() {
                    if senses[slot].is_some() {
                        continue;
                    }

                    let ant = app.world().get::<Ant>(entity).expect("ant");
                    let pos = ant_pos(&app, entity);
                    let food_grid = app.world().resource::<FoodGrid>();

                    if sense_food(ant, pos, food_grid).is_some() {
                        senses[slot] = Some(tick);
                    }
                }
            }
        }

        let with_memory: usize = first_sense.iter().map(|s| s[0].unwrap_or(CAP)).sum();
        let without_memory: usize = first_sense.iter().map(|s| s[1].unwrap_or(CAP)).sum();

        assert!(
            with_memory < CAP,
            "a remembered route must reach the food within the window"
        );
        assert!(
            with_memory < without_memory,
            "route memory must speed up re-discovery: {with_memory} ticks with, \
             {without_memory} without"
        );
    }

    #[test]
    fn overlapping_ants_are_pushed_apart_by_contact() {
        let mut app = movement_app();
        let mut first = Ant::test_ant(0.0);
        first.base_speed = 0.0;
        first.phase = AntPhase::Nursing;
        let mut second = Ant::test_ant(0.0);
        second.base_speed = 0.0;
        second.phase = AntPhase::Nursing;

        let a = spawn_ant(&mut app, first, 0, Vec2::new(0.0, 0.0));
        let b = spawn_ant(&mut app, second, 1, Vec2::new(0.0, 0.5));

        for _ in 0..10 {
            run_one_step(&mut app);
        }

        let distance = ant_pos(&app, a).distance(ant_pos(&app, b));
        assert!(
            distance >= CONTACT_RADIUS * 0.9,
            "contact separation must push overlapping ants apart, distance {distance}"
        );
    }
}
