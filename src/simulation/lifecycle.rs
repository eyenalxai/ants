//! Colony lifecycle: the queen and the staged brood pipeline, mortality,
//! corpses, necrophoresis and flexible task allocation.
//!
//! Every system here is registered through [`register`] into
//! [`SimSet::Lifecycle`], after the age/energy pass in
//! [`crate::simulation::ant::update_ant_energy_age`]. Random draws come from
//! the ant's own [`AntRng`], so the module stays deterministic. The brood
//! pipeline itself uses no RNG at all: stage timers advance with
//! `Time<Fixed>` deltas, eggs are placed on a deterministic spiral and each
//! brood item carries the per-ant RNG stream index it was assigned at laying.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::constants::ant::{ANT_SIZE, MAX_ANTS};
use crate::constants::colony::{
    FORAGER_REVERSION_RATE, LARVA_FOOD_PER_SEC, QUEEN_EGG_RATE, QUEEN_LAYING_FULL_STORE,
    QUEEN_LAYING_THRESHOLD, RECRUIT_THRESHOLD, REVERSION_NURSE_RATIO, REVERSION_STIMULUS,
    TARGET_FORAGER_FRACTION,
};
use crate::constants::lifecycle::{
    BROOD_AREA_RADIUS, BROOD_SPIRAL_ANGLE, BROOD_SPIRAL_STEP, BROOD_TEND_BONUS, BROOD_TEND_RADIUS,
    BROOD_Z, CORPSE_CARRY_SPEED, CORPSE_DROP_RADIUS, CORPSE_PICKUP_RADIUS, CORPSE_TTL, CORPSE_Z,
    EGG_DURATION, INITIAL_BROOD, INITIAL_NURSES, LARVA_DURATION, MORTALITY_HAZARD, PUPA_DURATION,
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

// --- Queen and staged brood pipeline (F8) ------------------------------------

/// The colony's queen: a single egg-laying resource.
///
/// She is the only source of new adults now. [`crate::simulation::ant::
/// queen_lays_eggs`] (registered as `ant::spawn_ants` in [`SimSet::Spawn`])
/// turns [`Queen::lay_rate`] into [`Brood`] eggs, [`develop_brood`] grows them
/// through larva and pupa, and the completed pupa ecloses into a callow
/// worker.
#[derive(Resource)]
pub struct Queen {
    /// Eggs per second at full activity and a fully open store.
    pub egg_rate: f32,
    /// Fractional eggs carried between laying windows.
    pub egg_accumulator: f32,
    /// Stream index for the next worker. Each brood item stores its own index
    /// when it is laid, so eclosion order can never change an ant's RNG
    /// stream.
    next_spawn_index: u64,
}

impl Default for Queen {
    fn default() -> Self {
        Self {
            egg_rate: QUEEN_EGG_RATE,
            egg_accumulator: 0.0,
            next_spawn_index: 0,
        }
    }
}

impl Queen {
    /// Egg-laying rate per second: zero at or below [`QUEEN_LAYING_THRESHOLD`],
    /// then scaled linearly with the store up to [`QUEEN_LAYING_FULL_STORE`]
    /// and by the day-night
    /// [`crate::simulation::environment::SimClock::activity`].
    pub fn lay_rate(&self, activity: f32, store_food: f32) -> f32 {
        if store_food <= QUEEN_LAYING_THRESHOLD {
            return 0.0;
        }

        let span = QUEEN_LAYING_FULL_STORE - QUEEN_LAYING_THRESHOLD;
        let store_scale = if span <= 0.0 {
            1.0
        } else {
            ((store_food - QUEEN_LAYING_THRESHOLD) / span).clamp(0.0, 1.0)
        };
        self.egg_rate * activity.clamp(0.0, 1.0) * store_scale
    }

    /// Accumulate `rate * window` eggs and return the whole ones. The
    /// fractional remainder is carried, so a rate below one egg per window
    /// still lays deterministically over time.
    pub fn take_eggs(&mut self, rate: f32, window: f32) -> usize {
        if !rate.is_finite() || rate <= 0.0 || !window.is_finite() || window <= 0.0 {
            return 0;
        }

        self.egg_accumulator += rate * window;
        let eggs = self.egg_accumulator.floor();
        self.egg_accumulator -= eggs;
        eggs as usize
    }

    /// Whether the queen is currently able to lay at all (store above the
    /// threshold and a positive rate). Drives the HUD's queen line.
    pub fn is_laying(&self, store_food: f32) -> bool {
        store_food > QUEEN_LAYING_THRESHOLD && self.egg_rate > 0.0
    }

    /// Next deterministic per-worker RNG stream index. The first call returns
    /// `1`, matching the historical `spawn_ants` counter.
    pub fn next_spawn_index(&mut self) -> u64 {
        self.next_spawn_index = self.next_spawn_index.wrapping_add(1);
        self.next_spawn_index
    }
}

/// Developmental stage of a brood item.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BroodStage {
    /// Laid egg, hatching into a larva.
    #[default]
    Egg,
    /// Growing larva: consumes the nest store while it develops.
    Larva,
    /// Metamorphosing pupa: needs no food and ecloses into a callow worker.
    Pupa,
}

