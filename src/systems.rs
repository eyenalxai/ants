use crate::ant_spawner::AntSpawner;
use crate::components::{Ant, Food, Nest, PheromoneCell};
use crate::constants::{
    ANT_SPEED, GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, MAX_ANTS, WINDOW_HEIGHT, WINDOW_WIDTH,
};
use crate::pheromone::PheromoneGrid;
use bevy::prelude::*;
use std::f32::consts::PI;

pub fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    for y in 0..GRID_HEIGHT {
        for x in 0..GRID_WIDTH {
            let world_x = x as f32 * GRID_SIZE - WINDOW_WIDTH as f32 / 2.0 + GRID_SIZE / 2.0;
            let world_y = y as f32 * GRID_SIZE - WINDOW_HEIGHT as f32 / 2.0 + GRID_SIZE / 2.0;

            commands.spawn((
                PheromoneCell {
                    grid_x: x,
                    grid_y: y,
                },
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.0),
                    custom_size: Some(Vec2::new(GRID_SIZE, GRID_SIZE)),
                    ..default()
                },
                Transform::from_xyz(world_x, world_y, 0.0),
            ));
        }
    }

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

pub fn move_ants(
    mut ant_query: Query<(&mut Ant, &mut Transform)>,
    time: Res<Time>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
) {
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

        let pos = Vec2::new(transform.translation.x, transform.translation.y);
        if let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(pos)
            && let Some(cell) = pheromone_grid.get_mut(grid_x, grid_y)
        {
            if ant.has_food {
                cell.to_food += 1.0;
            } else {
                cell.to_nest += 1.0;
            }
        }
    }
}

pub fn check_collisions(
    mut ant_query: Query<(&mut Ant, &Transform)>,
    food_query: Query<&Transform, With<Food>>,
    nest_query: Query<&Transform, With<Nest>>,
) {
    let Some(food_transform) = food_query.iter().next() else {
        return;
    };
    let Some(nest_transform) = nest_query.iter().next() else {
        return;
    };
    let food_pos = Vec2::new(food_transform.translation.x, food_transform.translation.y);
    let nest_pos = Vec2::new(nest_transform.translation.x, nest_transform.translation.y);
    let food_radius = 15.0 / 2.0;
    let nest_radius = 40.0 / 2.0;

    for (mut ant, transform) in &mut ant_query {
        let ant_pos = Vec2::new(transform.translation.x, transform.translation.y);

        if !ant.has_food && ant_pos.distance(food_pos) < food_radius {
            ant.has_food = true;
        } else if ant.has_food && ant_pos.distance(nest_pos) < nest_radius {
            ant.has_food = false;
        }
    }
}

pub fn update_pheromone_visuals(
    mut cell_query: Query<(&PheromoneCell, &mut Sprite)>,
    pheromone_grid: Res<PheromoneGrid>,
) {
    for (cell, mut sprite) in &mut cell_query {
        if let Some(pheromone) = pheromone_grid.get(cell.grid_x, cell.grid_y) {
            let to_food_intensity = (pheromone.to_food / 100.0).min(1.0);
            let to_nest_intensity = (pheromone.to_nest / 100.0).min(1.0);

            let red = to_food_intensity;
            let blue = to_nest_intensity;
            let alpha = (to_food_intensity + to_nest_intensity).min(1.0) * 0.5;

            sprite.color = Color::srgba(red, 0.0, blue, alpha);
        }
    }
}
