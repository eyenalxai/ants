//! Ant steering: trail following, path integration, route memory, food
//! approach, antennal casting and wandering.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use crate::constants::ant::*;
use crate::constants::sensor::{NUM_SENSORS, SENSOR_ANGLE};
use crate::constants::world::{NEST_RADIUS, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};
use crate::core::grid::world_to_grid;
use crate::simulation::ant::Ant;
use crate::simulation::food::FoodGrid;
use crate::simulation::movement::sensors::sensor_offset;

/// Signed shortest difference from `from` to `to`, in `(-PI, PI]`.
pub fn shortest_angle_diff(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(2.0 * PI) - PI
}

/// Turn `ant.direction` toward `target`, capped at `max_turn` radians.
pub fn turn_towards(ant: &mut Ant, target: f32, max_turn: f32) {
    let turn = shortest_angle_diff(ant.direction, target).clamp(-max_turn, max_turn);
    ant.direction = (ant.direction + turn).rem_euclid(2.0 * PI);
}

/// Strongest normalized reading, used as the trail-strength signal in `[0, 1]`.
///
/// Using the maximum (rather than the mean over all rays) keeps a directional
/// trail that lights only two or three rays from being diluted by the empty
/// rays: a committed ant should commit.
pub fn trail_strength(readings: &[f32; NUM_SENSORS]) -> f32 {
    readings.iter().copied().fold(0.0, f32::max).clamp(0.0, 1.0)
}

/// World-space unit vector toward the strongest trail side. `None` when no
/// reading carries a usable signal.
pub fn trail_vector(ant: &Ant, readings: &[f32; NUM_SENSORS]) -> Option<Vec2> {
    let mut sum = Vec2::ZERO;

    for (index, &reading) in readings.iter().enumerate() {
        if reading <= 0.0 {
            continue;
        }

        let angle = ant.direction + sensor_offset(index);
        sum += Vec2::new(angle.cos(), angle.sin()) * reading;
    }

    (sum.length_squared() > f32::EPSILON).then(|| sum.normalize())
}

/// Path-integration-corrected angle toward the nest, in radians.
///
/// The true home vector is rotated by the ant's fixed bias and its
/// distance-accumulated drift; `None` when the ant sits exactly on its home
/// coordinate.
pub fn homeward_angle(ant: &Ant, pos: Vec2) -> Option<f32> {
    let to_home = ant.home - pos;

    if to_home.length_squared() <= f32::EPSILON {
        return None;
    }

    Some(to_home.y.atan2(to_home.x) + ant.pi_bias + ant.pi_drift)
}

/// Accumulate path-integration drift for `distance` world units travelled.
///
/// The drift is a clamped random walk: `(rng - 0.5) * distance /
/// [`PI_DRIFT_DISTANCE`]`. The biology stream resets it in the nest; the clamp
/// keeps homing sane if that reset has not landed yet.
pub fn accumulate_pi_drift(ant: &mut Ant, distance: f32, rng: &mut fastrand::Rng) {
    if distance <= 0.0 {
        return;
    }

    let noise = rng.f32() - 0.5;
    ant.pi_drift =
        (ant.pi_drift + noise * distance / PI_DRIFT_DISTANCE).clamp(-PI_DRIFT_MAX, PI_DRIFT_MAX);
}

/// Per-ant sensor gain and noise, applied to the readings returned by
/// `read_sensors`. Zero readings stay zero (they carry no signal) and do not
/// consume a noise draw.
pub fn apply_sensor_noise(readings: &mut [f32; NUM_SENSORS], ant: &Ant, rng: &mut fastrand::Rng) {
    for reading in readings.iter_mut() {
        if *reading > 0.0 {
            *reading *= ant.sensor_gain * (1.0 + (rng.f32() - 0.5) * SENSOR_NOISE);
        }
    }
}

