use crate::ant_spawner::AntSpawner;
use crate::components::{Ant, Nest};
use crate::constants::{ANT_BATCH_SIZE, ANT_LIFETIME, ANT_SPEED, MAX_ANTS};
use bevy::prelude::*;
use std::f32::consts::PI;

pub fn spawn_ants(
    mut commands: Commands,
    mut spawner: ResMut<AntSpawner>,
    time: Res<Time>,
    nest_query: Query<&Transform, With<Nest>>,
    ant_query: Query<&Ant>,
) {
    let current_ant_count = ant_query.iter().count();

    if current_ant_count >= MAX_ANTS {
        return;
    }

    spawner.timer.tick(time.delta());

    if spawner.timer.just_finished()
        && let Ok(nest_transform) = nest_query.single()
    {
        let batch_size = ANT_BATCH_SIZE.min(MAX_ANTS - current_ant_count);

        for _ in 0..batch_size {
            let random_angle = rand::random::<f32>() * 2.0 * PI;
            let lifetime_variation = 0.5 + rand::random::<f32>();
            let speed_variation = 0.5 + rand::random::<f32>();
            let max_lifetime = ANT_LIFETIME * lifetime_variation;

            commands.spawn((
                Ant {
                    direction: random_angle,
                    has_food: false,
                    lifetime: max_lifetime,
                    max_lifetime,
                    speed: ANT_SPEED * speed_variation,
                },
                Sprite {
                    color: Color::srgba(1.0, 1.0, 1.0, 0.01),
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

pub fn update_ant_lifetime(
    mut commands: Commands,
    mut ant_query: Query<(Entity, &mut Ant)>,
    time: Res<Time>,
) {
    for (entity, mut ant) in &mut ant_query {
        ant.lifetime -= time.delta_secs();

        if ant.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
