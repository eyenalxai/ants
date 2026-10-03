//! Colony lifecycle: mortality, corpses, necrophoresis and flexible task
//! allocation.
//!
//! Every system here is registered through [`register`] into
//! [`SimSet::Lifecycle`], after the age/energy pass in
//! [`crate::simulation::ant::update_ant_energy_age`]. Random draws come from
//! the ant's own [`AntRng`], so the module stays deterministic.

use bevy::prelude::*;

use crate::constants::ant::ANT_SIZE;
use crate::constants::colony::{
    FORAGER_REVERSION_RATE, RECRUIT_THRESHOLD, REVERSION_NURSE_RATIO, REVERSION_STIMULUS,
    TARGET_FORAGER_FRACTION,
};
use crate::constants::lifecycle::{
    CORPSE_CARRY_SPEED, CORPSE_DROP_RADIUS, CORPSE_PICKUP_RADIUS, CORPSE_TTL, CORPSE_Z,
    MORTALITY_HAZARD,
};
use crate::constants::world::{
    DENSITY_CELL_SIZE, DENSITY_GRID_HEIGHT, DENSITY_GRID_WIDTH, NEST_RADIUS, PLAY_AREA_HEIGHT,
    PLAY_AREA_WIDTH,
};
use crate::core::grid::world_to_index;
use crate::core::sets::SimSet;
use crate::simulation::NestPosition;
use crate::simulation::ant::{Ant, AntPhase, AntPopulation, AntRng};
use crate::simulation::colony::{ColonyStats, NestStore};
use crate::simulation::nest::NestGeometry;

/// A dead ant. It decays after [`CORPSE_TTL`] seconds, or is carried to the
/// refuse pile by a forager first (necrophoresis).
#[derive(Component)]
pub struct Corpse {
    /// Seconds left before the corpse decays and despawns.
    pub ttl: f32,
}

/// Spawn a corpse at `pos`. Shared by every death path (hazard, starvation and
/// old age) and by the refuse dropoff.
pub(crate) fn spawn_corpse(commands: &mut Commands, pos: Vec2) {
    commands.spawn((
        Corpse { ttl: CORPSE_TTL },
        Sprite {
            color: Color::srgba(0.35, 0.28, 0.2, 0.9),
            custom_size: Some(Vec2::splat(ANT_SIZE)),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, CORPSE_Z),
    ));
}

/// Task-allocation stimulus (F7):
/// `S = (1 - store/RECRUIT_THRESHOLD).max(0) + forager_shortage`, where
/// `forager_shortage = clamp(1 - foragers / (TARGET_FORAGER_FRACTION *
/// population), 0, 1)`.
///
/// `S` is 0 when the store is full and the target forager share is met, and
/// rises to 2 when the store is empty and no forager is out.
pub fn forage_stimulus(store_food: f32, population: usize, foragers: usize) -> f32 {
    let satiation = (1.0 - store_food / RECRUIT_THRESHOLD).max(0.0);
    let target = TARGET_FORAGER_FRACTION * population as f32;
    let shortage = if target <= 0.0 {
        0.0
    } else {
        (1.0 - foragers as f32 / target).clamp(0.0, 1.0)
    };

    satiation + shortage
}

/// Whether the colony is satiated enough and short enough on nurses to push
/// foragers back to nursing.
pub fn reversion_allowed(stimulus: f32, nurse_ratio: f32) -> bool {
    stimulus < REVERSION_STIMULUS && nurse_ratio < REVERSION_NURSE_RATIO
}

/// Response-threshold task allocation.
///
/// Nurses mature when the stimulus exceeds their individual
/// [`Ant::forage_threshold`], even before the age threshold (which is handled
/// by the age pass). When the colony is satiated and short on nurses, a small
/// per-second fraction of empty foragers reverts to nursing.
pub fn allocate_tasks(
    mut ant_query: Query<(&mut Ant, Option<&mut AntRng>)>,
    population: Res<AntPopulation>,
    nest_store: Res<NestStore>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();
    let mut nurses = 0usize;
    let mut foragers = 0usize;

    for (ant, _) in &mut ant_query {
        if ant.phase == AntPhase::Nursing {
            nurses += 1;
        }

        if ant.phase == AntPhase::Foraging || ant.is_laden() {
            foragers += 1;
        }
    }

    let population = population.count();
    let stimulus = forage_stimulus(nest_store.food(), population, foragers);
    let nurse_ratio = if population == 0 {
        0.0
    } else {
        nurses as f32 / population as f32
    };
    let reversion = reversion_allowed(stimulus, nurse_ratio);

    for (mut ant, rng) in &mut ant_query {
        match ant.phase {
            AntPhase::Nursing if ant.nursing_over() || stimulus > ant.forage_threshold => {
                ant.mature();
            }
            AntPhase::Foraging if reversion && !ant.is_laden() && !ant.is_handling() => {
                if let Some(mut rng) = rng
                    && rng.0.f32() < FORAGER_REVERSION_RATE * dt
                {
                    ant.revert_to_nursing();
                }
            }
            _ => {}
        }
    }
}

