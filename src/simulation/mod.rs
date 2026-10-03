//! Simulation world: camera, walls, nest, ants, food and movement.

pub mod ant;
pub mod collide;
pub mod colony;
pub mod density;
pub mod deposit;
pub mod environment;
pub mod food;
pub mod lifecycle;
pub mod movement;
pub mod nest;

#[cfg(test)]
mod trail_tests;

use bevy::log::info;
use bevy::prelude::*;

use crate::constants::ant::{ANT_SPAWN_INTERVAL, MAX_ANTS};
use crate::constants::world::{
    NEST_RADIUS, NEST_X, NEST_Y, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH, WALL_THICKNESS,
};
use crate::core::layers::{Z_NEST, Z_WALL};
use crate::core::sets::{GameSet, SimSet};
use crate::perf::PerfEnabled;
use ant::{Ant, AntPopulation, AntSpawner};

/// Marker for the single nest entity.
#[derive(Component)]
pub struct Nest;

/// World-space nest centre, kept in sync with the [`Nest`] transform by
/// [`sync_nest_position`] at the start of every fixed step. Systems read this
/// instead of querying the nest entity directly.
#[derive(Resource, Default)]
pub struct NestPosition(pub Vec2);

/// Copy the nest transform into [`NestPosition`]. A missing nest leaves the
/// last known position in place.
fn sync_nest_position(
    nest_query: Query<&Transform, With<Nest>>,
    mut nest_position: ResMut<NestPosition>,
) {
    if let Ok(transform) = nest_query.single() {
        nest_position.0 = Vec2::new(transform.translation.x, transform.translation.y);
    }
}

/// Owns the simulation resources, startup spawns and the fixed-step chain.
pub struct SimulationPlugin;

/// Initialize every resource the simulation reads: the food grid, the ant
/// population and colony economy, the density grid, the nest position and
/// geometry, the environment clock, the fixed clock and the spawner.
///
/// Headless harnesses get this through
/// [`SimulationPlugin::add_fixed_step_systems`]; tests may override any
/// resource afterwards (e.g. a stocked [`colony::NestStore`] or a shorter
/// [`AntSpawner`] timer), because `init_resource` never clobbers an explicit
/// insert.
pub(crate) fn register_sim_resources(app: &mut App) {
    app.init_resource::<food::FoodGrid>()
        .init_resource::<AntPopulation>()
        .init_resource::<colony::ColonyStats>()
        .init_resource::<colony::NestStore>()
        .init_resource::<density::AntDensity>()
        .init_resource::<NestPosition>()
        .init_resource::<environment::SimClock>()
        .init_resource::<nest::NestGeometry>()
        // Bevy's default fixed timestep is 64 Hz; an explicit
        // `insert_resource(Time::<Fixed>::from_hz(..))` still wins.
        .init_resource::<Time<Fixed>>()
        .insert_resource(AntSpawner {
            timer: Timer::from_seconds(ANT_SPAWN_INTERVAL, TimerMode::Repeating),
        });
}

impl SimulationPlugin {
    /// Register the simulation resources and the fixed-step chain.
    ///
    /// [`SimSet`] declares the order once — `Clock -> NestSync -> Density ->
    /// Lifecycle -> Spawn -> Collide -> Delivery -> Move -> Deposit -> Decay ->
    /// Deplete -> Visuals` — and every system is placed in its set where it is
    /// registered, so no cross-plugin ordering edges exist. Headless tests
    /// call this without [`SimulationPlugin::build`], which is why it also
    /// initializes the resources via [`register_sim_resources`].
    pub(crate) fn add_fixed_step_systems(app: &mut App) {
        register_sim_resources(app);

        app.configure_sets(
            FixedUpdate,
            (
                SimSet::Clock,
                SimSet::NestSync,
                SimSet::Density,
                SimSet::Lifecycle,
                SimSet::Spawn,
                SimSet::Collide,
                SimSet::Delivery,
                SimSet::Move,
                SimSet::Deposit,
                SimSet::Decay,
                SimSet::Deplete,
                SimSet::Visuals,
            )
                .chain()
                .in_set(GameSet::Sim),
        )
        .add_systems(
            FixedUpdate,
            (
                sync_nest_position.in_set(SimSet::NestSync),
                density::rebuild_ant_density.in_set(SimSet::Density),
                ant::update_ant_energy_age.in_set(SimSet::Lifecycle),
                ant::spawn_ants.in_set(SimSet::Spawn),
                collide::check_collisions.in_set(SimSet::Collide),
                colony::tick_delivery_ema.in_set(SimSet::Delivery),
                (
                    capture_prev_positions.before(movement::move_ants),
                    movement::move_ants,
                )
                    .in_set(SimSet::Move),
                deposit::deposit_pheromones.in_set(SimSet::Deposit),
                food::deplete_food.in_set(SimSet::Deplete),
                food::update_food_visuals.in_set(SimSet::Visuals),
            ),
        );
    }
}

/// Capture every ant's position before [`movement::move_ants`] rewrites it.
///
/// [`Ant::prev_pos`] is the honest start of this tick's movement segment:
/// `move_ants` bounces off walls after moving and may rotate `direction`, so
/// the segment cannot be reconstructed from the post-move heading. The deposit
/// pass should read `prev_pos` (owned by the pheromone stream).
fn capture_prev_positions(mut ant_query: Query<(&mut Ant, &Transform)>) {
    for (mut ant, transform) in &mut ant_query {
        ant.prev_pos = Vec2::new(transform.translation.x, transform.translation.y);
    }
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        register_sim_resources(app);
        lifecycle::register(app);
        environment::register(app);
        nest::register(app);

        let hz = app
            .world()
            .resource::<Time<Fixed>>()
            .timestep()
            .as_secs_f32()
            .recip();
        let perf_enabled = app
            .world()
            .get_resource::<PerfEnabled>()
            .map_or_else(|| PerfEnabled::from_env().0, |enabled| enabled.0);
        info!(
            "ants: play area {PLAY_AREA_WIDTH}x{PLAY_AREA_HEIGHT} u, max {MAX_ANTS} ants, \
             fixed {hz:.0} Hz, ANTS_PERF {}",
            if perf_enabled { "on" } else { "off" }
        );

        app.add_systems(Startup, (setup_camera, spawn_world, food::setup_food_patch));

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
        Sprite {
            color: wall_color,
            custom_size: Some(Vec2::new(WALL_THICKNESS, PLAY_AREA_HEIGHT)),
            ..default()
        },
        Transform::from_xyz(half_width + WALL_THICKNESS / 2.0, 0.0, Z_WALL),
    ));

    commands.spawn((
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
    /// the pheromone plugin (which owns decay) and runs a few fixed ticks.
    /// Catches invalid ordering edges and system param panics without opening
    /// a window.
    #[test]
    fn fixed_step_chain_builds_and_runs_headless() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(PheromonePlugin)
            .init_resource::<FixedStepCount>()
            .add_systems(FixedUpdate, count_fixed_steps)
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