/// Desired heading blending the followed trail with the path-integrated home
/// vector. The home vector dominates as the trail fades; with no trail at all
/// the ant walks home with a small heading noise.
pub fn homeward_desired(
    ant: &Ant,
    pos: Vec2,
    readings: &[f32; NUM_SENSORS],
    rng: &mut fastrand::Rng,
) -> f32 {
    let strength = trail_strength(readings);
    let trail = trail_vector(ant, readings);
    let home_angle = homeward_angle(ant, pos);

    match (trail, home_angle) {
        (Some(trail), Some(home_angle)) => {
            let home_weight = ANT_HOME_WEIGHT_BASE + ANT_HOME_WEIGHT_TRAIL * (1.0 - strength);
            let home = Vec2::new(home_angle.cos(), home_angle.sin());
            let blended = trail * (1.0 - home_weight) + home * home_weight;

            if blended.length_squared() > f32::EPSILON {
                blended.y.atan2(blended.x)
            } else {
                home_angle
            }
        }
        (Some(trail), None) => trail.y.atan2(trail.x),
        (None, Some(home_angle)) => {
            let noise = (rng.f32() - 0.5) * 2.0 * ANT_HOME_HEADING_NOISE;
            home_angle + noise
        }
        (None, None) => ant.direction,
    }
}

/// Steer a returning ant: path integration blended with the `ToNest` trail,
/// capped at one [`ANT_TURN_RATE`] step.
pub fn steer_homeward(
    ant: &mut Ant,
    pos: Vec2,
    readings: &[f32; NUM_SENSORS],
    delta: f32,
    rng: &mut fastrand::Rng,
) {
    let desired = homeward_desired(ant, pos, readings, rng);
    turn_towards(ant, desired, ANT_TURN_RATE * delta);
}

/// Steer toward a sensed food position with a turn-rate-limited turn.
pub fn steer_towards_food(
    ant: &mut Ant,
    food_pos: Vec2,
    pos: Vec2,
    delta: f32,
    rng: &mut fastrand::Rng,
) {
    let offset = food_pos - pos;

    if offset.length_squared() <= f32::EPSILON {
        return;
    }

    let max_turn = ANT_TURN_RATE * delta;
    let jitter = (rng.f32() - 0.5) * max_turn * FOOD_APPROACH_JITTER_FACTOR;
    turn_towards(ant, offset.y.atan2(offset.x) + jitter, max_turn);
}

/// Remember the world position `food_pos` as a nest-relative route.
pub fn remember_food(ant: &mut Ant, food_pos: Vec2) {
    ant.route_memory = Some(food_pos - ant.home);
}

/// Angle toward the remembered food, if the remembered cell still holds food.
///
/// Clears the memory when the remembered cell is out of bounds or empty, so a
/// depleted source stops attracting ants after a single check.
pub fn route_memory_angle(ant: &mut Ant, pos: Vec2, food_grid: &FoodGrid) -> Option<f32> {
    let target = ant.home + ant.route_memory?;
    let has_food = world_to_grid(target)
        .and_then(|cell| food_grid.amount(cell))
        .is_some_and(|amount| amount > 0.0);

    if !has_food {
        ant.route_memory = None;
        return None;
    }

    let offset = target - pos;
    (offset.length_squared() > f32::EPSILON).then(|| offset.y.atan2(offset.x))
}

/// Steer along the remembered route when it still points at food. Returns
/// whether the memory was usable this tick.
pub fn steer_towards_route_memory(
    ant: &mut Ant,
    pos: Vec2,
    food_grid: &FoodGrid,
    delta: f32,
    rng: &mut fastrand::Rng,
) -> bool {
    let Some(target) = route_memory_angle(ant, pos, food_grid) else {
        return false;
    };

    let max_turn = ANT_TURN_RATE * delta;
    let jitter = (rng.f32() - 0.5) * max_turn * ROUTE_MEMORY_JITTER_FACTOR;
    turn_towards(ant, target + jitter, max_turn);
    true
}

/// Nurses wander near the nest and are pulled home when they drift too far.
pub fn steer_nursing(ant: &mut Ant, pos: Vec2, delta: f32, rng: &mut fastrand::Rng) {
    let leash = NEST_RADIUS * NURSING_LEASH_FACTOR;
    let to_home = ant.home - pos;

    if to_home.length_squared() > leash * leash {
        let noise = (rng.f32() - 0.5) * 2.0 * ANT_HOME_HEADING_NOISE;
        turn_towards(
            ant,
            to_home.y.atan2(to_home.x) + noise,
            ANT_TURN_RATE * delta,
        );
    } else if rng.f32() < ANT_RANDOM_TURN_CHANCE {
        apply_random_turn(ant, delta * NURSING_RANDOM_TURN_FACTOR, rng);
    }
}

