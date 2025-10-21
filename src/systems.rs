use crate::ant_spawner::AntSpawner;
use crate::components::{Ant, Food, Nest};
use crate::constants::{ANT_SPEED, MAX_ANTS, WINDOW_HEIGHT, WINDOW_WIDTH};
use bevy::prelude::*;
use std::f32::consts::PI;

pub fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Nest,
        Sprite {
            color: Color::srgb(0.4, 0.25, 0.1),
            custom_size: Some(Vec2::new(40.0, 40.0)),
            ..default()
        },
        Transform::from_xyz(-350.0, 0.0, 0.0),
    ));

    commands.spawn((
        Food,
        Sprite {
            color: Color::srgb(0.2, 0.8, 0.2),
            custom_size: Some(Vec2::new(15.0, 15.0)),
            ..default()
        },
        Transform::from_xyz(320.0, 0.0, 0.0),
    ));
}

pub fn spawn_ants(
    mut commands: Commands,
    mut spawner: ResMut<AntSpawner>,
    time: Res<Time>,
    nest_query: Query<&Transform, With<Nest>>,
) {
    if spawner.count >= MAX_ANTS {
        return;
    }

    spawner.timer.tick(time.delta());

    if spawner.timer.just_finished()
        && let Ok(nest_transform) = nest_query.single()
    {
        let random_angle = rand::random::<f32>() * 2.0 * PI;

        commands.spawn((
            Ant {
                direction: random_angle,
            },
            Sprite {
                color: Color::srgb(0.1, 0.1, 0.1),
                custom_size: Some(Vec2::new(2.0, 2.0)),
                ..default()
            },
            Transform::from_xyz(
                nest_transform.translation.x,
                nest_transform.translation.y,
                1.0,
            ),
        ));

        spawner.count += 1;
    }
}

pub fn move_ants(mut ant_query: Query<(&mut Ant, &mut Transform)>, time: Res<Time>) {
    let half_width = WINDOW_WIDTH as f32 / 2.0;
    let half_height = WINDOW_HEIGHT as f32 / 2.0;
    let min_angle = 30.0_f32.to_radians();

    for (mut ant, mut transform) in &mut ant_query {
        let velocity = Vec2::new(ant.direction.cos(), ant.direction.sin()) * ANT_SPEED;
        transform.translation.x += velocity.x * time.delta_secs();
        transform.translation.y += velocity.y * time.delta_secs();

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
}
