use bevy::prelude::*;

use crate::constants::sensor::{NUM_SENSORS, SENSOR_ANGLE, SENSOR_DISTANCE};
use crate::core::grid::world_to_grid;
use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
use crate::simulation::ant::Ant;

/// Read the pheromone intensity at each sensor position (no allocation).
pub fn read_sensors(
    ant: &Ant,
    current_pos: Vec2,
    pheromone_grid: &PheromoneGrid,
) -> [f32; NUM_SENSORS] {
    let mut sensor_readings = [0.0; NUM_SENSORS];
    let sensor_step = (2.0 * SENSOR_ANGLE) / (NUM_SENSORS - 1) as f32;
    let kind = if ant.has_food {
        PheromoneKind::ToNest
    } else {
        PheromoneKind::ToFood
    };

    for (index, reading) in sensor_readings.iter_mut().enumerate() {
        let angle_offset = -SENSOR_ANGLE + index as f32 * sensor_step;
        let check_angle = ant.direction + angle_offset;
        let (sin, cos) = check_angle.sin_cos();

        let sensor_pos = current_pos + Vec2::new(cos * SENSOR_DISTANCE, sin * SENSOR_DISTANCE);

        if let Some(cell) = world_to_grid(sensor_pos) {
            *reading = pheromone_grid.sample(cell, kind);
        }
    }

    sensor_readings
}
