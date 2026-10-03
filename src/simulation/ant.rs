//! Ant component, population bookkeeping and the energy/age lifecycle.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::f32::consts::PI;

use crate::constants::ant::*;
use crate::constants::colony::{
    CROP_CAPACITY_MAX, CROP_CAPACITY_MIN, ENERGY_FULL_EPS, ENERGY_REFILL_COST, ENERGY_REFILL_RATE,
    EXPLORE_TENDENCY_MAX, EXPLORE_TENDENCY_MIN, FORAGE_THRESHOLD_MAX, FORAGE_THRESHOLD_MIN,
    NURSE_UPKEEP_PER_ANT, PI_BIAS_MAX_DEG, PI_BIAS_MIN_DEG, RECRUIT_THRESHOLD, SENSOR_GAIN_MAX,
    SENSOR_GAIN_MIN,
};
use crate::constants::world::NEST_RADIUS;
use crate::core::layers::Z_ANT;
use crate::simulation::NestPosition;
use crate::simulation::colony::{ColonyStats, NestStore};
use crate::simulation::lifecycle::spawn_corpse;
use crate::simulation::nest::NestGeometry;

/// Behavioral phase of an ant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AntPhase {
    /// First quarter of life: stays around the nest, neither forages nor
    /// deposits pheromones.
    #[default]
    Nursing,
    /// Searches for food, follows the `ToFood` trail and deposits `ToNest`.
    Foraging,
    /// Low on energy: returns home using path integration.
    Returning,
}

#[derive(Component)]
pub struct Ant {
    pub direction: f32,
    pub has_food: bool,
    /// Amount of food currently carried, set at pickup and cleared at
    /// dropoff. Zero whenever `has_food` is false.
    pub carrying: f32,
    /// Nest position when last visited, used for path integration.
    pub home: Vec2,
    pub age: f32,
    pub max_lifetime: f32,
    /// 1.0 is full; drains while moving and refills inside the nest.
    pub energy: f32,
    pub phase: AntPhase,
    /// Seconds left standing still after a pickup or dropoff.
    pub handling_timer: f32,
    /// Speed before situational factors (carrying, crowding, nursing).
    pub base_speed: f32,
    /// Speed actually used for the last movement step.
    pub speed: f32,
    /// Completed food deliveries; boosts pheromone deposits.
    pub trips_completed: u32,
    /// Nest-relative vector to the last food pickup (`None` until discovery).
    /// Owner: navigation stream.
    pub route_memory: Option<Vec2>,
    /// Fixed path-integration angular error in radians. Owner: navigation.
    pub pi_bias: f32,
    /// Path-integration drift accumulated since the last nest visit, radians.
    /// Owner: navigation.
    pub pi_drift: f32,
    /// Seconds since a trail was last sensed. Owner: navigation.
    pub lost_time: f32,
    /// Position at the start of the last movement step, so deposit can
    /// reconstruct the real segment even after a wall bounce rewrote the
    /// heading. Owner: pheromone stream.
    pub prev_pos: Vec2,
    /// Task-allocation response threshold. Owner: colony stream.
    pub forage_threshold: f32,
    /// Individual exploration multiplier. Owner: navigation/colony streams.
    pub explore_tendency: f32,
    /// Individual sensor gain. Owner: navigation stream.
    pub sensor_gain: f32,
    /// Individual crop capacity. Owner: colony stream.
    pub crop_capacity: f32,
    /// Quality of the currently carried load. Owner: pheromone stream.
    pub carrying_quality: f32,
    /// Seconds left resting after a delivery. Owner: colony stream.
    pub rest_timer: f32,
    /// True while the ant drags a corpse to the refuse pile. The necrophoresis
    /// pass owns the ant until then: movement, collision and deposit skip it
    /// because [`Ant::tick_handling`] holds `handling_timer` while this is set.
    pub carrying_corpse: bool,
}

impl Ant {
    pub fn is_handling(&self) -> bool {
        self.handling_timer > 0.0
    }

    /// Whether the ant is carrying food (`carrying > 0.0`).
    ///
    /// The single source of truth for "carrying food": the biology stream will
    /// delete the legacy `has_food` flag in favour of this check.
    pub fn is_laden(&self) -> bool {
        self.carrying > 0.0
    }

    /// Stand still for [`HANDLING_TIME`] after a pickup or dropoff.
    pub fn start_handling(&mut self) {
        self.handling_timer = HANDLING_TIME;
    }

    /// Count down the handling pause.
    ///
    /// Held while the ant carries a corpse: [`crate::simulation::lifecycle`]
    /// keeps the ant "busy" for the whole trip to the refuse pile so movement,
    /// collision and deposit leave it alone, and releases it on dropoff.
    pub fn tick_handling(&mut self, dt: f32) {
        if self.carrying_corpse {
            return;
        }

        if self.handling_timer > 0.0 {
            self.handling_timer = (self.handling_timer - dt).max(0.0);
        }
    }