/// Background mortality hazard for ants outside the nest disc.
///
/// `P_die = 1 - exp(-MORTALITY_HAZARD * dt)` is drawn from each ant's own
/// [`AntRng`]; a death leaves a [`Corpse`] at the ant's position and bumps
/// [`ColonyStats::deaths`] and [`AntPopulation`].
pub fn mortality(
    mut commands: Commands,
    mut ant_query: Query<(Entity, &Ant, &Transform, &mut AntRng)>,
    nest_position: Res<NestPosition>,
    mut population: ResMut<AntPopulation>,
    mut colony: ResMut<ColonyStats>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();
    let p_die = 1.0 - (-MORTALITY_HAZARD * dt).exp();
    let nest_pos = nest_position.0;
    let nest_radius_squared = NEST_RADIUS * NEST_RADIUS;

    for (entity, _ant, transform, mut rng) in &mut ant_query {
        let pos = transform.translation.truncate();

        if pos.distance_squared(nest_pos) < nest_radius_squared {
            continue;
        }

        if rng.0.f32() < p_die {
            spawn_corpse(&mut commands, pos);
            commands.entity(entity).despawn();
            population.remove(1);
            colony.record_death();
        }
    }
}

/// Bucketed spatial index over every corpse, rebuilt once per fixed tick.
///
/// Cell size is [`DENSITY_CELL_SIZE`] (8 u), which is at least
/// [`CORPSE_PICKUP_RADIUS`] (3 u), so every corpse within pickup range of an
/// ant lies in the 3x3 neighborhood of the ant's cell. Buckets keep their
/// capacity between ticks and [`CorpseGrid::rebuild`] clears only the cells
/// recorded in `touched`, so maintenance is O(corpses) per tick instead of
/// O(grid cells), and no per-tick heap growth accumulates in steady state.
///
/// Determinism: corpses are inserted in query order (stable entity order) and
/// queried by scanning the 3x3 neighborhood in a fixed `(dy, dx)` order, with
/// each bucket in insertion order. A claim removes the corpse from its bucket,
/// so a corpse is claimed by at most one ant per tick. The whole pass depends
/// only on the entity iteration order and the corpse positions.
pub(crate) struct CorpseGrid {
    cells: Box<[Vec<(Entity, Vec2)>]>,
    /// Cells holding at least one corpse after the last [`Self::rebuild`];
    /// only these need clearing on the next rebuild.
    touched: Vec<usize>,
}

impl Default for CorpseGrid {
    fn default() -> Self {
        Self {
            cells: (0..DENSITY_GRID_WIDTH * DENSITY_GRID_HEIGHT)
                .map(|_| Vec::new())
                .collect(),
            touched: Vec::new(),
        }
    }
}

impl CorpseGrid {
    /// Rebuild the index from every corpse, reusing bucket capacity.
    fn rebuild(&mut self, corpses: impl Iterator<Item = (Entity, Vec2)>) {
        for &index in &self.touched {
            self.cells[index].clear();
        }
        self.touched.clear();

        for (entity, pos) in corpses {
            let index = corpse_cell(pos);

            if self.cells[index].is_empty() {
                self.touched.push(index);
            }

            self.cells[index].push((entity, pos));
        }
    }

    /// Whether the last [`Self::rebuild`] found no corpses at all.
    fn is_empty(&self) -> bool {
        self.touched.is_empty()
    }

    /// Claim the first corpse within `radius_squared` of `pos` in the fixed
    /// neighborhood scan order, removing it so no other ant can claim it this
    /// tick.
    fn claim(&mut self, pos: Vec2, radius_squared: f32) -> Option<Entity> {
        let index = corpse_cell(pos);
        let cell_x = index % DENSITY_GRID_WIDTH;
        let cell_y = index / DENSITY_GRID_WIDTH;

        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let x = cell_x as i32 + dx;
                let y = cell_y as i32 + dy;

                if x < 0
                    || y < 0
                    || x >= DENSITY_GRID_WIDTH as i32
                    || y >= DENSITY_GRID_HEIGHT as i32
                {
                    continue;
                }

                let bucket = &mut self.cells[y as usize * DENSITY_GRID_WIDTH + x as usize];

                if let Some(claimed) = bucket
                    .iter()
                    .position(|(_, corpse_pos)| corpse_pos.distance_squared(pos) <= radius_squared)
                {
                    // `remove` (not `swap_remove`) keeps each bucket in
                    // insertion order for the rest of the tick.
                    let (entity, _) = bucket.remove(claimed);
                    return Some(entity);
                }
            }
        }

        None
    }
}

