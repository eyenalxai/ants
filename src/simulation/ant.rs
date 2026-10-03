//! Ant component, population bookkeeping and the energy/age lifecycle.

use bevy::prelude::*;
use std::f32::consts::PI;

use crate::constants::ant::*;
use crate::constants::world::NEST_RADIUS;
use crate::core::layers::Z_ANT;
use crate::simulation::Nest;
use crate::simulation::colony::ColonyStats;

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
}

impl Ant {
    pub fn is_handling(&self) -> bool {
        self.handling_timer > 0.0
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
    /// Inside the nest the tank refills instantly and a low-energy return is
    /// considered complete.
    pub fn tick_energy(&mut self, dt: f32, in_nest: bool) -> bool {
        if in_nest {
            self.energy = 1.0;
            if self.phase == AntPhase::Returning {
                self.phase = AntPhase::Foraging;
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

        if self.energy < ANT_ENERGY_RETURN_THRESHOLD {
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
            home: Vec2::ZERO,
            age: 0.0,
            max_lifetime: ANT_LIFETIME,
            energy: 1.0,
            phase: AntPhase::Foraging,
            handling_timer: 0.0,
            base_speed: ANT_SPEED,
            speed: 0.0,
            trips_completed: 0,
        }
    }
}

/// Current number of live ants, decremented when ants despawn.
#[derive(Resource, Default)]
pub struct AntPopulation(pub usize);

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

/// Spawn new nurses at a ramped rate: slower near the population cap, faster
/// after recent food deliveries. [`MAX_ANTS`] stays a hard safety cap.
pub fn spawn_ants(
    mut commands: Commands,
    mut spawner: ResMut<AntSpawner>,
    mut population: ResMut<AntPopulation>,
    colony: Res<ColonyStats>,
    time: Res<Time<Fixed>>,
    nest_query: Query<&Transform, With<Nest>>,
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

    let Ok(nest_transform) = nest_query.single() else {
        return;
    };

    let nest_pos = Vec2::new(nest_transform.translation.x, nest_transform.translation.y);
    let ramp = (1.0 - population.0 as f32 / MAX_ANTS as f32).clamp(0.0, 1.0);
    let batch_size = ((ANT_BATCH_SIZE as f32 * ramp * colony.delivery_boost()) as usize)
        .max(1)
        .min(capacity);

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
                home: nest_pos,
                age: 0.0,
                max_lifetime,
                energy: 1.0,
                phase: AntPhase::Nursing,
                handling_timer: 0.0,
                base_speed,
                speed: 0.0,
                trips_completed: 0,
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
pub fn update_ant_energy_age(
    mut commands: Commands,
    mut ant_query: Query<(Entity, &mut Ant, &Transform)>,
    mut population: ResMut<AntPopulation>,
    nest_query: Query<&Transform, With<Nest>>,
    time: Res<Time<Fixed>>,
) {
    let dt = time.delta_secs();
    let nest_pos = nest_query
        .iter()
        .next()
        .map(|transform| Vec2::new(transform.translation.x, transform.translation.y));

    for (entity, mut ant, transform) in &mut ant_query {
        ant.tick_handling(dt);
        ant.age += dt;

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        let in_nest = nest_pos.is_some_and(|nest| pos.distance(nest) < NEST_RADIUS);

        if ant.age >= ant.max_lifetime || ant.tick_energy(dt, in_nest) {
            commands.entity(entity).despawn();
            population.0 = population.0.saturating_sub(1);
            continue;
        }

        if let Some(nest) = nest_pos
            && in_nest
        {
            ant.home = nest;
        }

        if ant.phase == AntPhase::Nursing && ant.nursing_over() {
            ant.phase = AntPhase::Foraging;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn energy_drain_forces_returning_then_starvation() {
        let mut ant = Ant::test_ant(0.0);
        let dt = 1.0 / 64.0;

        let mut ticks = 0;
        while ant.phase != AntPhase::Returning && ticks < 64 * 120 {
            assert!(!ant.tick_energy(dt, false));
            ticks += 1;
        }

        assert_eq!(ant.phase, AntPhase::Returning);
        assert!(ant.energy < ANT_ENERGY_RETURN_THRESHOLD);

        let mut starved = false;
        for _ in 0..64 * 120 {
            if ant.tick_energy(dt, false) {
                starved = true;
                break;
            }
        }
        assert!(starved);
    }

    #[test]
    fn carrying_drains_faster_and_nest_refills() {
        let mut laden = Ant::test_ant(0.0);
        laden.has_food = true;
        let mut walker = Ant::test_ant(0.0);

        laden.tick_energy(1.0, false);
        walker.tick_energy(1.0, false);
        assert!(laden.energy < walker.energy);

        laden.phase = AntPhase::Returning;
        assert!(!laden.tick_energy(1.0, true));
        assert!((laden.energy - 1.0).abs() < EPS);
        assert_eq!(laden.phase, AntPhase::Foraging);
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
