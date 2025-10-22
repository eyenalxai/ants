use crate::components::Ant;
use crate::constants::*;
use crate::resources::SelectedAnt;
use bevy::prelude::*;
use std::f32::consts::PI;

#[derive(Component)]
pub struct SensorConeMarker;

#[derive(Component)]
pub struct SensorLineMarker(usize);

#[derive(Component)]
pub struct SensorDotMarker(usize);

#[derive(Component)]
pub struct SensorAntMarker;

#[allow(clippy::type_complexity)]
pub fn draw_sensor_cone(
    mut commands: Commands,
    mut selected_ant: ResMut<SelectedAnt>,
    ant_query: Query<(Entity, &Ant, &Transform)>,
    mut dot_query: Query<
        (&SensorDotMarker, &mut Transform, &mut Visibility),
        (
            Without<Ant>,
            Without<SensorLineMarker>,
            Without<SensorAntMarker>,
        ),
    >,
    mut line_query: Query<
        (&SensorLineMarker, &mut Transform, &mut Visibility),
        (
            Without<Ant>,
            Without<SensorDotMarker>,
            Without<SensorAntMarker>,
        ),
    >,
    mut ant_marker_query: Query<
        (&mut Transform, &mut Visibility),
        (
            With<SensorAntMarker>,
            Without<Ant>,
            Without<SensorLineMarker>,
            Without<SensorDotMarker>,
        ),
    >,
) {
    let Some(selected_entity) = selected_ant.entity else {
        for (_, _, mut vis) in &mut dot_query {
            *vis = Visibility::Hidden;
        }
        for (_, _, mut vis) in &mut line_query {
            *vis = Visibility::Hidden;
        }
        for (_, mut vis) in &mut ant_marker_query {
            *vis = Visibility::Hidden;
        }
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

            let mut found_dot = false;
            for (marker, mut t, mut vis) in &mut dot_query {
                if marker.0 == i {
                    t.translation.x = sensor_pos.x;
                    t.translation.y = sensor_pos.y;
                    *vis = Visibility::Visible;
                    found_dot = true;
                    break;
                }
            }
            if !found_dot {
                commands.spawn((
                    SensorConeMarker,
                    SensorDotMarker(i),
                    Sprite {
                        color: Color::srgba(0.0, 1.0, 0.0, SENSOR_CONE_MARKER_ALPHA),
                        custom_size: Some(Vec2::new(
                            SENSOR_CONE_MARKER_SIZE,
                            SENSOR_CONE_MARKER_SIZE,
                        )),
                        ..default()
                    },
                    Transform::from_xyz(sensor_pos.x, sensor_pos.y, 2.0),
                ));
            }

            let line_pos_x = ant_pos.x + (check_angle.cos() * SENSOR_DISTANCE / 2.0);
            let line_pos_y = ant_pos.y + (check_angle.sin() * SENSOR_DISTANCE / 2.0);

            let mut found_line = false;
            for (marker, mut t, mut vis) in &mut line_query {
                if marker.0 == i {
                    t.translation.x = line_pos_x;
                    t.translation.y = line_pos_y;
                    t.rotation = Quat::from_rotation_z(check_angle - PI / 2.0);
                    *vis = Visibility::Visible;
                    found_line = true;
                    break;
                }
            }
            if !found_line {
                commands.spawn((
                    SensorConeMarker,
                    SensorLineMarker(i),
                    Sprite {
                        color: Color::srgba(0.0, 1.0, 0.0, SENSOR_CONE_LINE_ALPHA),
                        custom_size: Some(Vec2::new(SENSOR_CONE_LINE_WIDTH, SENSOR_DISTANCE)),
                        ..default()
                    },
                    Transform::from_xyz(line_pos_x, line_pos_y, 1.5)
                        .with_rotation(Quat::from_rotation_z(check_angle - PI / 2.0)),
                ));
            }
        }

        for (marker, _, mut vis) in &mut dot_query {
            if marker.0 >= NUM_SENSORS {
                *vis = Visibility::Hidden;
            }
        }
        for (marker, _, mut vis) in &mut line_query {
            if marker.0 >= NUM_SENSORS {
                *vis = Visibility::Hidden;
            }
        }

        if let Ok((mut t, mut vis)) = ant_marker_query.single_mut() {
            t.translation.x = ant_pos.x;
            t.translation.y = ant_pos.y;
            *vis = Visibility::Visible;
        } else {
            commands.spawn((
                SensorConeMarker,
                SensorAntMarker,
                Sprite {
                    color: Color::srgba(1.0, 0.0, 0.0, SENSOR_CONE_ANT_ALPHA),
                    custom_size: Some(Vec2::new(
                        SENSOR_CONE_ANT_MARKER_SIZE,
                        SENSOR_CONE_ANT_MARKER_SIZE,
                    )),
                    ..default()
                },
                Transform::from_xyz(ant_pos.x, ant_pos.y, 2.0),
            ));
        }
    } else {
        selected_ant.entity = None;

        let ants: Vec<Entity> = ant_query.iter().map(|(e, _, _)| e).collect();
        if !ants.is_empty() {
            let random_index = (fastrand::f32() * ants.len() as f32) as usize;
            selected_ant.entity = Some(ants[random_index]);
        }

        for (_, _, mut vis) in &mut dot_query {
            *vis = Visibility::Hidden;
        }
        for (_, _, mut vis) in &mut line_query {
            *vis = Visibility::Hidden;
        }
        for (_, mut vis) in &mut ant_marker_query {
            *vis = Visibility::Hidden;
        }
    }
}
