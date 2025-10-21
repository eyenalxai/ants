use crate::components::{Nest, PheromoneCell};
use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use crate::resources::FoodCells;
use bevy::prelude::*;

pub fn setup(
    mut commands: Commands,
    mut food_cells: ResMut<FoodCells>,
    pheromone_grid: Res<PheromoneGrid>,
) {
    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.0, 0.0, 0.0)),
            ..default()
        },
    ));

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
            custom_size: Some(Vec2::new(NEST_SIZE, NEST_SIZE)),
            ..default()
        },
        Transform::from_xyz(NEST_X, NEST_Y, 0.0),
    ));

    if let Some((food_grid_x, food_grid_y)) =
        pheromone_grid.world_to_grid(Vec2::new(FOOD_X, FOOD_Y))
    {
        food_cells.cells.push((food_grid_x, food_grid_y));

        let world_x = food_grid_x as f32 * GRID_SIZE - WINDOW_WIDTH as f32 / 2.0 + GRID_SIZE / 2.0;
        let world_y = food_grid_y as f32 * GRID_SIZE - WINDOW_HEIGHT as f32 / 2.0 + GRID_SIZE / 2.0;

        commands.spawn((
            Sprite {
                color: Color::srgb(0.2, 0.8, 0.2),
                custom_size: Some(Vec2::new(GRID_SIZE, GRID_SIZE)),
                ..default()
            },
            Transform::from_xyz(world_x, world_y, 0.5),
        ));
    }
}
