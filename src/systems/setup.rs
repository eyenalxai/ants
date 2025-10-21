use crate::components::{Nest, PheromoneCell, Wall};
use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use crate::resources::FoodCells;
use crate::systems::food_management::FoodMarker;
use bevy::prelude::*;

pub fn setup(
    mut commands: Commands,
    mut food_cells: ResMut<FoodCells>,
    pheromone_grid: Res<PheromoneGrid>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.1)),
            ..default()
        },
    ));

    for y in 0..GRID_HEIGHT {
        for x in 0..GRID_WIDTH {
            let world_x = x as f32 * GRID_SIZE - PLAY_AREA_WIDTH / 2.0 + GRID_SIZE / 2.0;
            let world_y = y as f32 * GRID_SIZE - PLAY_AREA_HEIGHT / 2.0 + GRID_SIZE / 2.0;

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

    let wall_color = Color::srgb(0.3, 0.3, 0.3);
    let half_width = PLAY_AREA_WIDTH / 2.0;
    let half_height = PLAY_AREA_HEIGHT / 2.0;

    commands.spawn((
        Wall,
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(
                PLAY_AREA_WIDTH + WALL_THICKNESS * 2.0,
                WALL_THICKNESS,
            )),
            ..default()
        },
        Transform::from_xyz(0.0, half_height + WALL_THICKNESS / 2.0, 0.1),
    ));

    commands.spawn((
        Wall,
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(
                PLAY_AREA_WIDTH + WALL_THICKNESS * 2.0,
                WALL_THICKNESS,
            )),
            ..default()
        },
        Transform::from_xyz(0.0, -half_height - WALL_THICKNESS / 2.0, 0.1),
    ));

    commands.spawn((
        Wall,
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(WALL_THICKNESS, PLAY_AREA_HEIGHT)),
            ..default()
        },
        Transform::from_xyz(half_width + WALL_THICKNESS / 2.0, 0.0, 0.1),
    ));

    commands.spawn((
        Wall,
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(WALL_THICKNESS, PLAY_AREA_HEIGHT)),
            ..default()
        },
        Transform::from_xyz(-half_width - WALL_THICKNESS / 2.0, 0.0, 0.1),
    ));

    let nest_radius = NEST_SIZE / 4.0;
    commands.spawn((
        Nest,
        Mesh2d(meshes.add(Circle::new(nest_radius))),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgb(1.0, 0.0, 0.0)))),
        Transform::from_xyz(NEST_X, NEST_Y, 0.0),
    ));

    if let Some((food_grid_x, food_grid_y)) =
        pheromone_grid.world_to_grid(Vec2::new(FOOD_X, FOOD_Y))
    {
        for dy in 0..3 {
            for dx in 0..3 {
                let gx = food_grid_x + dx;
                let gy = food_grid_y + dy;

                if gx < GRID_WIDTH && gy < GRID_HEIGHT {
                    food_cells.cells.insert((gx, gy), INITIAL_FOOD_AMOUNT);

                    let world_x = gx as f32 * GRID_SIZE - PLAY_AREA_WIDTH / 2.0 + GRID_SIZE / 2.0;
                    let world_y = gy as f32 * GRID_SIZE - PLAY_AREA_HEIGHT / 2.0 + GRID_SIZE / 2.0;

                    commands.spawn((
                        FoodMarker {
                            grid_x: gx,
                            grid_y: gy,
                        },
                        Mesh2d(meshes.add(Circle::new(FOOD_CELL_RADIUS))),
                        MeshMaterial2d(
                            materials.add(ColorMaterial::from_color(Color::srgb(0.2, 0.8, 0.2))),
                        ),
                        Transform::from_xyz(world_x, world_y, 0.5),
                    ));
                }
            }
        }
    }
}