/// Flat index of the corpse-grid cell containing `pos`.
///
/// Unlike [`world_to_index`] this mapping is total: wall clamping leaves ants
/// exactly on `±PLAY_AREA_WIDTH/2` / `±PLAY_AREA_HEIGHT/2`, which the
/// half-open play area would reject. Clamping `pos` into the grid moves at
/// most half a cell at the positive edges (still inside the last cell), so
/// boundary ants and corpses stay addressable and distant out-of-bounds
/// positions simply land in the nearest edge cell.
fn corpse_cell(pos: Vec2) -> usize {
    let limit = Vec2::new(
        PLAY_AREA_WIDTH / 2.0 - DENSITY_CELL_SIZE / 2.0,
        PLAY_AREA_HEIGHT / 2.0 - DENSITY_CELL_SIZE / 2.0,
    );

    world_to_index(
        pos.clamp(-limit, limit),
        DENSITY_CELL_SIZE,
        DENSITY_GRID_WIDTH,
        DENSITY_GRID_HEIGHT,
    )
    .expect("a clamped position lies inside the play area")
}

/// Foraging ants pick up the first corpse within [`CORPSE_PICKUP_RADIUS`]
/// (necrophoresis), queried from a bucketed corpse index over the 3x3 cell
/// neighborhood of the ant. The ant is held in the "busy" state while carrying
/// so movement, collision and deposit skip it; [`carry_corpses`] walks it to
/// the refuse pile.
///
/// The index is rebuilt once per tick in a fixed order, so the pass is
/// O(ants + corpses) instead of O(ants * corpses). Claimed corpses are removed
/// from the index immediately, preserving the "one corpse per ant, one ant per
/// corpse per tick" rule. The system is allocation-free in steady state: the
/// [`Local`] grid only ever grows bucket capacity.
pub(crate) fn pick_up_corpses(
    mut commands: Commands,
    mut ant_query: Query<(&mut Ant, &Transform)>,
    corpse_query: Query<(Entity, &Transform), With<Corpse>>,
    mut grid: Local<CorpseGrid>,
) {
    grid.rebuild(
        corpse_query
            .iter()
            .map(|(entity, transform)| (entity, transform.translation.truncate())),
    );

    if grid.is_empty() {
        return;
    }

    let pickup_squared = CORPSE_PICKUP_RADIUS * CORPSE_PICKUP_RADIUS;

    for (mut ant, transform) in &mut ant_query {
        if ant.carrying_corpse
            || ant.is_laden()
            || ant.is_handling()
            || ant.phase != AntPhase::Foraging
        {
            continue;
        }

        let pos = transform.translation.truncate();
        let Some(corpse) = grid.claim(pos, pickup_squared) else {
            continue;
        };

        commands.entity(corpse).despawn();
        ant.carrying_corpse = true;
        ant.start_handling();
    }
}

/// Walk corpse carriers to [`NestGeometry::refuse`] and drop them within
/// [`CORPSE_DROP_RADIUS`]. Because the carrier is held "busy" (see
/// [`crate::simulation::ant::Ant::tick_handling`]), this is the only system
/// that moves it, so the trip is exact and independent of steering.
pub fn carry_corpses(
    mut commands: Commands,
    mut ant_query: Query<(&mut Ant, &mut Transform)>,
    geometry: Res<NestGeometry>,
    mut colony: ResMut<ColonyStats>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();
    let refuse = geometry.refuse;
    let drop_squared = CORPSE_DROP_RADIUS * CORPSE_DROP_RADIUS;

    for (mut ant, mut transform) in &mut ant_query {
        if !ant.carrying_corpse {
            continue;
        }

        let pos = transform.translation.truncate();
        let to_refuse = refuse - pos;

        if to_refuse.length_squared() <= drop_squared {
            spawn_corpse(&mut commands, pos);
            ant.carrying_corpse = false;
            ant.handling_timer = 0.0;
            ant.speed = 0.0;
            colony.record_refuse();
            continue;
        }

        let direction = to_refuse.normalize();
        let step = direction * CORPSE_CARRY_SPEED * dt;
        transform.translation.x += step.x;
        transform.translation.y += step.y;
        ant.direction = direction.y.atan2(direction.x);
        ant.speed = CORPSE_CARRY_SPEED;
    }
}