    /// Count down the post-delivery rest timer.
    pub fn tick_rest(&mut self, dt: f32) {
        if self.rest_timer > 0.0 {
            self.rest_timer = (self.rest_timer - dt).max(0.0);
        }
    }

    /// Load `amount` of food of the given `quality` into the crop.
    ///
    /// `has_food` mirrors `carrying > 0` for the legacy consumers; both are
    /// written here and in [`Ant::deliver`] so they can never disagree.
    pub fn pick_up(&mut self, amount: f32, quality: f32) {
        self.carrying = amount.max(0.0);
        self.has_food = self.carrying > 0.0;
        self.carrying_quality = quality;

        if self.has_food {
            self.start_handling();
        }
    }

    /// Complete a dropoff: clear the carry, count the trip and resume
    /// foraging. Returns the amount actually delivered.
    pub fn deliver(&mut self) -> f32 {
        let delivered = self.carrying.max(0.0);
        self.carrying = 0.0;
        self.has_food = false;
        self.carrying_quality = 1.0;
        self.phase = AntPhase::Foraging;
        self.trips_completed = self.trips_completed.saturating_add(1);
        self.start_handling();
        delivered
    }

    /// Foraging → Returning: the tank is low, head home.
    pub fn begin_returning(&mut self) {
        if self.phase == AntPhase::Foraging {
            self.phase = AntPhase::Returning;
        }
    }

    /// Returning → Foraging: home with a full tank.
    pub fn arrive_home(&mut self) {
        if self.phase == AntPhase::Returning {
            self.phase = AntPhase::Foraging;
        }
    }

    /// Nursing → Foraging: age or colony demand matured the ant.
    pub fn mature(&mut self) {
        if self.phase == AntPhase::Nursing {
            self.phase = AntPhase::Foraging;
        }
    }

    /// Foraging → Nursing: demand-driven reversion of an empty forager (F7).
    pub fn revert_to_nursing(&mut self) {
        if self.phase == AntPhase::Foraging && !self.is_laden() {
            self.phase = AntPhase::Nursing;
        }
    }

    /// Drain (or refill) energy. Returns `true` when the ant starves.
    ///
    /// Inside the nest the ant refills at [`ENERGY_REFILL_RATE`] while `store`
    /// holds food, paying [`ENERGY_REFILL_COST`] food per energy unit. The
    /// refill is a per-second rate, so behavior is tick-rate independent.
    /// With an empty store an ant in the nest drains like anywhere else and
    /// can starve. An [`AntPhase::Returning`] ant is considered home once its
    /// tank is full.
    pub fn tick_energy(&mut self, dt: f32, in_nest: bool, store: &mut NestStore) -> bool {
        if in_nest && store.food() > 0.0 {
            let deficit = (1.0 - self.energy).max(0.0);

            if deficit > 0.0 {
                let requested = (ENERGY_REFILL_RATE * dt).min(deficit);
                let spent = store.spend(requested * ENERGY_REFILL_COST);
                self.energy = (self.energy + spent / ENERGY_REFILL_COST).min(1.0);
            }

            if self.energy >= 1.0 - ENERGY_FULL_EPS {
                self.energy = 1.0;
                self.arrive_home();
            }

            return false;
        }

        // Graded load penalty: an empty crop drains at 1.0, a full crop at
        // ANT_CARRY_ENERGY_DRAIN_FACTOR. Partial loads (crop-limited pickups)
        // cost proportionally less.
        let load_fraction = if self.crop_capacity > 0.0 {
            (self.carrying / self.crop_capacity).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let drain_factor = 1.0 + (ANT_CARRY_ENERGY_DRAIN_FACTOR - 1.0) * load_fraction;
        self.energy -= ANT_ENERGY_DRAIN_RATE * drain_factor * dt;

        if self.energy <= 0.0 {
            return true;
        }

        if self.energy < ANT_ENERGY_RETURN_THRESHOLD {
            self.begin_returning();
        }

        false
    }

    /// Whether this ant follows the `ToNest` channel (laden or low energy).
    pub fn follows_to_nest(&self) -> bool {
        self.has_food || self.phase == AntPhase::Returning
    }

    /// Whether the ant has grown out of the nursing phase for its age.
    pub fn nursing_over(&self) -> bool {
        self.age >= self.max_lifetime * ANT_NURSING_LIFETIME_FRACTION
    }

    #[cfg(test)]
    pub(crate) fn test_ant(direction: f32) -> Self {
        Self {
            direction,
            has_food: false,
            carrying: 0.0,
            home: Vec2::ZERO,
            age: 0.0,
            max_lifetime: ANT_LIFETIME,
            energy: 1.0,
            phase: AntPhase::Foraging,
            handling_timer: 0.0,
            base_speed: ANT_SPEED,
            speed: 0.0,
            trips_completed: 0,
            route_memory: None,
            pi_bias: 0.0,
            pi_drift: 0.0,
            lost_time: 0.0,
            prev_pos: Vec2::ZERO,
            forage_threshold: 0.5,
            explore_tendency: 1.0,
            sensor_gain: 1.0,
            crop_capacity: 1.0,
            carrying_quality: 1.0,
            rest_timer: 0.0,
            carrying_corpse: false,
        }
    }
}

/// Current number of live ants, decremented when ants despawn.
///
/// The count is private: reads go through [`Self::count`], mutations through
/// [`Self::add`] and [`Self::remove`], so the population resource cannot be
/// corrupted from outside this module.
#[derive(Resource, Default)]
pub struct AntPopulation(usize);

impl AntPopulation {
    /// Number of live ants.
    pub fn count(&self) -> usize {
        self.0
    }