/// Turn the ant toward the strongest pheromone reading (or wander randomly).
///
/// Readings are already gated per sensor by `SENSOR_SIGNAL_THRESHOLD`, so any
/// non-zero total is a usable signal. `activity` scales the exploration
/// probability so a resting colony explores less.
pub fn apply_steering(
    ant: &mut Ant,
    pos: Vec2,
    sensor_readings: &[f32; NUM_SENSORS],
    delta: f32,
    activity: f32,
    rng: &mut fastrand::Rng,
) {
    let total_intensity: f32 = sensor_readings.iter().sum();
    let intensity_strength = trail_strength(sensor_readings);
    // A strong signal suppresses exploration so a busy trail is followed
    // tightly; with no signal the ant explores at the full base rate.
    let explore_chance =
        ANT_EXPLORATION_CHANCE * (1.0 - intensity_strength) * activity.clamp(0.0, 1.0);
    let cast = update_lost_state(ant, intensity_strength, delta, rng);

    if total_intensity > 0.0 && rng.f32() > explore_chance {
        let use_probabilistic = rng.f32() < ANT_PROBABILISTIC_STEERING_CHANCE;

        let target_direction = if use_probabilistic {
            probabilistic_direction(ant, sensor_readings, total_intensity, rng)
        } else {
            weighted_direction(ant, sensor_readings)
        };

        let max_turn = ANT_TURN_RATE
            * delta
            * (ANT_TURN_INTENSITY_BASE + intensity_strength * ANT_TURN_INTENSITY_SCALE);
        turn_towards(ant, target_direction, max_turn);
    } else if let Some(sweep) = cast {
        apply_casting(ant, pos, delta, sweep);
    } else if rng.f32() < ANT_RANDOM_TURN_CHANCE {
        apply_random_turn(ant, delta, rng);
    }
}

/// Advance the lost-signal timer for one tick and return the casting sweep
/// angle delta for this tick (`None` while not casting).
///
/// The timer also carries the short U-turn delay: a fresh loss may schedule a
/// U-turn by going negative, and the reversal fires when the timer crosses
/// zero. Casting windows are `[CAST_AFTER + k / CAST_HZ, +CAST_DURATION)`.
pub fn update_lost_state(
    ant: &mut Ant,
    strength: f32,
    delta: f32,
    rng: &mut fastrand::Rng,
) -> Option<f32> {
    if strength > 0.0 {
        ant.lost_time = 0.0;
        return None;
    }

    // The signal was present last tick and just dropped: maybe schedule a
    // delayed U-turn at the trail end. Only ants that have found food before
    // U-turn — a naive forager with no route has not lost anything.
    let knows_route = ant.trips_completed > 0 || ant.route_memory.is_some();

    if ant.lost_time == 0.0 && knows_route && rng.f32() < U_TURN_CHANCE {
        ant.lost_time = -U_TURN_DELAY;
    }

    let pending_u_turn = ant.lost_time < 0.0;
    ant.lost_time += delta;

    if pending_u_turn && ant.lost_time >= 0.0 {
        apply_u_turn(ant, rng);
    }

    if ant.lost_time < CAST_AFTER {
        return None;
    }

    let period = 1.0 / CAST_HZ;
    let in_window = (ant.lost_time - CAST_AFTER).rem_euclid(period);

    if in_window >= CAST_DURATION {
        return None;
    }

    // One full sweep cycle per window: the offset starts and ends at zero, so
    // successive windows cannot accumulate into a spiral.
    let phase = (in_window / CAST_DURATION) * TAU;
    let step = (delta / CAST_DURATION) * TAU;
    let sweep = CAST_SWEEP_FRACTION * SENSOR_ANGLE * ((phase + step).sin() - phase.sin());

    Some(sweep)
}