impl BroodStage {
    /// Seconds the stage lasts without tending.
    pub fn duration(self) -> f32 {
        match self {
            Self::Egg => EGG_DURATION,
            Self::Larva => LARVA_DURATION,
            Self::Pupa => PUPA_DURATION,
        }
    }

    /// Sprite size of the stage, in world units.
    pub fn size(self) -> f32 {
        match self {
            Self::Egg => ANT_SIZE * 0.7,
            Self::Larva => ANT_SIZE * 0.9,
            Self::Pupa => ANT_SIZE,
        }
    }

    /// Sprite color of the stage.
    pub fn color(self) -> Color {
        match self {
            Self::Egg => Color::srgb(0.95, 0.95, 0.8),
            Self::Larva => Color::srgb(0.9, 0.75, 0.4),
            Self::Pupa => Color::srgb(0.7, 0.55, 0.3),
        }
    }
}

/// A brood item: the queen's egg, growing through larva and pupa until it
/// ecloses into a callow worker.
#[derive(Component)]
pub struct Brood {
    /// Current developmental stage.
    pub stage: BroodStage,
    /// Seconds spent in the current stage; advances only while the item is fed
    /// (larvae) and is accelerated by tending.
    pub timer: f32,
    /// Deterministic per-ant RNG stream index for the adult this brood
    /// becomes, assigned at laying time.
    pub spawn_index: u64,
}

/// Spawn one brood item at a deterministic position in the nest's brood area.
///
/// Successive items follow a golden-angle spiral (`BROOD_SPIRAL_ANGLE` /
/// `BROOD_SPIRAL_STEP`), so no RNG draw is needed and the placement is
/// reproducible across replays. Brood sprites are purely decorative: they have
/// no collision.
pub(crate) fn spawn_brood(
    commands: &mut Commands,
    stage: BroodStage,
    spawn_index: u64,
    nest_pos: Vec2,
) {
    let angle = spawn_index as f32 * BROOD_SPIRAL_ANGLE;
    let radius = BROOD_AREA_RADIUS * ((spawn_index as f32 * BROOD_SPIRAL_STEP).fract()).sqrt();
    let pos = nest_pos + Vec2::new(angle.cos(), angle.sin()) * radius;

    commands.spawn((
        Brood {
            stage,
            timer: 0.0,
            spawn_index,
        },
        Sprite {
            color: stage.color(),
            custom_size: Some(Vec2::splat(stage.size())),
            ..default()
        },
        Transform::from_xyz(pos.x, pos.y, BROOD_Z),
    ));
}

/// Bucketed spatial index over the nursing ants, rebuilt once per fixed tick,
/// used by [`develop_brood`] to decide whether a brood item is tended.
///
/// The cell size is [`DENSITY_CELL_SIZE`] and [`BROOD_TEND_RADIUS`] is at most
/// one cell, so scanning the 3x3 neighborhood of a brood item's cell is
/// exhaustive. Buckets keep their capacity between ticks and only the cells
/// recorded in `touched` are cleared, so maintenance is O(nurses) per tick.
pub(crate) struct NurseGrid {
    cells: Box<[Vec<Vec2>]>,
    /// Cells holding at least one nurse after the last rebuild; only these
    /// need clearing next time.
    touched: Vec<usize>,
}

impl Default for NurseGrid {
    fn default() -> Self {
        Self {
            cells: (0..DENSITY_GRID_WIDTH * DENSITY_GRID_HEIGHT)
                .map(|_| Vec::new())
                .collect(),
            touched: Vec::new(),
        }
    }
}

impl NurseGrid {
    /// Rebuild the index from every nursing ant, reusing bucket capacity.
    fn rebuild(&mut self, nurses: impl Iterator<Item = Vec2>) {
        for &index in &self.touched {
            self.cells[index].clear();
        }
        self.touched.clear();

        for pos in nurses {
            let index = density_cell(pos);

            if self.cells[index].is_empty() {
                self.touched.push(index);
            }

            self.cells[index].push(pos);
        }
    }