    /// Record `n` new ants, saturating at `usize::MAX`.
    ///
    /// Together with [`Self::remove`] these are the only mutation entry points.
    pub fn add(&mut self, n: usize) {
        self.0 = self.0.saturating_add(n);
    }

    /// Record `n` removed ants, saturating at zero.
    pub fn remove(&mut self, n: usize) {
        self.0 = self.0.saturating_sub(n);
    }
}

/// Deterministic per-ant random source for steering and wandering.
///
/// Keeping the RNG on the ant (rather than using the thread-local generator)
/// makes a seeded run reproducible no matter which executor thread steps the
/// ant, and keeps the ants independent of each other's draw order.
#[derive(Component)]
pub struct AntRng(pub(crate) fastrand::Rng);

impl AntRng {
    /// Deterministic stream for the `n`-th ant ever spawned.
    pub(crate) fn for_spawn(n: u64) -> Self {
        let seed = n.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(31) ^ 0xA5A5_5A5A_5A5A_5A5A;
        Self(fastrand::Rng::with_seed(seed))
    }
}

#[derive(Resource)]
pub struct AntSpawner {
    pub timer: Timer,
}

/// Batch size for one spawn interval: the population ramp and delivery boost
/// scaled by the store gate `clamp(store / RECRUIT_THRESHOLD, 0, 1)`, clamped
/// to the remaining capacity. There is deliberately no `.max(1)` floor: with
/// an empty store the colony stops recruiting and the population shrinks.
pub fn recruitment_batch(
    ramp: f32,
    delivery_boost: f32,
    store_food: f32,
    capacity: usize,
) -> usize {
    let gate = (store_food / RECRUIT_THRESHOLD).clamp(0.0, 1.0);
    ((ANT_BATCH_SIZE as f32 * ramp * delivery_boost * gate) as usize).min(capacity)
}

/// Read-only world inputs of the spawn ramp, bundled to keep the system
/// signature small.
#[derive(SystemParam)]
pub struct SpawnInputs<'w> {
    colony: Res<'w, ColonyStats>,
    nest_store: Res<'w, NestStore>,
    nest_position: Res<'w, NestPosition>,
    nest_geometry: Res<'w, NestGeometry>,
}

