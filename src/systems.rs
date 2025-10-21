use crate::ant_spawner::AntSpawner;
use crate::components::{Ant, Food, Nest};
use crate::constants::{ANT_SPEED, MAX_ANTS};
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

pub fn move_ants(mut ant_query: Query<(&Ant, &mut Transform)>, time: Res<Time>) {
    for (ant, mut transform) in &mut ant_query {
        let velocity = Vec2::new(ant.direction.cos(), ant.direction.sin()) * ANT_SPEED;
        transform.translation.x += velocity.x * time.delta_secs();
        transform.translation.y += velocity.y * time.delta_secs();
    }
}
