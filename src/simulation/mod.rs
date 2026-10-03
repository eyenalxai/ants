//! Simulation world: camera, walls, nest, ants, food and movement.

pub mod ant;
pub mod collide;
pub mod colony;
pub mod density;
pub mod deposit;
pub mod food;
pub mod movement;

#[cfg(test)]
mod trail_tests;

use bevy::prelude::*;

use crate::constants::ant::ANT_SPAWN_INTERVAL;
use crate::constants::world::{
    NEST_RADIUS, NEST_X, NEST_Y, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH, WALL_THICKNESS,
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

impl SimulationPlugin {
    /// Register the explicit fixed-step chain:
    /// density -> lifetime/energy -> spawn -> collide -> move -> deposit ->
    /// food depletion. The deposit system itself lives in the pheromone plugin;
    /// ordering edges are attached here instead.
    pub(crate) fn add_fixed_step_systems(app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                density::rebuild_ant_density,
                ant::update_ant_energy_age,
                ant::spawn_ants,
                collide::check_collisions,
                movement::move_ants.before(deposit::deposit_pheromones),
                food::deplete_food.after(deposit::deposit_pheromones),
                food::update_food_visuals,
            )
                .chain()
                .in_set(GameSet::Sim),
        );
    }
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<food::FoodGrid>()
            .init_resource::<AntPopulation>()
            .init_resource::<colony::ColonyStats>()
            .init_resource::<density::AntDensity>()
            .insert_resource(AntSpawner {
                timer: Timer::from_seconds(ANT_SPAWN_INTERVAL, TimerMode::Repeating),
            })
            .add_systems(Startup, (setup_camera, spawn_world, food::setup_food_patch));

        Self::add_fixed_step_systems(app);
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

    commands.spawn((
        Nest,
        Mesh2d(meshes.add(Circle::new(NEST_RADIUS))),
        MeshMaterial2d(materials.add(ColorMaterial::from_color(Color::srgb(1.0, 0.0, 0.0)))),
        Transform::from_xyz(NEST_X, NEST_Y, Z_NEST),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pheromone::PheromonePlugin;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    #[derive(Resource, Default)]
    struct FixedStepCount(u32);

    fn count_fixed_steps(mut count: ResMut<FixedStepCount>) {
        count.0 += 1;
    }

    /// Headless schedule smoke test: builds the real fixed-step chain next to
    /// the pheromone plugin (which owns `deposit_pheromones`) and runs a few
    /// fixed ticks. Catches invalid cross-plugin ordering edges and system
    /// param panics without opening a window.
    #[test]
    fn fixed_step_chain_builds_and_runs_headless() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(PheromonePlugin)
            .init_resource::<food::FoodGrid>()
            .init_resource::<AntPopulation>()
            .init_resource::<colony::ColonyStats>()
            .init_resource::<density::AntDensity>()
            .init_resource::<FixedStepCount>()
            .add_systems(FixedUpdate, count_fixed_steps)
            .insert_resource(AntSpawner {
                timer: Timer::from_seconds(ANT_SPAWN_INTERVAL, TimerMode::Repeating),
            })
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / 32.0,
            )));

        SimulationPlugin::add_fixed_step_systems(&mut app);

        for _ in 0..4 {
            app.update();
        }

        assert!(app.world().resource::<FixedStepCount>().0 >= 1);
    }
}