/// Spawn new nurses at a ramped rate: slower near the population cap, faster
/// after recent food deliveries, and only while the nest store can pay for
/// them. [`MAX_ANTS`] stays a hard safety cap.
pub fn spawn_ants(
    mut commands: Commands,
    mut spawner: ResMut<AntSpawner>,
    mut population: ResMut<AntPopulation>,
    inputs: SpawnInputs,
    time: Res<Time<Fixed>>,
    mut spawn_counter: Local<u64>,
) {
    spawner.timer.tick(time.delta());

    if !spawner.timer.just_finished() {
        return;
    }

    let capacity = MAX_ANTS.saturating_sub(population.count());
    if capacity == 0 {
        return;
    }

    let nest_pos = inputs.nest_position.0;
    let entrance = inputs.nest_geometry.entrance;
    let entrance_radius = inputs.nest_geometry.entrance_radius.max(0.0);
    let ramp = (1.0 - population.count() as f32 / MAX_ANTS as f32).clamp(0.0, 1.0);
    let batch_size = recruitment_batch(
        ramp,
        inputs.colony.delivery_boost(),
        inputs.nest_store.food(),
        capacity,
    );

    for _ in 0..batch_size {
        let spawn_index = spawn_counter.wrapping_add(1);
        *spawn_counter = spawn_index;

        // Every draw for this ant comes from its own seeded stream, so the
        // whole spawn is reproducible.
        let mut rng = AntRng::for_spawn(spawn_index).0;

        let heading = rng.f32() * 2.0 * PI;
        let spawn_angle = rng.f32() * 2.0 * PI;
        // sqrt keeps the spawn distribution uniform over the entrance disc.
        let spawn_radius = entrance_radius * rng.f32().sqrt();
        let spawn_pos = entrance + Vec2::new(spawn_angle.cos(), spawn_angle.sin()) * spawn_radius;
        let max_lifetime = ANT_LIFETIME * (ANT_LIFETIME_VARIATION_MIN + rng.f32());
        let base_speed = ANT_SPEED * (ANT_SPEED_VARIATION_MIN + rng.f32());

        // Individual variation (F7), drawn from the same seeded stream. The
        // ranges are documented in `constants/colony.rs`:
        //   pi_bias          ±(5..10) degrees, fixed path-integration error
        //   explore_tendency 0.5..1.5, scales the exploration chance
        //   sensor_gain      0.7..1.3, scales sensor readings
        //   forage_threshold 0.3..0.9, task-allocation response threshold
        //   crop_capacity    0.8..1.2, load a full crop can hold
        let pi_bias_sign = if rng.f32() < 0.5 { -1.0 } else { 1.0 };
        let pi_bias_deg = PI_BIAS_MIN_DEG + rng.f32() * (PI_BIAS_MAX_DEG - PI_BIAS_MIN_DEG);
        let pi_bias = pi_bias_sign * pi_bias_deg.to_radians();
        let explore_tendency =
            EXPLORE_TENDENCY_MIN + rng.f32() * (EXPLORE_TENDENCY_MAX - EXPLORE_TENDENCY_MIN);
        let sensor_gain = SENSOR_GAIN_MIN + rng.f32() * (SENSOR_GAIN_MAX - SENSOR_GAIN_MIN);
        let forage_threshold =
            FORAGE_THRESHOLD_MIN + rng.f32() * (FORAGE_THRESHOLD_MAX - FORAGE_THRESHOLD_MIN);
        let crop_capacity = CROP_CAPACITY_MIN + rng.f32() * (CROP_CAPACITY_MAX - CROP_CAPACITY_MIN);

        commands.spawn((
            Ant {
                direction: heading,
                has_food: false,
                carrying: 0.0,
                home: nest_pos,
                age: 0.0,
                max_lifetime,
                energy: 1.0,
                phase: AntPhase::Nursing,
                handling_timer: 0.0,
                base_speed,
                speed: 0.0,
                trips_completed: 0,
                route_memory: None,
                pi_bias,
                pi_drift: 0.0,
                lost_time: 0.0,
                prev_pos: spawn_pos,
                forage_threshold,
                explore_tendency,
                sensor_gain,
                crop_capacity,
                carrying_quality: 1.0,
                rest_timer: 0.0,
                carrying_corpse: false,
            },
            AntRng(rng),
            Sprite {
                color: Color::srgba(1.0, 1.0, 1.0, ANT_ALPHA),
                custom_size: Some(Vec2::new(ANT_SIZE, ANT_SIZE)),
                ..default()
            },
            Transform::from_xyz(spawn_pos.x, spawn_pos.y, Z_ANT),
        ));
    }

    population.add(batch_size);
}

