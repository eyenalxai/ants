use crate::components::Ant;
use crate::constants::*;
use bevy::prelude::*;
use std::f32::consts::PI;

pub fn handle_wall_collision(ant: &mut Ant, transform: &mut Transform) {
    let half_width = PLAY_AREA_WIDTH / 2.0;
    let half_height = PLAY_AREA_HEIGHT / 2.0;
    let min_angle = WALL_BOUNCE_MIN_ANGLE.to_radians();

    if transform.translation.x > half_width || transform.translation.x < -half_width {
        if transform.translation.x > half_width {
            transform.translation.x = half_width;
        } else {
            transform.translation.x = -half_width;
        }

        let mut new_direction = PI - ant.direction;

        let angle_to_horizontal = (new_direction.rem_euclid(2.0 * PI) - PI).abs();
        if angle_to_horizontal < min_angle {
            if new_direction.sin() > 0.0 {
                new_direction = PI - min_angle;
            } else {
                new_direction = PI + min_angle;
            }
        }

        ant.direction = new_direction.rem_euclid(2.0 * PI);
    }

    if transform.translation.y > half_height || transform.translation.y < -half_height {
        if transform.translation.y > half_height {
            transform.translation.y = half_height;
        } else {
            transform.translation.y = -half_height;
        }

        let mut new_direction = (2.0 * PI) - ant.direction;

        let normalized = new_direction.rem_euclid(2.0 * PI);
        let angle_to_horizontal = normalized.min((2.0 * PI) - normalized);
        if angle_to_horizontal < min_angle {
            if new_direction.cos() > 0.0 {
                new_direction = min_angle;
            } else {
                new_direction = PI - min_angle;
            }
        }

        ant.direction = new_direction.rem_euclid(2.0 * PI);
    }
}
