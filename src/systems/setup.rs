use crate::components::{Food, Nest, PheromoneCell};
use crate::constants::{GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, WINDOW_HEIGHT, WINDOW_WIDTH};
use bevy::prelude::*;

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