    /// Whether the last rebuild found no nurses at all.
    fn is_empty(&self) -> bool {
        self.touched.is_empty()
    }

    /// Whether any indexed nurse lies within `radius_squared` of `pos`.
    fn any_within(&self, pos: Vec2, radius_squared: f32) -> bool {
        let index = density_cell(pos);
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

                let bucket = &self.cells[y as usize * DENSITY_GRID_WIDTH + x as usize];

                if bucket
                    .iter()
                    .any(|nurse| nurse.distance_squared(pos) <= radius_squared)
                {
                    return true;
                }
            }
        }

        false
    }
}

/// Mutable colony inputs of the brood pass, bundled to keep the system
/// signature small.
#[derive(SystemParam)]
pub(crate) struct BroodInputs<'w> {
    nest_store: ResMut<'w, NestStore>,
    population: ResMut<'w, AntPopulation>,
    nest_position: Res<'w, NestPosition>,
    geometry: Res<'w, NestGeometry>,
}

/// Advance every brood item through its stages and eclose completed pupae into
/// callow workers.
///
/// Development is deterministic (`Time<Fixed>` deltas only):
/// - eggs and pupae advance at the base rate;
/// - a larva pays [`LARVA_FOOD_PER_SEC`] per second from the nest store and
///   stalls (its timer does not advance) while the store cannot cover it;
/// - a brood item within [`BROOD_TEND_RADIUS`] of a nursing ant advances at
///   `1 + BROOD_TEND_BONUS` times the base rate, so nursing shortens every
///   stage.
///
/// A completed pupa ecloses at [`NestGeometry::entrance`] into an
/// [`AntPhase::Nursing`] worker whose per-ant draws come from
/// [`AntRng::for_spawn`] with the index stored at laying time (see
/// [`crate::simulation::ant::spawn_worker`]). Eclosion is gated by
/// [`MAX_ANTS`]: a pupa at a full cap waits, keeping the population exact.
pub(crate) fn develop_brood(
    mut commands: Commands,
    mut brood_query: Query<(Entity, &mut Brood, &Transform)>,
    nurse_query: Query<(&Ant, &Transform)>,
    mut inputs: BroodInputs,
    time: Res<Time<Fixed>>,
    mut nurses: Local<NurseGrid>,
) {
    if brood_query.is_empty() {
        return;
    }

    let dt = time.delta_secs();
    nurses.rebuild(
        nurse_query
            .iter()
            .filter(|(ant, _)| ant.phase == AntPhase::Nursing)
            .map(|(_, transform)| transform.translation.truncate()),
    );

    let tend_squared = BROOD_TEND_RADIUS * BROOD_TEND_RADIUS;
    let mut spawned = 0usize;

    for (entity, mut brood, transform) in &mut brood_query {
        let duration = brood.stage.duration();

        // A pupa whose timer is already complete is only waiting for room;
        // feeding/tending it would be meaningless.
        if brood.stage != BroodStage::Pupa || brood.timer < duration {
            if brood.stage == BroodStage::Larva {
                let requested = LARVA_FOOD_PER_SEC * dt;
                let spent = inputs.nest_store.spend(requested);

                if spent + f32::EPSILON < requested {
                    continue;
                }
            }

            let tended = !nurses.is_empty()
                && nurses.any_within(transform.translation.truncate(), tend_squared);
            brood.timer += dt * (1.0 + if tended { BROOD_TEND_BONUS } else { 0.0 });

            if brood.timer >= duration {
                match brood.stage {
                    BroodStage::Egg => {
                        brood.stage = BroodStage::Larva;
                        brood.timer -= duration;
                    }
                    BroodStage::Larva => {
                        brood.stage = BroodStage::Pupa;
                        brood.timer -= duration;
                    }
                    // Completed this tick: keep the full timer so the eclosion
                    // check below fires (or waits at the population cap).
                    BroodStage::Pupa => {}
                }
            }
        }

        if brood.stage == BroodStage::Pupa && brood.timer >= duration {
            if inputs.population.count() + spawned < MAX_ANTS {
                let index = brood.spawn_index;
                crate::simulation::ant::spawn_worker(
                    &mut commands,
                    index,
                    inputs.geometry.entrance,
                    inputs.geometry.entrance_radius.max(0.0),
                    inputs.nest_position.0,
                );
                commands.entity(entity).despawn();
                spawned += 1;
            } else {
                // Hold the completed pupa until there is room.
                brood.timer = duration;
            }
        }
    }

    inputs.population.add(spawned);
}