/// Apply one casting step: the sweep delta plus a pull toward the nearest
/// wall's tangent when lost close to a boundary.
fn apply_casting(ant: &mut Ant, pos: Vec2, delta: f32, sweep: f32) {
    let max_turn = ANT_TURN_RATE * delta;
    let turn = (sweep + wall_tangent_pull(ant, pos, delta)).clamp(-max_turn, max_turn);
    ant.direction = (ant.direction + turn).rem_euclid(TAU);
}

/// Turn-rate-proportional pull toward the tangent of the nearest wall, active
/// only within [`CAST_WALL_MARGIN`]. The tangent keeps the current travel
/// direction, so a lost ant follows the boundary instead of bouncing.
fn wall_tangent_pull(ant: &Ant, pos: Vec2, delta: f32) -> f32 {
    let margin_x = PLAY_AREA_WIDTH / 2.0 - pos.x.abs();
    let margin_y = PLAY_AREA_HEIGHT / 2.0 - pos.y.abs();

    if margin_x.min(margin_y) > CAST_WALL_MARGIN {
        return 0.0;
    }

    let tangent = if margin_x < margin_y {
        if ant.direction.sin() >= 0.0 {
            FRAC_PI_2
        } else {
            -FRAC_PI_2
        }
    } else if ant.direction.cos() >= 0.0 {
        0.0
    } else {
        PI
    };

    CAST_WALL_TANGENT_FACTOR * shortest_angle_diff(ant.direction, tangent) * ANT_TURN_RATE * delta
}

/// Reverse the heading with a small jitter, used at a trail end.
fn apply_u_turn(ant: &mut Ant, rng: &mut fastrand::Rng) {
    let jitter = (rng.f32() - 0.5) * 2.0 * U_TURN_JITTER;
    ant.direction = (ant.direction + PI + jitter).rem_euclid(TAU);
}

fn probabilistic_direction(
    ant: &Ant,
    sensor_readings: &[f32; NUM_SENSORS],
    total_intensity: f32,
    rng: &mut fastrand::Rng,
) -> f32 {
    let random_value = rng.f32() * total_intensity;
    let mut cumulative = 0.0;
    let mut chosen_angle = ant.direction;

    for (index, &intensity) in sensor_readings.iter().enumerate() {
        cumulative += intensity;
        if cumulative >= random_value {
            chosen_angle = ant.direction + sensor_offset(index);
            break;
        }
    }

    let noise = (rng.f32() - 0.5) * SENSOR_ANGLE * ANT_STEERING_NOISE_FACTOR;
    chosen_angle + noise
}

fn weighted_direction(ant: &Ant, sensor_readings: &[f32; NUM_SENSORS]) -> f32 {
    let mut weighted = Vec2::ZERO;

    for (index, &intensity) in sensor_readings.iter().enumerate() {
        let angle = ant.direction + sensor_offset(index);
        weighted += Vec2::new(angle.cos(), angle.sin()) * intensity;
    }

    if weighted.length_squared() > f32::EPSILON {
        weighted.y.atan2(weighted.x)
    } else {
        ant.direction
    }
}

