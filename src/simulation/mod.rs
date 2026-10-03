//! Simulation world: camera, walls, nest, ants, food and movement.

pub mod ant;
pub mod collide;
pub mod deposit;
pub mod food;
pub mod movement;

use bevy::prelude::*;

use crate::constants::ant::ANT_SPAWN_INTERVAL;
use crate::constants::world::{
    NEST_SIZE, NEST_X, NEST_Y, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH, WALL_THICKNESS,
};
use crate::core::layers::{Z_NEST, Z_WALL};
use crate::core::sets::GameSet;
use ant::{AntPopulation, AntSpawner};

/// Marker for the single nest entity.
#[derive(Component)]
pub struct Nest;

/// Marker for the static play-area walls.
#[derive(Component)]
pub struct Wall;

/// Owns the simulation resources, startup spawns and the fixed-step chain.
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<food::FoodGrid>()
            .init_resource::<AntPopulation>()
            .insert_resource(AntSpawner {
                timer: Timer::from_seconds(ANT_SPAWN_INTERVAL, TimerMode::Repeating),
                count: 0,
            })
            .add_systems(Startup, (setup_camera, spawn_world, food::setup_food_patch))
            .add_systems(
                FixedUpdate,
                (
                    ant::update_ant_lifetime,
                    ant::spawn_ants,
                    movement::move_ants,
                    collide::check_collisions,
                    food::deplete_food,
                    food::update_food_visuals,
                )
                    .chain()
                    .in_set(GameSet::Sim),
            );
    }
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.1, 0.1, 0.1)),
            ..default()
        },
    ));
}

fn spawn_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
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
        Transform::from_xyz(0.0, half_height + WALL_THICKNESS / 2.0, Z_WALL),
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
        Transform::from_xyz(0.0, -half_height - WALL_THICKNESS / 2.0, Z_WALL),
    ));

    commands.spawn((
        Wall,
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(WALL_THICKNESS, PLAY_AREA_HEIGHT)),
            ..default()
        },
        Transform::from_xyz(half_width + WALL_THICKNESS / 2.0, 0.0, Z_WALL),
    ));

    commands.spawn((
        Wall,
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(WALL_THICKNESS, PLAY_AREA_HEIGHT)),
            ..default()
        },
        Transform::from_xyz(-half_width - WALL_THICKNESS / 2.0, 0.0, Z_WALL),
    ));

    let nest_radius = NEST_SIZE / 4.0;
    commands.spawn((
        Nest,
        Mesh2d(meshes.add(Circle::new(nest_radius))),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgb(1.0, 0.0, 0.0)))),
        Transform::from_xyz(NEST_X, NEST_Y, Z_NEST),
    ));
}
