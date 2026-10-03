//! Multi-ring pheromone sampling.

use bevy::prelude::*;

use crate::constants::pheromone::PHEROMONE_MAX_INTENSITY;
use crate::constants::sensor::{
    NUM_SENSORS, SENSOR_ANGLE, SENSOR_RING_ATTENUATION, SENSOR_RING_DISTANCES,
    SENSOR_SIGNAL_THRESHOLD,
};
use crate::core::grid::world_to_grid;
use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
use crate::simulation::ant::Ant;

/// Angle of ray `index` relative to the ant's heading, spanning
/// `-SENSOR_ANGLE..=SENSOR_ANGLE`.
pub fn sensor_offset(index: usize) -> f32 {
    let step = (2.0 * SENSOR_ANGLE) / (NUM_SENSORS - 1) as f32;
    -SENSOR_ANGLE + index as f32 * step
}

/// Compressive normalization of a raw pheromone reading to `[0, 1]`.
pub fn normalized_intensity(raw: f32) -> f32 {
    (raw / PHEROMONE_MAX_INTENSITY).clamp(0.0, 1.0).sqrt()
}

/// Sample the strongest attenuated signal along each sensor ray (no
/// allocation). Laden and low-energy ants read the `ToNest` channel, everyone
/// else reads `ToFood`.
pub fn read_sensors(
    ant: &Ant,
    current_pos: Vec2,
    pheromone_grid: &PheromoneGrid,
) -> [f32; NUM_SENSORS] {
    let mut readings = [0.0; NUM_SENSORS];
    let kind = if ant.follows_to_nest() {
        PheromoneKind::ToNest
    } else {
        PheromoneKind::ToFood
    };

    for (index, reading) in readings.iter_mut().enumerate() {
        let check_angle = ant.direction + sensor_offset(index);
        let (sin, cos) = check_angle.sin_cos();
        let ray = Vec2::new(cos, sin);

        let mut strongest: f32 = 0.0;
        for &ring in &SENSOR_RING_DISTANCES {
            let sensor_pos = current_pos + ray * ring;

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
    use crate::constants::sensor::SENSOR_DISTANCE;

    const EPS: f32 = 1e-4;

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

    #[test]
    fn normalization_is_compressive_and_monotonic() {
        assert_eq!(normalized_intensity(0.0), 0.0);
        assert_eq!(normalized_intensity(-5.0), 0.0);
        assert!((normalized_intensity(PHEROMONE_MAX_INTENSITY) - 1.0).abs() < EPS);
        assert!((normalized_intensity(2.0 * PHEROMONE_MAX_INTENSITY) - 1.0).abs() < EPS);

        let low = normalized_intensity(PHEROMONE_MAX_INTENSITY * 0.25);
        let high = normalized_intensity(PHEROMONE_MAX_INTENSITY * 0.75);
        assert!(low < high);
        assert!(low > 0.25, "sqrt should lift mid-range readings");
    }

    #[test]
    fn forward_deposit_is_sensed_but_backward_deposit_is_not() {
        let ant = Ant::test_ant(0.0);

        let mut grid = PheromoneGrid::new();
        let ahead = world_to_grid(Vec2::new(6.0, 0.0)).expect("in bounds");
        grid.add(ahead, PHEROMONE_MAX_INTENSITY, 0.0);

        let readings = read_sensors(&ant, Vec2::ZERO, &grid);
        assert!(readings.iter().copied().fold(0.0, f32::max) > 0.0);

        grid.clear();
        let behind = world_to_grid(Vec2::new(-6.0, 0.0)).expect("in bounds");
        grid.add(behind, PHEROMONE_MAX_INTENSITY, 0.0);

        let readings = read_sensors(&ant, Vec2::ZERO, &grid);
        assert_eq!(readings.iter().copied().fold(0.0, f32::max), 0.0);
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
        grid.add(ahead, PHEROMONE_MAX_INTENSITY, 0.0);
        assert_eq!(
            read_sensors(&ant, Vec2::ZERO, &grid)
                .iter()
                .copied()
                .fold(0.0, f32::max),
            0.0
        );

        grid.clear();
        grid.add(ahead, 0.0, PHEROMONE_MAX_INTENSITY);
        assert!(
            read_sensors(&ant, Vec2::ZERO, &grid)
                .iter()
                .copied()
                .fold(0.0, f32::max)
                > 0.0
        );
    }
}
