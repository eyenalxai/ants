use crate::components::Ant;
use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use bevy::prelude::*;

pub fn read_sensors(
    ant: &Ant,
    current_pos: Vec2,
    pheromone_grid: &PheromoneGrid,
) -> Vec<(f32, f32)> {
    let mut sensor_readings = Vec::with_capacity(NUM_SENSORS);
    let sensor_step = (2.0 * SENSOR_ANGLE) / (NUM_SENSORS - 1) as f32;

    for i in 0..NUM_SENSORS {
        let angle_offset = -SENSOR_ANGLE + i as f32 * sensor_step;
        let check_angle = ant.direction + angle_offset;
        let (sin, cos) = check_angle.sin_cos();

        let sensor_pos = current_pos + Vec2::new(cos * SENSOR_DISTANCE, sin * SENSOR_DISTANCE);

        let intensity = if let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(sensor_pos)
            && let Some(pheromone) = pheromone_grid.get(grid_x, grid_y)
        {
            if ant.has_food {
                pheromone.to_nest
            } else {
                pheromone.to_food
            }
        } else {
            0.0
        };

        sensor_readings.push((check_angle, intensity));
    }

    sensor_readings
}
