//! Multi-ring pheromone sampling.

use bevy::prelude::*;

use crate::constants::sensor::{
    NUM_SENSORS, SENSOR_ANGLE, SENSOR_DISTANCE, SENSOR_HALF_SATURATION, SENSOR_RING_ATTENUATION,
    SENSOR_RING_DISTANCES, SENSOR_SIGNAL_THRESHOLD,
};
use crate::core::grid::world_to_grid;
use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
use crate::simulation::ant::Ant;
use crate::simulation::environment::Obstacles;

/// Angle of ray `index` relative to the ant's heading, spanning
/// `-SENSOR_ANGLE..=SENSOR_ANGLE`.
pub fn sensor_offset(index: usize) -> f32 {
    let step = (2.0 * SENSOR_ANGLE) / (NUM_SENSORS - 1) as f32;
    -SENSOR_ANGLE + index as f32 * step
}

/// Saturating perception curve `raw / (raw + SENSOR_HALF_SATURATION)`.
///
/// Graded at operational values and independent of the storage clamp: a fresh
/// pass (raw ~0.04) reads ~0.02, a five-pass trail (~0.2) reads ~0.09, and a
/// busy trail (raw 20-70) reads 0.91-0.97. Monotonic and always below 1.
pub fn normalized_intensity(raw: f32) -> f32 {
    let raw = raw.max(0.0);
    raw / (raw + SENSOR_HALF_SATURATION)
}

