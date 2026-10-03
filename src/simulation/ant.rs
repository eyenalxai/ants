//! Ant component, population bookkeeping and the energy/age lifecycle.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use std::f32::consts::PI;

use crate::constants::ant::*;
use crate::constants::colony::{
    ENERGY_FULL_EPS, ENERGY_REFILL_COST, ENERGY_REFILL_RATE, NURSE_UPKEEP_PER_ANT,
    RECRUIT_THRESHOLD,
};
use crate::constants::world::NEST_RADIUS;
use crate::core::layers::Z_ANT;
use crate::simulation::NestPosition;
use crate::simulation::colony::{ColonyStats, NestStore};

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
    pub fn tick_handling(&mut self, dt: f32) {
        if self.handling_timer > 0.0 {
            self.handling_timer = (self.handling_timer - dt).max(0.0);
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
                if self.phase == AntPhase::Returning {
                    self.phase = AntPhase::Foraging;
                }
            }

            return false;
        }

        let drain_factor = if self.has_food {
            ANT_CARRY_ENERGY_DRAIN_FACTOR
        } else {
            1.0
        };
        self.energy -= ANT_ENERGY_DRAIN_RATE * drain_factor * dt;

        if self.energy <= 0.0 {
            return true;
        }

        if self.energy < ANT_ENERGY_RETURN_THRESHOLD && self.phase == AntPhase::Foraging {
            self.phase = AntPhase::Returning;
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
        }
    }
}

/// Current number of live ants, decremented when ants despawn.
#[derive(Resource, Default)]
pub struct AntPopulation(pub usize);

impl AntPopulation {
    /// Number of live ants.
    pub fn count(&self) -> usize {
        self.0
    }

    /// Record `n` new ants, saturating at `usize::MAX`.
    ///
    /// Together with [`Self::remove`] these are the only mutation entry points
    /// going forward; the field stays public until the biology stream
    /// privatizes it.
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

    let capacity = MAX_ANTS.saturating_sub(population.0);
    if capacity == 0 {
        return;
    }

    let nest_pos = inputs.nest_position.0;
    let ramp = (1.0 - population.0 as f32 / MAX_ANTS as f32).clamp(0.0, 1.0);
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
        // sqrt keeps the spawn distribution uniform over the nest disc.
        let spawn_radius = NEST_RADIUS * rng.f32().sqrt();
        let spawn_pos = nest_pos + Vec2::new(spawn_angle.cos(), spawn_angle.sin()) * spawn_radius;
        let max_lifetime = ANT_LIFETIME * (ANT_LIFETIME_VARIATION_MIN + rng.f32());
        let base_speed = ANT_SPEED * (ANT_SPEED_VARIATION_MIN + rng.f32());

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
                pi_bias: 0.0,
                pi_drift: 0.0,
                lost_time: 0.0,
                prev_pos: spawn_pos,
                forage_threshold: 0.5,
                explore_tendency: 1.0,
                sensor_gain: 1.0,
                crop_capacity: 1.0,
                carrying_quality: 1.0,
                rest_timer: 0.0,
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

    population.0 += batch_size;
}

/// Advance age, handling pauses and energy; despawn ants that die of old age
/// or exhaustion, and refresh the home vector inside the nest.
///
/// Also drains the colony's nursing upkeep: every nursing ant consumes
/// [`NURSE_UPKEEP_PER_ANT`] food per second from the store, so a colony
/// without income shrinks even before its foragers starve.
pub fn update_ant_energy_age(
    mut commands: Commands,
    mut ant_query: Query<(Entity, &mut Ant, &Transform)>,
    mut population: ResMut<AntPopulation>,
    mut nest_store: ResMut<NestStore>,
    nest_position: Res<NestPosition>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();
    let nest_pos = nest_position.0;
    let nest_radius_squared = NEST_RADIUS * NEST_RADIUS;
    let mut nurses: u32 = 0;

    for (entity, mut ant, transform) in &mut ant_query {
        ant.tick_handling(dt);
        ant.age += dt;

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        let in_nest = pos.distance_squared(nest_pos) < nest_radius_squared;

        if ant.age >= ant.max_lifetime || ant.tick_energy(dt, in_nest, &mut nest_store) {
            commands.entity(entity).despawn();
            population.0 = population.0.saturating_sub(1);
            continue;
        }

        if ant.phase == AntPhase::Nursing {
            nurses += 1;
        }

        if in_nest {
            ant.home = nest_pos;
        }

        if ant.phase == AntPhase::Nursing && ant.nursing_over() {
            ant.phase = AntPhase::Foraging;
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
        let mut store = NestStore { food: 0.0 };
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

        let mut store = NestStore { food: 0.0 };
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
        laden.has_food = true;
        let mut walker = Ant::test_ant(0.0);
        let mut empty = NestStore { food: 0.0 };

        laden.tick_energy(1.0, false, &mut empty);
        walker.tick_energy(1.0, false, &mut empty);
        assert!(laden.energy < walker.energy);

        laden.phase = AntPhase::Returning;
        let mut stocked = NestStore { food: 10.0 };
        assert!(!laden.tick_energy(1.0, true, &mut stocked));
        assert!((laden.energy - 1.0).abs() < EPS);
        assert_eq!(laden.phase, AntPhase::Foraging);
        assert!(stocked.food() < 10.0, "the refill must be paid for");
    }

    #[test]
    fn nest_refill_consumes_store_and_stalls_when_empty() {
        let dt = 1.0 / 64.0;
        let mut ant = Ant::test_ant(0.0);
        ant.energy = 0.2;
        let mut store = NestStore { food: 10.0 };

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
        store.food = 0.0;
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
        let mut fine_store = NestStore { food: 10.0 };
        for _ in 0..64 {
            fine.tick_energy(1.0 / 64.0, true, &mut fine_store);
        }

        let mut coarse = Ant::test_ant(0.0);
        coarse.energy = 0.2;
        let mut coarse_store = NestStore { food: 10.0 };
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
        app.insert_resource(NestStore { food: store_food })
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
            stocked.world().resource::<AntPopulation>().0,
            "AntPopulation must stay exact"
        );

        let mut starving = spawn_app(0.0);
        starving.update();
        starving.update();
        assert_eq!(starving.world().resource::<AntPopulation>().0, 0);
        let mut query = starving.world_mut().query::<&Ant>();
        assert_eq!(query.iter(starving.world()).count(), 0);
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
            .insert_resource(NestStore { food: 1.0 })
            .add_systems(FixedUpdate, update_ant_energy_age);

        for _ in 0..2 {
            let mut ant = Ant::test_ant(0.0);
            ant.phase = AntPhase::Nursing;
            app.world_mut()
                .spawn((ant, Transform::from_xyz(0.0, 0.0, 0.0)));
        }
        app.world_mut().resource_mut::<AntPopulation>().0 = 2;

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
