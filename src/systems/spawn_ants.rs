use crate::ant_spawner::AntSpawner;
use crate::components::{Ant, Nest};
use crate::constants::MAX_ANTS;
use bevy::prelude::*;
use std::f32::consts::PI;

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
        let batch_size = 10.min(MAX_ANTS - spawner.count);

        for _ in 0..batch_size {
            let random_angle = rand::random::<f32>() * 2.0 * PI;

            commands.spawn((
                Ant {
                    direction: random_angle,
                    has_food: false,
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
}