/// Startup: seed the founding colony - [`INITIAL_NURSES`] callow nurses at the
/// nest plus the queen's [`INITIAL_BROOD`] founding eggs.
///
/// This is the documented gameplay bootstrap. It reproduces the opening
/// workforce of the old direct-spawn ramp (see [`INITIAL_NURSES`]) so the short
/// trail regression keeps its timing, while every worker after the founding
/// cohort comes out of the brood pipeline.
pub fn spawn_founding_colony(
    mut commands: Commands,
    mut queen: ResMut<Queen>,
    mut population: ResMut<AntPopulation>,
    nest_position: Res<NestPosition>,
    geometry: Res<NestGeometry>,
) {
    let entrance = geometry.entrance;
    let entrance_radius = geometry.entrance_radius.max(0.0);
    let nest_pos = nest_position.0;
    let nurses = INITIAL_NURSES.min(MAX_ANTS);
    let brood = INITIAL_BROOD.min(MAX_ANTS - nurses);

    for _ in 0..nurses {
        let index = queen.next_spawn_index();
        crate::simulation::ant::spawn_worker(
            &mut commands,
            index,
            entrance,
            entrance_radius,
            nest_pos,
        );
    }
    population.add(nurses);

    for _ in 0..brood {
        let index = queen.next_spawn_index();
        spawn_brood(&mut commands, BroodStage::Egg, index, nest_pos);
    }
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
            let index = density_cell(pos);

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
        let index = density_cell(pos);
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

/// Flat index of the density-grid cell containing `pos`.
///
/// Unlike [`world_to_index`] this mapping is total: wall clamping leaves ants
/// exactly on `±PLAY_AREA_WIDTH/2` / `±PLAY_AREA_HEIGHT/2`, which the
/// half-open play area would reject. Clamping `pos` into the grid moves at
/// most half a cell at the positive edges (still inside the last cell), so
/// boundary ants, corpses and brood stay addressable and distant
/// out-of-bounds positions simply land in the nearest edge cell.
///
/// Shared by [`CorpseGrid`] and [`NurseGrid`].
fn density_cell(pos: Vec2) -> usize {
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
/// Corpses already inside the refuse drop radius are excluded from the index:
/// they are disposed. Without the exclusion, an ant passing the pile picks one
/// up, [`carry_corpses`] sees it is already within [`CORPSE_DROP_RADIUS`] and
/// drops it again on the very next tick, and the pick-up/drop loop inflates
/// [`ColonyStats::refuse`] forever while keeping ants busy at the pile instead
/// of foraging.
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
    geometry: Res<NestGeometry>,
    mut grid: Local<CorpseGrid>,
) {
    let refuse = geometry.refuse;
    let drop_squared = CORPSE_DROP_RADIUS * CORPSE_DROP_RADIUS;

    grid.rebuild(
        corpse_query
            .iter()
            .map(|(entity, transform)| (entity, transform.translation.truncate()))
            .filter(|(_, pos)| pos.distance_squared(refuse) > drop_squared),
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

/// Wiring hook for the colony lifecycle stream: the founding colony, the
/// queen's brood pipeline, mortality, corpses, necrophoresis and flexible task
/// allocation.
pub fn register(app: &mut App) {
    app.init_resource::<Queen>()
        .add_systems(Startup, spawn_founding_colony)
        .add_systems(
            FixedUpdate,
            (
                allocate_tasks,
                develop_brood,
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

    // --- Queen and staged brood pipeline (F8) ------------------------------

    #[test]
    fn queen_lay_rate_is_gated_and_scaled_by_store_and_activity() {
        let queen = Queen::default();

        assert_eq!(queen.lay_rate(1.0, QUEEN_LAYING_THRESHOLD), 0.0);
        assert_eq!(queen.lay_rate(1.0, 0.0), 0.0);
        assert!(
            !queen.is_laying(QUEEN_LAYING_THRESHOLD),
            "at the threshold the queen is not laying"
        );

        let full = queen.lay_rate(1.0, NEST_STORE_CAP);
        assert!((full - QUEEN_EGG_RATE).abs() < 1e-6, "got {full}");

        let dim = queen.lay_rate(0.5, NEST_STORE_CAP);
        assert!((dim - QUEEN_EGG_RATE * 0.5).abs() < 1e-6, "got {dim}");

        // Halfway between the threshold and the full-store level is half rate.
        let half_store =
            QUEEN_LAYING_THRESHOLD + (QUEEN_LAYING_FULL_STORE - QUEEN_LAYING_THRESHOLD) * 0.5;
        let half = queen.lay_rate(1.0, half_store);
        assert!((half - QUEEN_EGG_RATE * 0.5).abs() < 1e-6, "got {half}");

        assert!(queen.is_laying(half_store));
    }

    #[test]
    fn take_eggs_carries_the_fraction_between_windows() {
        let mut queen = Queen::default();

        // 0.4 eggs per window: the first two windows yield nothing, the third
        // yields one egg and keeps 0.2.
        assert_eq!(queen.take_eggs(0.4, 1.0), 0);
        assert_eq!(queen.take_eggs(0.4, 1.0), 0);
        assert_eq!(queen.take_eggs(0.4, 1.0), 1);
        assert!((queen.egg_accumulator - 0.2).abs() < 1e-6);

        // Degenerate inputs are safe and lay nothing.
        assert_eq!(queen.take_eggs(0.0, 1.0), 0);
        assert_eq!(queen.take_eggs(1.0, 0.0), 0);
        assert_eq!(queen.take_eggs(f32::NAN, 1.0), 0);
        assert_eq!(queen.take_eggs(1.0, f32::INFINITY), 0);
    }

    #[test]
    fn brood_progresses_in_order_and_ecloses_a_callow_nurse_at_the_entrance() {
        let mut world = base_world();
        let entrance = Vec2::new(30.0, -10.0);
        world.insert_resource(NestGeometry {
            entrance,
            entrance_radius: 2.0,
            refuse: Vec2::ZERO,
        });
        world.insert_resource(NestStore::with_food(100.0));

        let entity = world
            .spawn((
                Brood {
                    stage: BroodStage::Egg,
                    timer: 0.0,
                    spawn_index: 11,
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();

        // Egg -> larva.
        for _ in 0..(EGG_DURATION / DT).ceil() as u32 {
            step(&mut world, DT);
            world.run_system_once(develop_brood).unwrap();
        }
        assert_eq!(world.get::<Brood>(entity).unwrap().stage, BroodStage::Larva);

        // Larva -> pupa.
        for _ in 0..(LARVA_DURATION / DT).ceil() as u32 {
            step(&mut world, DT);
            world.run_system_once(develop_brood).unwrap();
        }
        assert_eq!(world.get::<Brood>(entity).unwrap().stage, BroodStage::Pupa);

        // Pupa -> callow worker.
        for _ in 0..(PUPA_DURATION / DT).ceil() as u32 {
            step(&mut world, DT);
            world.run_system_once(develop_brood).unwrap();
        }
        assert!(world.get_entity(entity).is_err(), "the pupa must eclose");
        assert_eq!(world.resource::<AntPopulation>().count(), 1);

        let mut query = world.query::<(&Ant, &Transform)>();
        let (ant, transform) = query.single(&world).unwrap();
        assert_eq!(ant.phase, AntPhase::Nursing, "workers eclose as callows");
        assert_eq!(ant.age, 0.0);
        assert!(
            transform.translation.truncate().distance(entrance) <= 2.0 + 1e-3,
            "the worker must spawn at the nest entrance, got {:?}",
            transform.translation
        );
    }

    #[test]
    fn larvae_pay_food_and_stall_when_the_store_is_empty() {
        let mut world = base_world();
        world.insert_resource(NestStore::with_food(0.0));

        let entity = world
            .spawn((
                Brood {
                    stage: BroodStage::Larva,
                    timer: 0.0,
                    spawn_index: 1,
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();

        // One second with an empty store: no progress.
        for _ in 0..64 {
            step(&mut world, DT);
            world.run_system_once(develop_brood).unwrap();
        }
        assert_eq!(
            world.get::<Brood>(entity).unwrap().timer,
            0.0,
            "an unfed larva must not develop"
        );

        // One second of food: the timer advances and the store pays for it.
        world.insert_resource(NestStore::with_food(1.0));
        for _ in 0..64 {
            step(&mut world, DT);
            world.run_system_once(develop_brood).unwrap();
        }
        let brood = world.get::<Brood>(entity).unwrap();
        assert!(
            (brood.timer - 1.0).abs() < 0.02,
            "a fed larva advances in real time, got {}",
            brood.timer
        );
        let store = world.resource::<NestStore>().food();
        assert!(
            (store - (1.0 - LARVA_FOOD_PER_SEC)).abs() < 0.01,
            "the larva must pay {LARVA_FOOD_PER_SEC}/s, store is {store}"
        );
    }

    #[test]
    fn nursing_ants_tend_brood_but_foragers_do_not() {
        let mut world = base_world();
        world.insert_resource(NestStore::with_food(10.0));

        let tended = world
            .spawn((
                Brood {
                    stage: BroodStage::Egg,
                    timer: 0.0,
                    spawn_index: 1,
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        let alone = world
            .spawn((
                Brood {
                    stage: BroodStage::Egg,
                    timer: 0.0,
                    spawn_index: 2,
                },
                Transform::from_xyz(100.0, 0.0, 0.0),
            ))
            .id();
        let foraged = world
            .spawn((
                Brood {
                    stage: BroodStage::Egg,
                    timer: 0.0,
                    spawn_index: 3,
                },
                Transform::from_xyz(-100.0, 0.0, 0.0),
            ))
            .id();

        // A nurse next to the first egg, a forager next to the third.
        let mut nurse = Ant::test_ant(0.0);
        nurse.phase = AntPhase::Nursing;
        world.spawn((nurse, Transform::from_xyz(2.0, 0.0, 0.0)));
        world.spawn((Ant::test_ant(0.0), Transform::from_xyz(-98.0, 0.0, 0.0)));

        for _ in 0..64 {
            step(&mut world, DT);
            world.run_system_once(develop_brood).unwrap();
        }

        let tended_timer = world.get::<Brood>(tended).unwrap().timer;
        let alone_timer = world.get::<Brood>(alone).unwrap().timer;
        let foraged_timer = world.get::<Brood>(foraged).unwrap().timer;

        assert!(
            (alone_timer - 1.0).abs() < 0.02,
            "untended brood advances at the base rate, got {alone_timer}"
        );
        assert!(
            (tended_timer - (1.0 + BROOD_TEND_BONUS)).abs() < 0.03,
            "tended brood must advance at 1 + {BROOD_TEND_BONUS}, got {tended_timer}"
        );
        assert!(
            (foraged_timer - alone_timer).abs() < 1e-6,
            "a forager must not tend brood"
        );
    }

    #[test]
    fn eclosion_waits_at_the_population_cap_and_keeps_the_count_exact() {
        let mut world = base_world();
        world.insert_resource(NestStore::with_food(10.0));
        world.resource_mut::<AntPopulation>().add(MAX_ANTS);

        let entity = world
            .spawn((
                Brood {
                    stage: BroodStage::Pupa,
                    timer: PUPA_DURATION,
                    spawn_index: 1,
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();

        step(&mut world, DT);
        world.run_system_once(develop_brood).unwrap();
        assert!(
            world.get_entity(entity).is_ok(),
            "a completed pupa must wait at the population cap"
        );
        assert_eq!(world.resource::<AntPopulation>().count(), MAX_ANTS);

        // Room appears: the pupa ecloses and the count stays exact.
        world.resource_mut::<AntPopulation>().remove(1);
        step(&mut world, DT);
        world.run_system_once(develop_brood).unwrap();
        assert!(world.get_entity(entity).is_err(), "the pupa must eclose");
        assert_eq!(world.resource::<AntPopulation>().count(), MAX_ANTS);
    }

    #[test]
    fn founding_colony_spawns_nurses_and_brood_with_exact_population() {
        let mut world = base_world();
        world.init_resource::<Queen>();

        world.run_system_once(spawn_founding_colony).unwrap();

        let mut ant_query = world.query::<&Ant>();
        let ants = ant_query.iter(&world).count();
        let mut brood_query = world.query::<&Brood>();
        let brood = brood_query.iter(&world).count();

        assert_eq!(ants, INITIAL_NURSES, "the founding workforce");
        assert_eq!(brood, INITIAL_BROOD, "the founding brood batch");
        assert_eq!(world.resource::<AntPopulation>().count(), INITIAL_NURSES);

        for ant in ant_query.iter(&world) {
            assert_eq!(
                ant.phase,
                AntPhase::Nursing,
                "the founding cohort must start as callow nurses"
            );
        }
    }

    /// End-to-end: an empty colony with a stocked store grows adults only
    /// through the queen -> egg -> larva -> pupa pipeline.
    #[test]
    fn brood_pipeline_turns_eggs_into_adults_end_to_end() {
        use bevy::time::TimeUpdateStrategy;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                DT,
            )))
            .add_systems(
                FixedUpdate,
                (crate::simulation::ant::queen_lays_eggs, develop_brood).chain(),
            );

        crate::simulation::register_sim_resources(&mut app);
        app.init_resource::<Queen>()
            .insert_resource(NestStore::with_food(NEST_STORE_CAP))
            .insert_resource(crate::simulation::ant::AntSpawner {
                timer: Timer::from_seconds(0.05, TimerMode::Repeating),
            });

        // One full egg + larva lifetime: brood exists, adults must not.
        for _ in 0..((EGG_DURATION + LARVA_DURATION) * 64.0) as u32 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<AntPopulation>().count(),
            0,
            "adults must not appear before pupae eclose"
        );
        {
            let world = app.world_mut();
            let mut query = world.query::<&Brood>();
            assert!(
                query.iter(world).count() > 0,
                "the queen must have laid brood"
            );
        }

        // Complete the pupa stage: callow workers appear and the count matches.
        for _ in 0..((PUPA_DURATION + 2.0) * 64.0) as u32 {
            app.update();
        }
        let population = app.world().resource::<AntPopulation>().count();
        let world = app.world_mut();
        let mut query = world.query::<&Ant>();
        let ants = query.iter(world).count();
        assert!(population > 0, "the pipeline must produce adults");
        assert_eq!(ants, population, "AntPopulation must stay exact");
    }

    #[test]
    fn eclosion_uses_the_stream_index_stored_at_laying() {
        let mut eclosed = base_world();
        eclosed.insert_resource(NestStore::with_food(10.0));
        eclosed.spawn((
            Brood {
                stage: BroodStage::Pupa,
                timer: PUPA_DURATION,
                spawn_index: 123,
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));

        step(&mut eclosed, DT);
        eclosed.run_system_once(develop_brood).unwrap();

        let mut direct = World::new();
        direct
            .run_system_once(move |mut commands: Commands| {
                crate::simulation::ant::spawn_worker(
                    &mut commands,
                    123,
                    Vec2::ZERO,
                    0.0,
                    Vec2::ZERO,
                );
            })
            .unwrap();

        let mut eclosed_query = eclosed.query::<&Ant>();
        let eclosed_ant = eclosed_query.single(&eclosed).unwrap();
        let mut direct_query = direct.query::<&Ant>();
        let direct_ant = direct_query.single(&direct).unwrap();

        assert_eq!(eclosed_ant.pi_bias.to_bits(), direct_ant.pi_bias.to_bits());
        assert_eq!(
            eclosed_ant.max_lifetime.to_bits(),
            direct_ant.max_lifetime.to_bits()
        );
        assert_eq!(
            eclosed_ant.base_speed.to_bits(),
            direct_ant.base_speed.to_bits()
        );
        assert_eq!(
            eclosed_ant.sensor_gain.to_bits(),
            direct_ant.sensor_gain.to_bits()
        );
        assert_eq!(
            eclosed_ant.forage_threshold.to_bits(),
            direct_ant.forage_threshold.to_bits()
        );
        assert_eq!(
            eclosed_ant.crop_capacity.to_bits(),
            direct_ant.crop_capacity.to_bits()
        );
        assert_eq!(
            eclosed_ant.explore_tendency.to_bits(),
            direct_ant.explore_tendency.to_bits()
        );
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

        // The dropped corpse is disposed at the pile: further ticks of the
        // full pickup/carry pair must not re-pick or re-drop it.
        for _ in 0..(64 * 10) {
            step(&mut world, DT);
            world.run_system_once(pick_up_corpses).unwrap();
            world.run_system_once(carry_corpses).unwrap();
        }

        assert!(
            !world.get::<Ant>(ant_entity).unwrap().carrying_corpse,
            "the disposed corpse must never be picked up again"
        );
        assert_eq!(
            world.resource::<ColonyStats>().refuse,
            1,
            "the pick-up/drop loop must not inflate the refuse counter"
        );
        assert_eq!(corpse_positions(&mut world).len(), 1);
    }

    /// A corpse sitting at the refuse pile is disposed; an ant standing on it
    /// must not start the endless pick-up/drop loop that inflated
    /// `ColonyStats::refuse` in the GUI run.
    #[test]
    fn corpses_at_the_refuse_pile_are_never_re_picked() {
        let mut world = base_world();
        let refuse = Vec2::new(50.0, 0.0);
        world.insert_resource(NestGeometry {
            entrance: Vec2::ZERO,
            entrance_radius: NEST_RADIUS,
            refuse,
        });

        world.spawn((
            Corpse { ttl: CORPSE_TTL },
            Transform::from_xyz(refuse.x, refuse.y, 0.0),
        ));
        let ant_entity = world
            .spawn((
                Ant::test_ant(0.0),
                Transform::from_xyz(refuse.x, refuse.y, 0.0),
            ))
            .id();

        for _ in 0..(64 * 10) {
            step(&mut world, DT);
            world.run_system_once(pick_up_corpses).unwrap();
            world.run_system_once(carry_corpses).unwrap();
        }

        assert!(
            !world.get::<Ant>(ant_entity).unwrap().carrying_corpse,
            "an ant on the refuse pile must not pick a disposed corpse up"
        );
        assert_eq!(
            world.resource::<ColonyStats>().refuse,
            0,
            "a disposed corpse must never count as a refuse delivery"
        );
        assert_eq!(
            corpse_positions(&mut world).len(),
            1,
            "the corpse must stay where it is"
        );
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
    /// Runs the real chain (`build_app`, which registers environment, nest and
    /// lifecycle exactly once) at a fixed population cap and compares the full
    /// lifecycle against a variant with corpses culled every tick, which
    /// isolates the corpse-handling cost from the rest of the chain.
    ///
    /// The old `none` variant is gone:
    /// [`crate::simulation::SimulationPlugin::add_fixed_step_systems`] always
    /// registers the lifecycle stream, and this module cannot unregister it.
    ///
    /// Env knobs: `PERF120_SECS` (default 120), `PERF120_CAP` (default 10000)
    /// and `PERF120_VARIANTS` (comma-separated subset of `full,cull`).
    #[test]
    #[ignore = "120 s analysis; run manually"]
    fn perf120_analysis() {
        use crate::simulation::ant::AntSpawner;
        use crate::simulation::trail_tests::{FoodSetup, build_app};
        use std::time::Instant;

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Variant {
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

            if variant == Variant::Cull {
                app.add_systems(
                    FixedUpdate,
                    cull_corpses
                        .before(pick_up_corpses)
                        .in_set(SimSet::Lifecycle),
                );
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
        let variants = std::env::var("PERF120_VARIANTS").unwrap_or_else(|_| "full,cull".into());

        for variant in variants.split(',') {
            match variant.trim() {
                "full" => run("full", Variant::Full, cap, secs),
                "cull" => run("no-corpses", Variant::Cull, cap, secs),
                other => panic!("unknown PERF120_VARIANTS entry {other:?} (expected full or cull)"),
            }
        }
    }

    /// Integration harness for the frozen trail gate: runs the real emergence
    /// scenario through the real fixed-step chain (founding colony + brood
    /// pipeline + mortality + rest) and checks the colony still reaches the
    /// delivery floor and forms the corridor.
    ///
    /// The chain is registered exactly once by `build_app`
    /// ([`crate::simulation::SimulationPlugin::add_fixed_step_systems`]), so
    /// this harness must not call [`register`] again. Ignored because it
    /// duplicates the 45 s regression.
    #[test]
    #[ignore = "trail-gate harness with the full brood pipeline; run manually"]
    fn brood_pipeline_keeps_the_trail_gate_green() {
        use crate::constants::world::{NEST_X, NEST_Y};
        use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
        use crate::simulation::trail_tests::{FoodSetup, build_app, corridor_avg, run_seconds};

        let food_x = NEST_X + 450.0;
        let mut app = build_app(FoodSetup::Custom(Vec2::new(food_x, NEST_Y)));
        run_seconds(&mut app, 45, Some(800));

        let deliveries = app.world().resource::<ColonyStats>().total_food_delivered;
        let deaths = app.world().resource::<ColonyStats>().deaths;
        let population = app.world().resource::<AntPopulation>().count();
        let brood = {
            let world = app.world_mut();
            let mut query = world.query::<&Brood>();
            query.iter(world).count()
        };
        let grid = app.world().resource::<PheromoneGrid>();
        let corridor = corridor_avg(grid, PheromoneKind::ToFood, NEST_Y, food_x);

        println!(
            "deliveries={deliveries} deaths={deaths} population={population} brood={brood} \
             corridor={corridor}"
        );

        assert!(
            deliveries >= 10.0,
            "expected at least 10 deliveries with the brood pipeline, got {deliveries}"
        );
        assert!(
            corridor >= 0.05,
            "expected a ToFood corridor with the brood pipeline, got {corridor}"
        );
    }
}