/// Sample the strongest attenuated signal along each sensor ray (no
/// allocation). Laden and low-energy ants read the `ToNest` channel, everyone
/// else reads `ToFood`.
///
/// Obstacles occlude the rays: a sample is skipped when the segment from the
/// ant to it crosses an obstacle, so a trail behind a wall cannot be followed.
/// The exact segment tests only run for ants whose sensor fan actually reaches
/// an obstacle ([`Obstacles::any_within`]); with an empty obstacle set the
/// function is byte-for-byte the old fast path.
pub fn read_sensors(
    ant: &Ant,
    current_pos: Vec2,
    pheromone_grid: &PheromoneGrid,
    obstacles: &Obstacles,
) -> [f32; NUM_SENSORS] {
    let mut readings = [0.0; NUM_SENSORS];
    let kind = if ant.follows_to_nest() {
        PheromoneKind::ToNest
    } else {
        PheromoneKind::ToFood
    };
    let may_be_occluded =
        !obstacles.is_empty() && obstacles.any_within(current_pos, SENSOR_DISTANCE);

    for (index, reading) in readings.iter_mut().enumerate() {
        let check_angle = ant.direction + sensor_offset(index);
        let (sin, cos) = check_angle.sin_cos();
        let ray = Vec2::new(cos, sin);

        let mut strongest: f32 = 0.0;
        for &ring in &SENSOR_RING_DISTANCES {
            let sensor_pos = current_pos + ray * ring;

            if may_be_occluded && obstacles.blocks_segment(current_pos, sensor_pos) {
                continue;
            }

            if let Some(cell) = world_to_grid(sensor_pos) {
                let attenuated = normalized_intensity(pheromone_grid.sample(cell, kind))
                    * (-ring / SENSOR_RING_ATTENUATION).exp();
                strongest = strongest.max(attenuated);
            }
        }

        *reading = if strongest >= SENSOR_SIGNAL_THRESHOLD {
            strongest
        } else {
            0.0
        };
    }

    readings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::environment::Obstacle;

    const EPS: f32 = 1e-4;

    /// Raw cell intensity of one fresh forager pass (measured peak ~0.0367).
    const SINGLE_PASS_RAW: f32 = 0.04;
    /// Raw intensity of a five-pass trail.
    const FIVE_PASS_RAW: f32 = 5.0 * SINGLE_PASS_RAW;
    /// Raw intensity of a busy (100-pass) trail; the overlay scale documents
    /// busy trails at roughly 20-70.
    const BUSY_TRAIL_RAW: f32 = 20.0;

    fn strongest(readings: &[f32; NUM_SENSORS]) -> f32 {
        readings.iter().copied().fold(0.0, f32::max)
    }

    #[test]
    fn rays_stay_within_the_configured_half_angle() {
        for index in 0..NUM_SENSORS {
            let offset = sensor_offset(index);
            assert!(offset >= -SENSOR_ANGLE - EPS);
            assert!(offset <= SENSOR_ANGLE + EPS);
        }

        assert!((sensor_offset(0) + SENSOR_ANGLE).abs() < EPS);
        assert!((sensor_offset(NUM_SENSORS - 1) - SENSOR_ANGLE).abs() < EPS);
    }

    #[test]
    fn outermost_ring_matches_the_overlay_distance() {
        assert_eq!(
            SENSOR_RING_DISTANCES[SENSOR_RING_DISTANCES.len() - 1],
            SENSOR_DISTANCE
        );
        assert!(
            SENSOR_RING_DISTANCES
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
    }

    /// F9: the Michaelis-Menten curve is graded at operational values and
    /// saturates on busy trails instead of being scaled by the storage clamp.
    #[test]
    fn perception_curve_is_graded_and_saturating() {
        assert_eq!(normalized_intensity(0.0), 0.0);
        assert_eq!(normalized_intensity(-5.0), 0.0);

        let fresh = normalized_intensity(SINGLE_PASS_RAW);
        let five = normalized_intensity(FIVE_PASS_RAW);
        let busy = normalized_intensity(BUSY_TRAIL_RAW);

        assert!(fresh > 0.0 && fresh < 0.05, "fresh pass reads {fresh}");
        assert!(five > fresh * 3.0, "5 passes ({five}) vs 1 ({fresh})");
        assert!(busy > 0.9, "a busy trail must saturate, reads {busy}");
        assert!(normalized_intensity(1000.0) < 1.0);

        // Saturation: doubling a huge reading barely moves the curve.
        assert!(normalized_intensity(1000.0) - normalized_intensity(500.0) < 0.01);

        // Independent of the storage clamp: the old max already reads ~1.
        assert!(normalized_intensity(crate::constants::pheromone::PHEROMONE_MAX_INTENSITY) > 0.99);
    }

    /// F9: a single fresh pass must be detectable at the inner ring so the
    /// first discovery remains followable.
    #[test]
    fn single_fresh_pass_is_detectable_at_the_inner_ring() {
        let ant = Ant::test_ant(0.0);
        let mut grid = PheromoneGrid::new();
        let inner = world_to_grid(Vec2::new(SENSOR_RING_DISTANCES[0], 0.0)).expect("in bounds");
        grid.add(inner, SINGLE_PASS_RAW, 0.0);

        let reading = strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty()));
        assert!(
            reading >= SENSOR_SIGNAL_THRESHOLD,
            "a fresh pass must read at least {SENSOR_SIGNAL_THRESHOLD} at {} u, got {reading}",
            SENSOR_RING_DISTANCES[0]
        );

        // The outer ring still ignores a fresh pass (below threshold).
        let mut far = PheromoneGrid::new();
        let outer = world_to_grid(Vec2::new(SENSOR_DISTANCE, 0.0)).expect("in bounds");
        far.add(outer, SINGLE_PASS_RAW, 0.0);
        assert_eq!(
            strongest(&read_sensors(&ant, Vec2::ZERO, &far, &Obstacles::empty())),
            0.0
        );
    }

    /// F9: an established (5-pass) trail is strongly detected, including at
    /// the middle ring.
    #[test]
    fn five_pass_trail_is_strongly_detected() {
        let ant = Ant::test_ant(0.0);
        let mut grid = PheromoneGrid::new();

        for &ring in &SENSOR_RING_DISTANCES {
            if let Some(cell) = world_to_grid(Vec2::new(ring, 0.0)) {
                grid.add(cell, FIVE_PASS_RAW, 0.0);
            }
        }

        let reading = strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty()));
        assert!(
            reading > 4.0 * SENSOR_SIGNAL_THRESHOLD,
            "a 5-pass trail must steer strongly, got {reading}"
        );
    }

    /// F9: a busy trail saturates the sensor even at the outer ring.
    #[test]
    fn hundred_pass_trail_saturates_the_sensor() {
        assert!(normalized_intensity(BUSY_TRAIL_RAW) > 0.9);

        let ant = Ant::test_ant(0.0);
        let mut grid = PheromoneGrid::new();
        let outer = world_to_grid(Vec2::new(SENSOR_DISTANCE, 0.0)).expect("in bounds");
        grid.add(outer, BUSY_TRAIL_RAW, 0.0);

        let reading = strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty()));
        assert!(
            reading > 0.1,
            "a busy trail must be sensed at the outer ring, got {reading}"
        );
    }

    #[test]
    fn forward_deposit_is_sensed_but_backward_deposit_is_not() {
        let ant = Ant::test_ant(0.0);

        let mut grid = PheromoneGrid::new();
        let ahead = world_to_grid(Vec2::new(6.0, 0.0)).expect("in bounds");
        grid.add(ahead, BUSY_TRAIL_RAW, 0.0);

        let readings = read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty());
        assert!(strongest(&readings) > 0.0);

        grid.clear();
        let behind = world_to_grid(Vec2::new(-6.0, 0.0)).expect("in bounds");
        grid.add(behind, BUSY_TRAIL_RAW, 0.0);

        let readings = read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty());
        assert_eq!(strongest(&readings), 0.0);
    }

    #[test]
    fn laden_ants_read_the_to_nest_channel() {
        let ant = {
            let mut ant = Ant::test_ant(0.0);
            ant.has_food = true;
            ant
        };

        let mut grid = PheromoneGrid::new();
        let ahead = world_to_grid(Vec2::new(6.0, 0.0)).expect("in bounds");
        grid.add(ahead, BUSY_TRAIL_RAW, 0.0);
        assert_eq!(
            strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty())),
            0.0
        );

        grid.clear();
        grid.add(ahead, 0.0, BUSY_TRAIL_RAW);
        assert!(strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty())) > 0.0);
    }

    /// F16: a wall between the ant and the trail hides it, for both obstacle
    /// shapes, while an obstacle behind the ant leaves the reading untouched.
    #[test]
    fn obstacles_occlude_trails_they_stand_in_front_of() {
        let ant = Ant::test_ant(0.0);
        let mut grid = PheromoneGrid::new();
        let ahead = world_to_grid(Vec2::new(6.0, 0.0)).expect("in bounds");
        grid.add(ahead, BUSY_TRAIL_RAW, 0.0);

        let unoccluded = strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty()));
        assert!(unoccluded > 0.0);

        let wall = Obstacles::new(vec![Obstacle::aabb(
            Vec2::new(3.0, 0.0),
            Vec2::new(0.5, 2.0),
        )]);
        assert_eq!(
            strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &wall)),
            0.0,
            "an AABB between ant and trail must occlude it"
        );

        let boulder = Obstacles::new(vec![Obstacle::circle(Vec2::new(3.0, 0.0), 3.0)]);
        assert_eq!(
            strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &boulder)),
            0.0,
            "a circle covering every ray that samples the trail must occlude it"
        );

        let behind = Obstacles::new(vec![Obstacle::aabb(
            Vec2::new(-3.0, 0.0),
            Vec2::new(0.5, 2.0),
        )]);
        assert_eq!(
            strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &behind)),
            unoccluded,
            "an obstacle behind the ant must not occlude the forward ray"
        );
    }

    /// Occlusion is per ray: an obstacle that only covers the outer rays leaves
    /// the center ray reading intact.
    #[test]
    fn occlusion_only_affects_rays_that_cross_the_obstacle() {
        let ant = Ant::test_ant(0.0);
        let mut grid = PheromoneGrid::new();
        let ahead = world_to_grid(Vec2::new(6.0, 0.0)).expect("in bounds");
        grid.add(ahead, BUSY_TRAIL_RAW, 0.0);

        let unoccluded = strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &Obstacles::empty()));

        // Sits over the +60 degree ray (3.0, 5.2) but not over the center ray.
        let side = Obstacles::new(vec![Obstacle::aabb(
            Vec2::new(3.0, 6.0),
            Vec2::new(1.0, 1.0),
        )]);
        assert_eq!(
            strongest(&read_sensors(&ant, Vec2::ZERO, &grid, &side)),
            unoccluded,
            "the center ray must still read the trail"
        );
    }

    /// Ignored micro-benchmark of the occlusion overhead at 30k ants: one
    /// `read_sensors` per ant and tick with and without the default obstacle
    /// set. Run with:
    /// `cargo test --release --locked --bin ants sensor_occlusion_micro_bench -- --ignored --nocapture`
    #[test]
    #[ignore = "perf micro-benchmark, run on demand"]
    fn sensor_occlusion_micro_bench_30k() {
        use crate::simulation::environment::Obstacle;
        use std::time::Instant;

        const ANTS: usize = 30_000;
        const TICKS: u32 = 100;

        let obstacles = Obstacles::new(vec![
            Obstacle::circle(Vec2::new(-120.0, 170.0), 34.0),
            Obstacle::aabb(Vec2::new(40.0, -180.0), Vec2::new(70.0, 12.0)),
            Obstacle::aabb(Vec2::new(240.0, 200.0), Vec2::new(14.0, 70.0)),
        ]);
        let empty = Obstacles::empty();
        let grid = PheromoneGrid::new();

        let mut rng = fastrand::Rng::with_seed(0x5E_50_12);
        let ants: Vec<(Ant, Vec2)> = (0..ANTS)
            .map(|_| {
                let ant = Ant::test_ant(rng.f32() * std::f32::consts::TAU);
                let pos = Vec2::new(rng.f32() * 760.0 - 380.0, rng.f32() * 560.0 - 280.0);
                (ant, pos)
            })
            .collect();

        let bench = |obstacles: &Obstacles| {
            for _ in 0..5 {
                for (ant, pos) in &ants {
                    std::hint::black_box(read_sensors(ant, *pos, &grid, obstacles));
                }
            }

            let start = Instant::now();

            for _ in 0..TICKS {
                for (ant, pos) in &ants {
                    std::hint::black_box(read_sensors(ant, *pos, &grid, obstacles));
                }
            }

            start.elapsed().as_secs_f64() * 1000.0 / f64::from(TICKS)
        };

        let empty_ms = bench(&empty);
        let obstacle_ms = bench(&obstacles);

        println!(
            "sensor occlusion micro-bench: {ANTS} ants, {TICKS} ticks, \
             empty {empty_ms:.3} ms/tick, {} obstacles {obstacle_ms:.3} ms/tick (+{:.3} ms/tick)",
            obstacles.len(),
            obstacle_ms - empty_ms
        );
    }
}