fn apply_random_turn(ant: &mut Ant, delta: f32, rng: &mut fastrand::Rng) {
    let turn_amount = (rng.f32() - 0.5) * 2.0 * ANT_TURN_RATE * delta;
    ant.direction = (ant.direction + turn_amount).rem_euclid(2.0 * PI);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::world::INITIAL_FOOD_AMOUNT;

    const EPS: f32 = 1e-4;

    fn food_grid_at(world_pos: Vec2, amount: f32) -> FoodGrid {
        let mut grid = FoodGrid::default();
        let mut world = World::new();
        let mut commands = world.commands();
        let cell = world_to_grid(world_pos).expect("food position in bounds");
        grid.set(&mut commands, cell, amount);
        grid
    }

    #[test]
    fn shortest_angle_wraps_correctly() {
        assert!((shortest_angle_diff(0.0, PI / 2.0) - PI / 2.0).abs() < EPS);
        assert!((shortest_angle_diff(0.0, -PI / 2.0) + PI / 2.0).abs() < EPS);
        assert!((shortest_angle_diff(0.1, 2.0 * PI - 0.1) + 0.2).abs() < EPS);
        assert!((shortest_angle_diff(2.0 * PI - 0.1, 0.1) - 0.2).abs() < EPS);
    }

    #[test]
    fn turn_is_capped_at_max_turn() {
        let mut ant = Ant::test_ant(0.0);
        turn_towards(&mut ant, PI / 2.0, 0.1);
        assert!((ant.direction - 0.1).abs() < EPS);

        let mut ant = Ant::test_ant(0.0);
        turn_towards(&mut ant, -PI / 2.0, 10.0);
        assert!((ant.direction - (2.0 * PI - PI / 2.0)).abs() < EPS);
    }

    #[test]
    fn homeward_steering_prefers_home_when_there_is_no_trail() {
        let mut ant = Ant::test_ant(0.0);
        ant.home = Vec2::new(0.0, 100.0);
        let mut rng = fastrand::Rng::with_seed(0);

        let readings = [0.0; NUM_SENSORS];
        for _ in 0..200 {
            steer_homeward(&mut ant, Vec2::ZERO, &readings, 1.0 / 64.0, &mut rng);
        }

        // Path integration should have turned the ant roughly northwards.
        let (sin, cos) = ant.direction.sin_cos();
        assert!(sin > 0.9, "ant should head home, got {sin} (cos {cos})");
    }

    #[test]
    fn pi_bias_produces_a_measurable_homing_offset() {
        let mut ant = Ant::test_ant(0.0);
        ant.home = Vec2::new(0.0, 100.0);
        ant.pi_bias = 0.3;

        let readings = [0.0; NUM_SENSORS];
        let mut rng = fastrand::Rng::with_seed(7);
        for _ in 0..200 {
            steer_homeward(&mut ant, Vec2::ZERO, &readings, 1.0 / 64.0, &mut rng);
        }

        let offset = shortest_angle_diff(PI / 2.0, ant.direction);
        assert!(
            (offset - 0.3).abs() < 0.1,
            "homing direction should carry the {:.1} rad bias, got {offset:.3}",
            0.3
        );
    }

    #[test]
    fn pi_drift_accumulates_with_distance_and_is_clamped() {
        let mut ant = Ant::test_ant(0.0);
        let mut rng = fastrand::Rng::with_seed(11);

        for _ in 0..10_000 {
            accumulate_pi_drift(&mut ant, 0.78, &mut rng);
        }
        assert!(ant.pi_drift != 0.0, "distance should accumulate drift");
        assert!(ant.pi_drift.abs() <= PI_DRIFT_MAX);

        ant.pi_drift = PI_DRIFT_MAX;
        accumulate_pi_drift(&mut ant, 1e6, &mut rng);
        assert!(
            (ant.pi_drift.abs() - PI_DRIFT_MAX).abs() < EPS,
            "a huge step must clamp to ±PI_DRIFT_MAX, got {}",
            ant.pi_drift
        );
    }

    #[test]
    fn route_memory_steers_toward_food_and_clears_when_depleted() {
        let mut ant = Ant::test_ant(PI);
        ant.home = Vec2::ZERO;
        ant.route_memory = Some(Vec2::new(64.0, 0.0));

        let stocked = food_grid_at(Vec2::new(64.0, 0.0), INITIAL_FOOD_AMOUNT);
        let mut rng = fastrand::Rng::with_seed(3);
        for _ in 0..300 {
            assert!(steer_towards_route_memory(
                &mut ant,
                Vec2::ZERO,
                &stocked,
                1.0 / 64.0,
                &mut rng
            ));
        }
        assert!(
            shortest_angle_diff(ant.direction, 0.0).abs() < 0.1,
            "route memory should turn the ant east, got {}",
            ant.direction
        );
        assert!(ant.route_memory.is_some(), "a stocked route must persist");

        let empty = FoodGrid::default();
        assert!(!steer_towards_route_memory(
            &mut ant,
            Vec2::ZERO,
            &empty,
            1.0 / 64.0,
            &mut rng
        ));
        assert!(
            ant.route_memory.is_none(),
            "a depleted remembered cell must clear the memory"
        );
    }

    #[test]
    fn casting_starts_after_the_delay_and_pauses_between_windows() {
        let mut ant = Ant::test_ant(0.0);
        let mut rng = fastrand::Rng::with_seed(5);
        let dt = 1.0 / 64.0;
        let mut cast_ticks = 0;

        for _ in 0..64 {
            // 1 s: covers the 0.5 s delay and two windows.
            if update_lost_state(&mut ant, 0.0, dt, &mut rng).is_some() {
                cast_ticks += 1;
            }
        }

        assert!(cast_ticks > 0, "casting must start after CAST_AFTER");
        assert!(ant.lost_time > CAST_AFTER);

        // Exactly in the pause between windows (0.4 s into a 0.5 s period).
        ant.lost_time = CAST_AFTER + CAST_DURATION + 0.01;
        assert!(update_lost_state(&mut ant, 0.0, dt, &mut rng).is_none());

        // A signal resets the timer and stops casting.
        assert!(update_lost_state(&mut ant, 0.5, dt, &mut rng).is_none());
        assert_eq!(ant.lost_time, 0.0);
    }

    #[test]
    fn casting_sweeps_both_ways_and_walls_bend_it_inward() {
        let mut ant = Ant::test_ant(0.0);
        let mut rng = fastrand::Rng::with_seed(9);
        let dt = 1.0 / 64.0;
        let mut positive = false;
        let mut negative = false;

        for _ in 0..128 {
            if let Some(sweep) = update_lost_state(&mut ant, 0.0, dt, &mut rng) {
                positive |= sweep > 0.0;
                negative |= sweep < 0.0;
            }
        }

        assert!(positive && negative, "the sweep must oscillate both ways");

        // Near the right wall while travelling into it, the pull turns the
        // ant toward the wall tangent (north for a positive y component).
        let near_wall = Ant::test_ant(0.1);
        let pull = wall_tangent_pull(&near_wall, Vec2::new(PLAY_AREA_WIDTH / 2.0 - 2.0, 0.0), dt);
        assert!(pull > 0.0, "lost ants should bend toward the wall tangent");

        let far = Ant::test_ant(0.0);
        assert_eq!(wall_tangent_pull(&far, Vec2::ZERO, dt), 0.0);
    }

    #[test]
    fn a_fresh_loss_can_schedule_a_u_turn_that_reverses_after_the_delay() {
        let dt = 1.0 / 64.0;

        for seed in 0..32 {
            let mut ant = Ant::test_ant(0.0);
            ant.trips_completed = 1;
            let mut rng = fastrand::Rng::with_seed(seed);
            let before = ant.direction;

            for _ in 0..64 {
                update_lost_state(&mut ant, 0.0, dt, &mut rng);
            }

            if shortest_angle_diff(before, ant.direction).abs() > 2.0 {
                assert!(
                    (shortest_angle_diff(before, ant.direction).abs() - PI).abs() < 0.3,
                    "a U-turn should reverse the heading"
                );
                return;
            }
        }

        panic!("no seed in 0..32 scheduled a U-turn at U_TURN_CHANCE = {U_TURN_CHANCE}");
    }

    #[test]
    fn trail_strength_uses_the_strongest_ray_not_the_mean() {
        let mut readings = [0.0; NUM_SENSORS];
        readings[3] = 0.3;
        readings[4] = 0.3;
        readings[5] = 0.3;

        assert!(
            (trail_strength(&readings) - 0.3).abs() < EPS,
            "a directional 3-ray trail must keep its full strength, got {}",
            trail_strength(&readings)
        );
    }

    #[test]
    fn sensor_noise_keeps_zero_readings_zero_and_scales_gain() {
        let mut ant = Ant::test_ant(0.0);
        ant.sensor_gain = 2.0;
        let mut rng = fastrand::Rng::with_seed(13);
        let mut readings = [0.0; NUM_SENSORS];
        readings[4] = 0.5;

        apply_sensor_noise(&mut readings, &ant, &mut rng);

        assert_eq!(readings[0], 0.0);
        assert!(
            (0.9..=1.1).contains(&readings[4]),
            "gain 2 with ±10% noise must stay near 1.0, got {}",
            readings[4]
        );
    }
}