/// Decay corpses: after [`CORPSE_TTL`] seconds they despawn.
pub fn decay_corpses(
    mut commands: Commands,
    mut corpse_query: Query<(Entity, &mut Corpse)>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();

    for (entity, mut corpse) in &mut corpse_query {
        corpse.ttl -= dt;

        if corpse.ttl <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Wiring hook for the colony lifecycle stream: mortality, corpses,
/// necrophoresis and flexible task allocation.
pub fn register(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        (
            allocate_tasks,
            mortality,
            pick_up_corpses,
            carry_corpses,
            decay_corpses,
        )
            .chain()
            .after(crate::simulation::ant::update_ant_energy_age)
            .in_set(SimSet::Lifecycle),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::colony::{FORAGE_THRESHOLD_MAX, FORAGE_THRESHOLD_MIN, NEST_STORE_CAP};
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;

    const DT: f32 = 1.0 / 64.0;

    fn step(world: &mut World, dt: f32) {
        world
            .resource_mut::<Time<Fixed>>()
            .advance_by(Duration::from_secs_f32(dt));
    }

    fn base_world() -> World {
        let mut world = World::new();
        world.insert_resource(Time::<Fixed>::from_hz(64.0));
        world.init_resource::<AntPopulation>();
        world.init_resource::<ColonyStats>();
        world.insert_resource(NestStore::default());
        world.insert_resource(NestPosition::default());
        world.insert_resource(NestGeometry::default());
        world
    }

    fn corpse_positions(world: &mut World) -> Vec<Vec2> {
        let mut query = world.query::<(&Corpse, &Transform)>();
        query
            .iter(world)
            .map(|(_, transform)| transform.translation.truncate())
            .collect()
    }

    #[test]
    fn forage_stimulus_rises_with_shortage_and_falls_with_the_store() {
        // Full store, target forager share met.
        assert_eq!(
            forage_stimulus(RECRUIT_THRESHOLD * 2.0, 100, 100),
            0.0,
            "a satiated colony with enough foragers has no stimulus"
        );

        // Empty store alone already asks for foragers.
        assert!((forage_stimulus(0.0, 100, 100) - 1.0).abs() < 1e-6);

        // No foragers at all is a full shortage.
        assert!((forage_stimulus(RECRUIT_THRESHOLD, 100, 0) - 1.0).abs() < 1e-6);

        // Half the target foragers is half the shortage.
        let half_target = (TARGET_FORAGER_FRACTION * 100.0 * 0.5) as usize;
        let partial = forage_stimulus(RECRUIT_THRESHOLD, 100, half_target);
        assert!((partial - 0.5).abs() < 1e-6, "got {partial}");

        // The shortage saturates toward 1.
        let almost = 1.0 - 1.0 / (TARGET_FORAGER_FRACTION * 100.0);
        assert!((forage_stimulus(RECRUIT_THRESHOLD, 100, 1) - almost).abs() < 1e-6);

        // An empty colony has no target to fall short of.
        assert_eq!(forage_stimulus(RECRUIT_THRESHOLD, 0, 0), 0.0);
    }

    #[test]
    fn reversion_needs_low_stimulus_and_low_nurse_ratio() {
        assert!(reversion_allowed(0.0, 0.0));
        assert!(reversion_allowed(
            REVERSION_STIMULUS - 0.01,
            REVERSION_NURSE_RATIO - 0.01
        ));
        assert!(!reversion_allowed(REVERSION_STIMULUS, 0.0));
        assert!(!reversion_allowed(0.0, REVERSION_NURSE_RATIO));
    }

    #[test]
    fn nurses_mature_early_under_shortage() {
        let mut world = base_world();
        world.insert_resource(NestStore::with_food(0.0));

        let mut eager = Ant::test_ant(0.0);
        eager.phase = AntPhase::Nursing;
        eager.forage_threshold = FORAGE_THRESHOLD_MIN;
        let eager_id = world.spawn(eager).id();

        let mut picky = Ant::test_ant(0.0);
        picky.phase = AntPhase::Nursing;
        picky.forage_threshold = FORAGE_THRESHOLD_MAX;
        let picky_id = world.spawn(picky).id();

        world.resource_mut::<AntPopulation>().add(2);

        step(&mut world, DT);
        world.run_system_once(allocate_tasks).unwrap();

        assert_eq!(
            world.get::<Ant>(eager_id).unwrap().phase,
            AntPhase::Foraging
        );
        assert_eq!(
            world.get::<Ant>(picky_id).unwrap().phase,
            AntPhase::Foraging,
            "an empty store plus full shortage (S = 2) exceeds every threshold"
        );
    }

    #[test]
    fn nurses_respect_their_threshold_when_the_colony_is_satiated() {
        let mut world = base_world();
        world.insert_resource(NestStore::with_food(RECRUIT_THRESHOLD));

        // Ten foragers satisfy the target share, so S = 0.
        for _ in 0..10 {
            let mut forager = Ant::test_ant(0.0);
            forager.phase = AntPhase::Foraging;
            world.spawn(forager);
        }

        let mut nurse = Ant::test_ant(0.0);
        nurse.phase = AntPhase::Nursing;
        nurse.forage_threshold = FORAGE_THRESHOLD_MIN;
        let nurse_id = world.spawn(nurse).id();
        world.resource_mut::<AntPopulation>().add(11);

        step(&mut world, DT);
        world.run_system_once(allocate_tasks).unwrap();
        assert_eq!(
            world.get::<Ant>(nurse_id).unwrap().phase,
            AntPhase::Nursing,
            "S = 0 must not mature a nurse with threshold {FORAGE_THRESHOLD_MIN}"
        );

        // Draining the store pushes the stimulus to 1 > threshold.
        world.insert_resource(NestStore::with_food(0.0));
        step(&mut world, DT);
        world.run_system_once(allocate_tasks).unwrap();
        assert_eq!(
            world.get::<Ant>(nurse_id).unwrap().phase,
            AntPhase::Foraging
        );
    }

    #[test]
    fn foragers_revert_when_the_stimulus_and_nurse_ratio_are_low() {
        let mut world = base_world();
        world.insert_resource(NestStore::with_food(NEST_STORE_CAP));

        for index in 0..200u64 {
            let mut forager = Ant::test_ant(0.0);
            forager.phase = AntPhase::Foraging;
            world.spawn((forager, AntRng::for_spawn(index)));
        }
        world.resource_mut::<AntPopulation>().add(200);

        let mut ticks = 0;
        for _ in 0..(64 * 10) {
            step(&mut world, DT);
            world.run_system_once(allocate_tasks).unwrap();
            ticks += 1;
        }

        let mut nurses = 0;
        let mut foragers = 0;
        {
            let mut query = world.query::<&Ant>();
            for ant in query.iter(&world) {
                match ant.phase {
                    AntPhase::Nursing => nurses += 1,
                    AntPhase::Foraging => foragers += 1,
                    AntPhase::Returning => {}
                }
            }
        }

        assert!(
            nurses >= 1,
            "some foragers must revert over {ticks} ticks (S = 0, nurse ratio 0)"
        );
        assert!(
            (nurses as f32 / 200.0) <= REVERSION_NURSE_RATIO + 0.02,
            "reversion must stop once the nurse ratio reaches the target, got {nurses} nurses"
        );
        assert_eq!(nurses + foragers, 200, "reversion must not lose ants");
    }

    #[test]
    fn mortality_rate_matches_the_hazard_within_bounds() {
        let mut world = base_world();
        let ants = 2000usize;

        for index in 0..ants {
            // Spread the ants far outside the nest disc.
            let pos = Vec2::new(
                -390.0 + (index % 100) as f32 * 7.0,
                -290.0 + (index / 100) as f32 * 7.0,
            );
            world.spawn((
                Ant::test_ant(0.0),
                Transform::from_xyz(pos.x, pos.y, 0.0),
                AntRng::for_spawn(index as u64),
            ));
        }
        world.resource_mut::<AntPopulation>().add(ants);

        let ticks = 640;
        for _ in 0..ticks {
            step(&mut world, DT);
            world.run_system_once(mortality).unwrap();
        }

        let deaths = world.resource::<ColonyStats>().deaths as f32;
        let expected = ants as f32 * (1.0 - (-MORTALITY_HAZARD * ticks as f32 * DT).exp());
        assert!(
            deaths > expected * 0.3 && deaths < expected * 2.0,
            "hazard deaths {deaths} far from the expected ~{expected}"
        );
        assert_eq!(
            world.resource::<AntPopulation>().count(),
            ants - deaths as usize,
            "every death must decrement the population"
        );

        let corpses = corpse_positions(&mut world).len();
        assert_eq!(
            corpses, deaths as usize,
            "every death must leave exactly one corpse"
        );
    }

    #[test]
    fn age_death_leaves_a_corpse_and_counts_it() {
        let mut world = base_world();

        let mut ant = Ant::test_ant(0.0);
        ant.age = ant.max_lifetime;
        let death_pos = Vec2::new(5.0, 5.0);
        world.spawn((ant, Transform::from_xyz(death_pos.x, death_pos.y, 0.0)));
        world.resource_mut::<AntPopulation>().add(1);

        step(&mut world, DT);
        world
            .run_system_once(crate::simulation::ant::update_ant_energy_age)
            .unwrap();

        assert_eq!(world.resource::<AntPopulation>().count(), 0);
        assert_eq!(world.resource::<ColonyStats>().deaths, 1);

        let corpses = corpse_positions(&mut world);
        assert_eq!(corpses.len(), 1);
        assert!(corpses[0].distance(death_pos) < 1e-6);
    }

    #[test]
    fn corpses_decay_after_their_ttl() {
        let mut world = base_world();
        let doomed = world
            .spawn((Corpse { ttl: DT * 2.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        let survivor = world
            .spawn((Corpse { ttl: 60.0 }, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();

        step(&mut world, DT);
        world.run_system_once(decay_corpses).unwrap();
        assert!(world.get_entity(doomed).is_ok(), "still within its ttl");

        step(&mut world, DT);
        world.run_system_once(decay_corpses).unwrap();
        assert!(world.get_entity(doomed).is_err(), "ttl reached: decayed");
        assert!(world.get_entity(survivor).is_ok(), "fresh corpse survives");
    }

    #[test]
    fn ants_carry_corpses_to_the_refuse_pile() {
        let mut world = base_world();
        let refuse = Vec2::new(50.0, 0.0);
        world.insert_resource(NestGeometry {
            entrance: Vec2::ZERO,
            entrance_radius: NEST_RADIUS,
            refuse,
        });

        let corpse = world
            .spawn((
                Corpse { ttl: CORPSE_TTL },
                Transform::from_xyz(2.0, 0.0, 0.0),
            ))
            .id();
        let ant_entity = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();

        step(&mut world, DT);
        world.run_system_once(pick_up_corpses).unwrap();

        let ant = world.get::<Ant>(ant_entity).unwrap();
        assert!(ant.carrying_corpse, "the ant must pick the corpse up");
        assert!(ant.is_handling(), "carrying keeps the ant busy");
        assert!(
            world.get_entity(corpse).is_err(),
            "the picked-up corpse entity is consumed"
        );

        let mut ticks = 0;
        while world.get::<Ant>(ant_entity).unwrap().carrying_corpse && ticks < 64 * 10 {
            step(&mut world, DT);
            world.run_system_once(carry_corpses).unwrap();
            ticks += 1;
        }

        let ant = world.get::<Ant>(ant_entity).unwrap();
        assert!(!ant.carrying_corpse, "the corpse must be dropped");
        assert!(!ant.is_handling(), "the ant resumes after the dropoff");
        assert_eq!(world.resource::<ColonyStats>().refuse, 1);

        let ant_pos = world
            .get::<Transform>(ant_entity)
            .unwrap()
            .translation
            .truncate();
        assert!(
            ant_pos.distance(refuse) <= CORPSE_DROP_RADIUS + 1e-3,
            "the ant must reach the refuse pile, at {ant_pos:?}"
        );

        let corpses = corpse_positions(&mut world);
        assert_eq!(corpses.len(), 1, "the corpse reappears at the refuse pile");
        assert!(corpses[0].distance(refuse) <= CORPSE_DROP_RADIUS + 1e-3);
    }

    #[test]
    fn corpse_pickup_ignores_laden_and_nursing_ants() {
        let mut world = base_world();

        let corpse = world
            .spawn((
                Corpse { ttl: CORPSE_TTL },
                Transform::from_xyz(1.0, 0.0, 0.0),
            ))
            .id();

        let mut laden = Ant::test_ant(0.0);
        laden.pick_up(1.0, 1.0);
        let laden_id = world
            .spawn((laden, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();

        let mut nurse = Ant::test_ant(0.0);
        nurse.phase = AntPhase::Nursing;
        let nurse_id = world
            .spawn((nurse, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();

        step(&mut world, DT);
        world.run_system_once(pick_up_corpses).unwrap();

        assert!(!world.get::<Ant>(laden_id).unwrap().carrying_corpse);
        assert!(!world.get::<Ant>(nurse_id).unwrap().carrying_corpse);
        assert!(
            world.get_entity(corpse).is_ok(),
            "the corpse must stay for a foraging ant"
        );
    }

    /// The corpse index is bucketed at [`DENSITY_CELL_SIZE`]; a corpse in the
    /// neighboring cell of the ant's cell must still be found.
    #[test]
    fn corpse_pickup_crosses_grid_cell_boundaries() {
        let mut world = base_world();

        // A density cell boundary sits at x = 0, so the two positions below
        // are in different cells one unit apart.
        let corpse = world
            .spawn((
                Corpse { ttl: CORPSE_TTL },
                Transform::from_xyz(0.5, 0.0, 0.0),
            ))
            .id();
        let ant_id = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(-0.5, 0.0, 0.0)))
            .id();

        step(&mut world, DT);
        world.run_system_once(pick_up_corpses).unwrap();

        assert!(
            world.get::<Ant>(ant_id).unwrap().carrying_corpse,
            "a corpse one cell over must be claimable"
        );
        assert!(world.get_entity(corpse).is_err());
    }

    /// A corpse may be claimed by at most one ant per tick.
    #[test]
    fn one_corpse_is_claimed_by_a_single_ant() {
        let mut world = base_world();

        let corpse = world
            .spawn((
                Corpse { ttl: CORPSE_TTL },
                Transform::from_xyz(1.0, 0.0, 0.0),
            ))
            .id();
        let first = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        let second = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(0.5, 0.0, 0.0)))
            .id();

        step(&mut world, DT);
        world.run_system_once(pick_up_corpses).unwrap();

        let carriers = [first, second]
            .into_iter()
            .filter(|entity| world.get::<Ant>(*entity).unwrap().carrying_corpse)
            .count();
        assert_eq!(carriers, 1, "exactly one ant may claim the corpse");
        assert!(world.get_entity(corpse).is_err());
    }

    /// Corpses outside [`CORPSE_PICKUP_RADIUS`] are left alone.
    #[test]
    fn corpse_outside_pickup_radius_is_not_claimed() {
        let mut world = base_world();

        let corpse = world
            .spawn((
                Corpse { ttl: CORPSE_TTL },
                Transform::from_xyz(CORPSE_PICKUP_RADIUS + 1.0, 0.0, 0.0),
            ))
            .id();
        let ant_id = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();

        step(&mut world, DT);
        world.run_system_once(pick_up_corpses).unwrap();

        assert!(!world.get::<Ant>(ant_id).unwrap().carrying_corpse);
        assert!(world.get_entity(corpse).is_ok());
    }

    /// Wall clamping puts ants exactly on the half-open play-area edge; a
    /// corpse there must still be addressable by the index.
    #[test]
    fn corpse_on_the_wall_boundary_is_claimable() {
        use crate::constants::world::{PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};

        let mut world = base_world();
        let edge = Vec2::new(PLAY_AREA_WIDTH / 2.0, -PLAY_AREA_HEIGHT / 2.0);

        let corpse = world
            .spawn((
                Corpse { ttl: CORPSE_TTL },
                Transform::from_xyz(edge.x, edge.y, 0.0),
            ))
            .id();
        let ant_id = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(edge.x, edge.y, 0.0)))
            .id();

        step(&mut world, DT);
        world.run_system_once(pick_up_corpses).unwrap();

        assert!(
            world.get::<Ant>(ant_id).unwrap().carrying_corpse,
            "a corpse on the play-area edge must be claimable"
        );
        assert!(world.get_entity(corpse).is_err());
    }

    /// 120 s performance analysis harness (ignored; run manually).
    ///
    /// Builds the real chain (`build_app` + environment + nest + lifecycle)
    /// and compares three variants at a fixed population cap: no lifecycle,
    /// the full lifecycle, and the full lifecycle with corpses culled every
    /// tick. The last one isolates the corpse-handling cost from the rest of
    /// the chain.
    ///
    /// Env knobs: `PERF120_SECS` (default 120), `PERF120_CAP` (default 10000)
    /// and `PERF120_VARIANTS` (comma-separated subset of `none,full,cull`).
    #[test]
    #[ignore = "120 s analysis; run manually"]
    fn perf120_analysis() {
        use crate::simulation::ant::AntSpawner;
        use crate::simulation::trail_tests::{FoodSetup, build_app};
        use crate::simulation::{environment, nest};
        use std::time::Instant;

        #[derive(Clone, Copy)]
        enum Variant {
            None,
            Full,
            Cull,
        }

        fn corpse_count(app: &mut App) -> usize {
            let world = app.world_mut();
            let mut query = world.query::<&Corpse>();
            query.iter(world).count()
        }

        fn cull_corpses(mut commands: Commands, corpses: Query<Entity, With<Corpse>>) {
            for entity in &corpses {
                commands.entity(entity).despawn();
            }
        }

        fn run(label: &str, variant: Variant, cap: usize, secs: u32) {
            let mut app = build_app(FoodSetup::Real);
            environment::register(&mut app);
            nest::register(&mut app);

            match variant {
                Variant::None => {}
                Variant::Full => register(&mut app),
                Variant::Cull => {
                    register(&mut app);
                    app.add_systems(
                        FixedUpdate,
                        cull_corpses
                            .before(pick_up_corpses)
                            .in_set(SimSet::Lifecycle),
                    );
                }
            }

            // Warm up to the population cap (or the whole budget if it never
            // gets there), then stop the spawner so the population is fixed.
            let mut warm = 0u32;
            while app.world().resource::<AntPopulation>().count() < cap && warm < secs * 64 {
                app.update();
                warm += 1;
            }
            app.world_mut()
                .resource_mut::<AntSpawner>()
                .timer
                .set_duration(Duration::from_secs(3600));

            println!(
                "[{label}] warmup {warm} ticks, pop={} corpses={}",
                app.world().resource::<AntPopulation>().count(),
                corpse_count(&mut app)
            );

            let mut sum = 0f64;
            for sec in 1..=secs {
                let start = Instant::now();
                for _ in 0..64 {
                    app.update();
                }
                let ms = start.elapsed().as_secs_f64() * 1000.0 / 64.0;
                sum += ms;

                if sec % 10 == 0 {
                    println!(
                        "[{label}] t={sec:>3}s chain={ms:8.3} ms/tick pop={:>6} corpses={:>5}",
                        app.world().resource::<AntPopulation>().count(),
                        corpse_count(&mut app),
                    );
                }
            }
            println!(
                "[{label}] {secs} s average = {:.3} ms/tick",
                sum / secs as f64
            );
        }

        let secs: u32 = std::env::var("PERF120_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(120);
        let cap: usize = std::env::var("PERF120_CAP")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10_000);
        let variants =
            std::env::var("PERF120_VARIANTS").unwrap_or_else(|_| "none,full,cull".into());

        for variant in variants.split(',') {
            match variant.trim() {
                "none" => run("no-lifecycle", Variant::None, cap, secs),
                "full" => run("full", Variant::Full, cap, secs),
                "cull" => run("no-corpses", Variant::Cull, cap, secs),
                other => panic!("unknown PERF120_VARIANTS entry {other:?}"),
            }
        }
    }

    /// Integration harness for the frozen trail gate: runs the real emergence
    /// scenario with the lifecycle systems registered (mortality + rest
    /// enabled) and checks the colony still reaches the delivery floor and
    /// forms the corridor.
    ///
    /// The navigation stream has not landed its `rest_timer` skip yet, so the
    /// harness emulates it locally (`hold_resting_ants` restores the pre-move
    /// position) to measure the worst case. Ignored because it duplicates the
    /// 45 s regression.
    #[test]
    #[ignore = "trail-gate harness with mortality/rest enabled; run manually"]
    fn mortality_and_rest_keep_the_trail_gate_green() {
        use crate::constants::world::{NEST_X, NEST_Y};
        use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
        use crate::simulation::trail_tests::{FoodSetup, build_app, corridor_avg, run_seconds};

        /// Emulate the navigation stream's "move_ants skips resting ants".
        fn hold_resting_ants(mut query: Query<(&Ant, &mut Transform)>) {
            for (ant, mut transform) in &mut query {
                if ant.rest_timer > 0.0 {
                    transform.translation.x = ant.prev_pos.x;
                    transform.translation.y = ant.prev_pos.y;
                }
            }
        }

        let food_x = NEST_X + 450.0;
        let mut app = build_app(FoodSetup::Custom(Vec2::new(food_x, NEST_Y)));
        register(&mut app);
        app.add_systems(
            FixedUpdate,
            hold_resting_ants
                .after(crate::simulation::movement::move_ants)
                .in_set(SimSet::Move),
        );
        run_seconds(&mut app, 45, Some(800));

        let deliveries = app.world().resource::<ColonyStats>().total_food_delivered;
        let deaths = app.world().resource::<ColonyStats>().deaths;
        let grid = app.world().resource::<PheromoneGrid>();
        let corridor = corridor_avg(grid, PheromoneKind::ToFood, NEST_Y, food_x);

        println!("deliveries={deliveries} deaths={deaths} corridor={corridor}");

        assert!(
            deliveries >= 10.0,
            "expected at least 10 deliveries with mortality/rest on, got {deliveries}"
        );
        assert!(
            corridor >= 0.05,
            "expected a ToFood corridor with mortality/rest on, got {corridor}"
        );
    }
}