/// Advance age, handling pauses, rest and energy; replace ants that die of old
/// age or exhaustion with a [`crate::simulation::lifecycle::Corpse`], and
/// refresh the home vector inside the nest.
///
/// Also drains the colony's nursing upkeep: every nursing ant consumes
/// [`NURSE_UPKEEP_PER_ANT`] food per second from the store, so a colony
/// without income shrinks even before its foragers starve.
pub fn update_ant_energy_age(
    mut commands: Commands,
    mut ant_query: Query<(Entity, &mut Ant, &Transform)>,
    mut population: ResMut<AntPopulation>,
    mut nest_store: ResMut<NestStore>,
    mut colony: ResMut<ColonyStats>,
    nest_position: Res<NestPosition>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();
    let nest_pos = nest_position.0;
    let nest_radius_squared = NEST_RADIUS * NEST_RADIUS;
    let mut nurses: u32 = 0;

    for (entity, mut ant, transform) in &mut ant_query {
        ant.tick_handling(dt);
        ant.tick_rest(dt);
        ant.age += dt;

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        let in_nest = pos.distance_squared(nest_pos) < nest_radius_squared;

        if ant.age >= ant.max_lifetime || ant.tick_energy(dt, in_nest, &mut nest_store) {
            spawn_corpse(&mut commands, pos);
            commands.entity(entity).despawn();
            population.remove(1);
            colony.record_death();
            continue;
        }

        if ant.phase == AntPhase::Nursing {
            nurses += 1;
        }

        // A corpse carrier keeps steering toward the refuse pile, so its home
        // anchor must not be reset by crossing the nest disc.
        if in_nest && !ant.carrying_corpse {
            ant.home = nest_pos;
            // The nest resets accumulated path-integration error (F3). The
            // fixed per-ant `pi_bias` deliberately survives: only the drift
            // since the last nest visit is cancelled.
            ant.pi_drift = 0.0;
        }

        if ant.phase == AntPhase::Nursing && ant.nursing_over() {
            ant.mature();
        }
    }

    if nurses > 0 {
        nest_store.spend(NURSE_UPKEEP_PER_ANT * nurses as f32 * dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::colony::NEST_STORE_CAP;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    const EPS: f32 = 1e-6;

    #[test]
    fn handling_pauses_and_resumes() {
        let mut ant = Ant::test_ant(0.0);
        assert!(!ant.is_handling());

        ant.start_handling();
        assert!(ant.is_handling());

        ant.tick_handling(HANDLING_TIME * 0.5);
        assert!(ant.is_handling());

        ant.tick_handling(HANDLING_TIME);
        assert!(!ant.is_handling());
        assert_eq!(ant.handling_timer, 0.0);
    }

    #[test]
    fn is_laden_tracks_the_carried_amount() {
        let mut ant = Ant::test_ant(0.0);
        assert!(!ant.is_laden());

        ant.carrying = 0.5;
        assert!(ant.is_laden());

        ant.carrying = 0.0;
        assert!(!ant.is_laden());
    }

    #[test]
    fn population_count_add_and_remove_saturate() {
        let mut population = AntPopulation::default();
        assert_eq!(population.count(), 0);

        population.add(3);
        assert_eq!(population.count(), 3);

        population.remove(2);
        assert_eq!(population.count(), 1);

        population.remove(10);
        assert_eq!(population.count(), 0, "removal saturates at zero");

        population.add(usize::MAX);
        assert_eq!(population.count(), usize::MAX);
        population.add(1);
        assert_eq!(population.count(), usize::MAX, "addition saturates");
    }

    #[test]
    fn energy_drain_forces_returning_then_starvation() {
        let mut ant = Ant::test_ant(0.0);
        let mut store = NestStore::with_food(0.0);
        let dt = 1.0 / 64.0;

        let mut ticks = 0;
        while ant.phase != AntPhase::Returning && ticks < 64 * 120 {
            assert!(!ant.tick_energy(dt, false, &mut store));
            ticks += 1;
        }

        assert_eq!(ant.phase, AntPhase::Returning);
        assert!(ant.energy < ANT_ENERGY_RETURN_THRESHOLD);

        let mut starved = false;
        for _ in 0..64 * 120 {
            if ant.tick_energy(dt, false, &mut store) {
                starved = true;
                break;
            }
        }
        assert!(starved);
    }

    #[test]
    fn nursing_ants_are_not_demoted_by_low_energy() {
        let mut ant = Ant::test_ant(0.0);
        ant.phase = AntPhase::Nursing;
        ant.energy = ANT_ENERGY_RETURN_THRESHOLD * 0.5;

        let mut store = NestStore::with_food(0.0);
        assert!(!ant.tick_energy(1.0, false, &mut store));
        assert_eq!(
            ant.phase,
            AntPhase::Nursing,
            "energy must not pull a nurse out of the nursing phase"
        );
    }

    #[test]
    fn carrying_drains_faster_and_nest_refills() {
        let mut laden = Ant::test_ant(0.0);
        laden.pick_up(1.0, 1.0);
        let mut walker = Ant::test_ant(0.0);
        let mut empty = NestStore::with_food(0.0);

        laden.tick_energy(1.0, false, &mut empty);
        walker.tick_energy(1.0, false, &mut empty);
        assert!(laden.energy < walker.energy);

        laden.phase = AntPhase::Returning;
        let mut stocked = NestStore::with_food(10.0);
        assert!(!laden.tick_energy(1.0, true, &mut stocked));
        assert!((laden.energy - 1.0).abs() < EPS);
        assert_eq!(laden.phase, AntPhase::Foraging);
        assert!(stocked.food() < 10.0, "the refill must be paid for");
    }

    #[test]
    fn carry_drain_is_graded_by_load_and_crop_capacity() {
        let mut empty = Ant::test_ant(0.0);
        let mut half = Ant::test_ant(0.0);
        half.pick_up(0.5, 1.0);
        let mut full = Ant::test_ant(0.0);
        full.pick_up(1.0, 1.0);
        let mut store = NestStore::with_food(0.0);

        empty.tick_energy(1.0, false, &mut store);
        half.tick_energy(1.0, false, &mut store);
        full.tick_energy(1.0, false, &mut store);

        let empty_drain = 1.0 - empty.energy;
        let half_drain = 1.0 - half.energy;
        let full_drain = 1.0 - full.energy;

        assert!((empty_drain - ANT_ENERGY_DRAIN_RATE).abs() < EPS);
        assert!(
            empty_drain < half_drain && half_drain < full_drain,
            "drain must grow with the load: {empty_drain} {half_drain} {full_drain}"
        );

        let expected_half =
            ANT_ENERGY_DRAIN_RATE * (1.0 + (ANT_CARRY_ENERGY_DRAIN_FACTOR - 1.0) * 0.5);
        assert!((half_drain - expected_half).abs() < EPS);
        assert!((full_drain - ANT_ENERGY_DRAIN_RATE * ANT_CARRY_ENERGY_DRAIN_FACTOR).abs() < EPS);

        // The same load on a bigger crop is a smaller fraction of the crop.
        let mut big_crop = Ant::test_ant(0.0);
        big_crop.crop_capacity = 2.0;
        big_crop.pick_up(1.0, 1.0);
        big_crop.tick_energy(1.0, false, &mut store);
        let big_crop_drain = 1.0 - big_crop.energy;
        assert!((big_crop_drain - expected_half).abs() < EPS);
    }

    #[test]
    fn pick_up_and_deliver_keep_the_carry_contract() {
        let mut ant = Ant::test_ant(0.0);
        ant.pick_up(0.75, 1.5);
        assert!(ant.is_laden());
        assert!(ant.has_food, "the legacy flag must mirror the carry");
        assert!((ant.carrying - 0.75).abs() < EPS);
        assert!((ant.carrying_quality - 1.5).abs() < EPS);
        assert!(ant.is_handling());

        let delivered = ant.deliver();
        assert!((delivered - 0.75).abs() < EPS);
        assert!(!ant.is_laden());
        assert!(!ant.has_food);
        assert_eq!(ant.carrying, 0.0);
        assert!((ant.carrying_quality - 1.0).abs() < EPS);
        assert_eq!(ant.phase, AntPhase::Foraging);
        assert_eq!(ant.trips_completed, 1);

        // A zero-amount pickup must not flag a carry.
        let mut empty = Ant::test_ant(0.0);
        empty.pick_up(0.0, 1.0);
        assert!(!empty.is_laden());
        assert!(!empty.has_food);
    }

    #[test]
    fn rest_timer_ticks_down_to_zero() {
        let mut ant = Ant::test_ant(0.0);
        ant.rest_timer = 4.0;

        ant.tick_rest(1.0);
        assert!((ant.rest_timer - 3.0).abs() < EPS);

        ant.tick_rest(10.0);
        assert_eq!(ant.rest_timer, 0.0, "the timer clamps at zero");
    }

    #[test]
    fn phase_transition_methods_are_guarded() {
        let mut ant = Ant::test_ant(0.0);

        ant.phase = AntPhase::Nursing;
        ant.begin_returning();
        assert_eq!(ant.phase, AntPhase::Nursing, "nurses do not return");
        ant.mature();
        assert_eq!(ant.phase, AntPhase::Foraging);

        ant.begin_returning();
        assert_eq!(ant.phase, AntPhase::Returning);
        ant.arrive_home();
        assert_eq!(ant.phase, AntPhase::Foraging);

        // A laden forager must not revert to nursing.
        ant.pick_up(1.0, 1.0);
        ant.revert_to_nursing();
        assert_eq!(ant.phase, AntPhase::Foraging);
        ant.deliver();
        ant.revert_to_nursing();
        assert_eq!(ant.phase, AntPhase::Nursing);
    }

    #[test]
    fn nest_refill_consumes_store_and_stalls_when_empty() {
        let dt = 1.0 / 64.0;
        let mut ant = Ant::test_ant(0.0);
        ant.energy = 0.2;
        let mut store = NestStore::with_food(10.0);

        // One second of refill: +0.5 energy for 0.05 food.
        for _ in 0..64 {
            assert!(!ant.tick_energy(dt, true, &mut store));
        }
        assert!((ant.energy - 0.7).abs() < 1e-4, "energy {}", ant.energy);
        assert!((store.food() - 9.95).abs() < 1e-4, "store {}", store.food());

        // Keep refilling to a full tank.
        for _ in 0..64 {
            ant.tick_energy(dt, true, &mut store);
        }
        assert!((ant.energy - 1.0).abs() < EPS);
        assert!((store.food() - 9.92).abs() < 1e-3, "store {}", store.food());

        // Empty store: no refill, the ant drains again even in the nest.
        store = NestStore::with_food(0.0);
        ant.energy = 0.5;
        for _ in 0..64 {
            ant.tick_energy(dt, true, &mut store);
        }
        assert!(ant.energy < 0.5, "empty store must not refill");
        assert!(ant.energy > 0.49);
    }

    #[test]
    fn nest_refill_is_frame_rate_independent() {
        let mut fine = Ant::test_ant(0.0);
        fine.energy = 0.2;
        let mut fine_store = NestStore::with_food(10.0);
        for _ in 0..64 {
            fine.tick_energy(1.0 / 64.0, true, &mut fine_store);
        }

        let mut coarse = Ant::test_ant(0.0);
        coarse.energy = 0.2;
        let mut coarse_store = NestStore::with_food(10.0);
        coarse.tick_energy(1.0, true, &mut coarse_store);

        assert!((fine.energy - coarse.energy).abs() < 1e-4);
        assert!((fine_store.food() - coarse_store.food()).abs() < 1e-4);
    }

    #[test]
    fn nursing_ends_after_the_configured_fraction_of_life() {
        let mut ant = Ant::test_ant(0.0);
        ant.phase = AntPhase::Nursing;
        ant.max_lifetime = 40.0;

        let threshold = 40.0 * ANT_NURSING_LIFETIME_FRACTION;
        ant.age = threshold - 0.01;
        assert!(!ant.nursing_over());

        ant.age = threshold;
        assert!(ant.nursing_over());
    }

    #[test]
    fn recruitment_gate_scales_with_the_store_and_stops_when_empty() {
        let capacity = MAX_ANTS;

        assert_eq!(
            recruitment_batch(1.0, 1.0, RECRUIT_THRESHOLD, capacity),
            ANT_BATCH_SIZE
        );
        assert_eq!(
            recruitment_batch(1.0, 1.0, RECRUIT_THRESHOLD / 2.0, capacity),
            ANT_BATCH_SIZE / 2
        );
        assert_eq!(
            recruitment_batch(1.0, 1.0, RECRUIT_THRESHOLD * 10.0, capacity),
            ANT_BATCH_SIZE,
            "a full store must not over-spawn"
        );
        assert_eq!(
            recruitment_batch(1.0, 1.0, 0.0, capacity),
            0,
            "an empty store stops recruitment entirely"
        );
        assert_eq!(recruitment_batch(1.0, 1.0, RECRUIT_THRESHOLD, 7), 7);
        assert_eq!(recruitment_batch(0.0, 2.5, RECRUIT_THRESHOLD, capacity), 0);
    }

    fn spawn_app(store_food: f32) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / 64.0,
            )))
            .add_systems(FixedUpdate, spawn_ants);

        crate::simulation::register_sim_resources(&mut app);

        // Tests override the defaults after registration.
        app.insert_resource(NestStore::with_food(store_food))
            .insert_resource(AntSpawner {
                timer: Timer::from_seconds(0.01, TimerMode::Repeating),
            });

        app
    }

    #[test]
    fn spawn_ants_keeps_population_exact_and_gated_by_the_store() {
        let mut stocked = spawn_app(NEST_STORE_CAP);
        // The first frame only primes the virtual clock; the fixed step runs
        // on the second update.
        stocked.update();
        stocked.update();

        let mut query = stocked.world_mut().query::<&Ant>();
        let spawned = query.iter(stocked.world()).count();
        assert!(spawned > 0, "a stocked colony must recruit");
        assert_eq!(
            spawned,
            stocked.world().resource::<AntPopulation>().count(),
            "AntPopulation must stay exact"
        );

        let mut starving = spawn_app(0.0);
        starving.update();
        starving.update();
        assert_eq!(starving.world().resource::<AntPopulation>().count(), 0);
        let mut query = starving.world_mut().query::<&Ant>();
        assert_eq!(query.iter(starving.world()).count(), 0);
    }

    #[test]
    fn spawn_variation_is_within_the_documented_ranges() {
        let mut app = spawn_app(NEST_STORE_CAP);
        app.update();
        app.update();

        let mut count = 0;
        let mut query = app.world_mut().query::<&Ant>();

        for ant in query.iter(app.world()) {
            let bias_deg = ant.pi_bias.to_degrees().abs();
            assert!(
                (PI_BIAS_MIN_DEG..=PI_BIAS_MAX_DEG).contains(&bias_deg),
                "pi_bias {bias_deg} deg outside ±{PI_BIAS_MIN_DEG}..{PI_BIAS_MAX_DEG}"
            );
            assert!(
                (EXPLORE_TENDENCY_MIN..=EXPLORE_TENDENCY_MAX).contains(&ant.explore_tendency),
                "explore_tendency {} outside the range",
                ant.explore_tendency
            );
            assert!(
                (SENSOR_GAIN_MIN..=SENSOR_GAIN_MAX).contains(&ant.sensor_gain),
                "sensor_gain {} outside the range",
                ant.sensor_gain
            );
            assert!(
                (FORAGE_THRESHOLD_MIN..=FORAGE_THRESHOLD_MAX).contains(&ant.forage_threshold),
                "forage_threshold {} outside the range",
                ant.forage_threshold
            );
            assert!(
                (CROP_CAPACITY_MIN..=CROP_CAPACITY_MAX).contains(&ant.crop_capacity),
                "crop_capacity {} outside the range",
                ant.crop_capacity
            );
            count += 1;
        }

        assert!(
            count >= ANT_BATCH_SIZE,
            "expected a full first batch, got {count}"
        );

        // Two spawns of the same stream are bit-identical.
        let mut replay = spawn_app(NEST_STORE_CAP);
        replay.update();
        replay.update();
        let mut first: Vec<(f32, f32, f32, f32, f32)> = query
            .iter(app.world())
            .map(|ant| {
                (
                    ant.pi_bias,
                    ant.explore_tendency,
                    ant.sensor_gain,
                    ant.forage_threshold,
                    ant.crop_capacity,
                )
            })
            .collect();
        let mut query = replay.world_mut().query::<&Ant>();
        let mut second: Vec<(f32, f32, f32, f32, f32)> = query
            .iter(replay.world())
            .map(|ant| {
                (
                    ant.pi_bias,
                    ant.explore_tendency,
                    ant.sensor_gain,
                    ant.forage_threshold,
                    ant.crop_capacity,
                )
            })
            .collect();
        first.sort_by(|a, b| a.partial_cmp(b).unwrap());
        second.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(first, second, "spawn variation must be deterministic");
    }

    #[test]
    fn nursing_upkeep_drains_the_store() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Time::<Fixed>::from_hz(64.0))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / 64.0,
            )))
            .insert_resource(NestPosition(Vec2::ZERO))
            .init_resource::<AntPopulation>()
            .init_resource::<ColonyStats>()
            .insert_resource(NestStore::with_food(1.0))
            .add_systems(FixedUpdate, update_ant_energy_age);

        for _ in 0..2 {
            let mut ant = Ant::test_ant(0.0);
            ant.phase = AntPhase::Nursing;
            app.world_mut()
                .spawn((ant, Transform::from_xyz(0.0, 0.0, 0.0)));
        }
        app.world_mut().resource_mut::<AntPopulation>().add(2);

        // The first frame only primes the virtual clock; the fixed step runs
        // on the second update.
        app.update();
        app.update();

        let expected = 1.0 - 2.0 * NURSE_UPKEEP_PER_ANT * (1.0 / 64.0);
        let store = app.world().resource::<NestStore>();
        assert!(
            (store.food() - expected).abs() < 1e-6,
            "two nurses for one tick should drain {} food, store is {}",
            2.0 * NURSE_UPKEEP_PER_ANT * (1.0 / 64.0),
            store.food()
        );
    }

    /// F3: returning to the nest cancels the accumulated path-integration
    /// drift and refreshes the home anchor, while the fixed individual
    /// `pi_bias` survives. An ant outside the nest disc keeps its drift.
    #[test]
    fn nest_visit_resets_pi_drift_but_keeps_the_fixed_bias() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Time::<Fixed>::from_hz(64.0))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / 64.0,
            )))
            .insert_resource(NestPosition(Vec2::ZERO))
            .init_resource::<AntPopulation>()
            .init_resource::<ColonyStats>()
            .insert_resource(NestStore::with_food(1.0))
            .add_systems(FixedUpdate, update_ant_energy_age);

        let mut inside = Ant::test_ant(0.0);
        inside.phase = AntPhase::Nursing;
        inside.home = Vec2::new(50.0, 0.0);
        inside.pi_bias = -0.1;
        inside.pi_drift = 0.4;
        let inside_id = app
            .world_mut()
            .spawn((inside, Transform::from_xyz(NEST_RADIUS * 0.5, 0.0, 0.0)))
            .id();

        let mut outside = Ant::test_ant(0.0);
        outside.phase = AntPhase::Nursing;
        outside.pi_drift = -0.5;
        let outside_id = app
            .world_mut()
            .spawn((outside, Transform::from_xyz(NEST_RADIUS * 3.0, 0.0, 0.0)))
            .id();

        // The first frame only primes the virtual clock; the fixed step runs
        // on the second update.
        app.update();
        app.update();

        let inside = app.world().get::<Ant>(inside_id).unwrap();
        assert_eq!(inside.pi_drift, 0.0, "the nest must cancel the drift");
        assert_eq!(
            inside.pi_bias, -0.1,
            "the fixed individual bias must survive"
        );
        assert_eq!(
            inside.home,
            Vec2::ZERO,
            "the home anchor is refreshed at the nest"
        );

        let outside = app.world().get::<Ant>(outside_id).unwrap();
        assert_eq!(
            outside.pi_drift, -0.5,
            "drift outside the nest must be left untouched"
        );
    }

    /// Guards the foraging economy: a median (base-speed, base-lifetime) ant
    /// must be able to walk the default nest→food→nest circuit with both a
    /// time and an energy margin. This failed after the realism update, which
    /// is what starved emergent trails of successful return trips.
    #[test]
    fn median_ant_can_afford_the_default_round_trip() {
        use crate::constants::world::{FOOD_X, NEST_X};

        let distance = FOOD_X - NEST_X;
        let outbound_time = distance / ANT_SPEED;
        let laden_time = distance / (ANT_SPEED * CARRY_SPEED_FACTOR);
        let trip_energy = outbound_time * ANT_ENERGY_DRAIN_RATE
            + laden_time * ANT_ENERGY_DRAIN_RATE * ANT_CARRY_ENERGY_DRAIN_FACTOR;
        let trip_time = ANT_LIFETIME * ANT_NURSING_LIFETIME_FRACTION
            + outbound_time
            + laden_time
            + 2.0 * HANDLING_TIME;

        assert!(
            trip_energy < 0.8,
            "straight round trip needs {trip_energy:.3} energy; keep a 20% margin"
        );
        assert!(
            trip_time < ANT_LIFETIME * 0.8,
            "straight round trip takes {trip_time:.1}s of a {ANT_LIFETIME:.0}s median life"
        );
    }
}
