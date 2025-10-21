use crate::SelectedAnt;
use crate::components::Ant;
use bevy::prelude::*;
use std::f32::consts::PI;

const SENSOR_DISTANCE: f32 = 20.0;
const SENSOR_ANGLE: f32 = PI / 4.0;
const NUM_SENSORS: usize = 5;

#[derive(Component)]
pub struct SensorConeMarker;

pub fn draw_sensor_cone(
    mut commands: Commands,
    mut selected_ant: ResMut<SelectedAnt>,
    ant_query: Query<(Entity, &Ant, &Transform)>,
    existing_markers: Query<Entity, With<SensorConeMarker>>,
) {
    for entity in &existing_markers {
        commands.entity(entity).despawn();
    }

    let Some(selected_entity) = selected_ant.entity else {
        return;
    };

    if let Ok((_entity, ant, transform)) = ant_query.get(selected_entity) {
        let ant_pos = Vec2::new(transform.translation.x, transform.translation.y);

        for i in 0..NUM_SENSORS {
            let angle_offset =
                -SENSOR_ANGLE + (i as f32 / (NUM_SENSORS - 1) as f32) * (2.0 * SENSOR_ANGLE);
            let check_angle = ant.direction + angle_offset;

            let sensor_pos = ant_pos
                + Vec2::new(
                    check_angle.cos() * SENSOR_DISTANCE,
                    check_angle.sin() * SENSOR_DISTANCE,
                );

            commands.spawn((
                SensorConeMarker,
                Sprite {
                    color: Color::srgba(0.0, 1.0, 0.0, 0.6),
                    custom_size: Some(Vec2::new(3.0, 3.0)),
                    ..default()
                },
                Transform::from_xyz(sensor_pos.x, sensor_pos.y, 2.0),
            ));

            commands.spawn((
                SensorConeMarker,
                Sprite {
                    color: Color::srgba(0.0, 1.0, 0.0, 0.2),
                    custom_size: Some(Vec2::new(1.0, SENSOR_DISTANCE)),
                    ..default()
                },
                Transform::from_xyz(
                    ant_pos.x + (check_angle.cos() * SENSOR_DISTANCE / 2.0),
                    ant_pos.y + (check_angle.sin() * SENSOR_DISTANCE / 2.0),
                    1.5,
                )
                .with_rotation(Quat::from_rotation_z(check_angle - PI / 2.0)),
            ));
        }

        commands.spawn((
            SensorConeMarker,
            Sprite {
                color: Color::srgba(1.0, 0.0, 0.0, 0.8),
                custom_size: Some(Vec2::new(5.0, 5.0)),
                ..default()
            },
            Transform::from_xyz(ant_pos.x, ant_pos.y, 2.0),
        ));
    } else {
        selected_ant.entity = None;

        let ants: Vec<Entity> = ant_query.iter().map(|(e, _, _)| e).collect();
        if !ants.is_empty() {
            let random_index = (rand::random::<f32>() * ants.len() as f32) as usize;
            selected_ant.entity = Some(ants[random_index]);
        }
    }
}
