use bevy::prelude::*;
use std::f32::consts::PI;

use crate::constants::ant::*;
use crate::core::layers::Z_ANT;
use crate::simulation::Nest;

#[derive(Component)]
pub struct Ant {
    pub direction: f32,
    pub has_food: bool,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub speed: f32,
}

/// Current number of live ants, decremented when ants despawn. Used instead of
/// a full query count for the population cap.
#[derive(Resource, Default)]
pub struct AntPopulation(pub usize);

#[derive(Resource)]
pub struct AntSpawner {
    pub timer: Timer,
    pub count: usize,
}

pub fn spawn_ants(
    mut commands: Commands,
    mut spawner: ResMut<AntSpawner>,
    mut population: ResMut<AntPopulation>,
    time: Res<Time<Fixed>>,
    nest_query: Query<&Transform, With<Nest>>,
) {
    if population.0 >= MAX_ANTS {
        return;
    }

    spawner.timer.tick(time.delta());

    if spawner.timer.just_finished()
        && let Ok(nest_transform) = nest_query.single()
    {
        let batch_size = ANT_BATCH_SIZE.min(MAX_ANTS - population.0);

        for _ in 0..batch_size {
            let random_angle = fastrand::f32() * 2.0 * PI;
            let lifetime_variation = ANT_LIFETIME_VARIATION_MIN + fastrand::f32();
            let speed_variation = ANT_SPEED_VARIATION_MIN + fastrand::f32();
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
                    color: Color::srgba(1.0, 1.0, 1.0, ANT_ALPHA),
                    custom_size: Some(Vec2::new(ANT_SIZE, ANT_SIZE)),
                    ..default()
                },
                Transform::from_xyz(
                    nest_transform.translation.x,
                    nest_transform.translation.y,
                    Z_ANT,
                ),
            ));

            spawner.count += 1;
        }

        population.0 += batch_size;
    }
}

pub fn update_ant_lifetime(
    mut commands: Commands,
    mut ant_query: Query<(Entity, &mut Ant)>,
    mut population: ResMut<AntPopulation>,
    time: Res<Time<Fixed>>,
) {
    for (entity, mut ant) in &mut ant_query {
        ant.lifetime -= time.delta_secs();

        if ant.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            population.0 = population.0.saturating_sub(1);
        }
    }
}
