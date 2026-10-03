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

use bevy::log::{info, warn_once};
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

/// Authoritative world-space nest centre.
///
/// The editor writes it through [`NestPosition::set`] and
/// [`sync_nest_transform`] mirrors it into the [`Nest`] transform every frame
/// (also while paused), so the resource is never stale. Systems read this
/// instead of querying the nest entity directly.
#[derive(Resource)]
pub struct NestPosition(pub Vec2);

impl Default for NestPosition {
    fn default() -> Self {
        Self(Vec2::new(NEST_X, NEST_Y))
    }
}

impl NestPosition {
    /// `pos` clamped so the whole nest circle stays inside the play area.
    pub fn clamped(pos: Vec2) -> Vec2 {
        let limit = Vec2::new(
            PLAY_AREA_WIDTH / 2.0 - NEST_RADIUS,
            PLAY_AREA_HEIGHT / 2.0 - NEST_RADIUS,
        );

        pos.clamp(-limit, limit)
    }

    /// Move the nest, clamped to the play area. This is the only supported
    /// writer.
    pub fn set(&mut self, pos: Vec2) {
        self.0 = Self::clamped(pos);
    }
}

/// Write the [`Nest`] transform from the authoritative [`NestPosition`].
///
/// Runs in `Update` after the editor set, so a nest drag shows up in the same
/// frame and the transform stays correct while paused (fixed steps stop).
fn sync_nest_transform(
    nest_position: Res<NestPosition>,
    mut nest_query: Query<&mut Transform, With<Nest>>,
) {
    let Ok(mut transform) = nest_query.single_mut() else {
        warn_once!("nest transform sync skipped: no nest entity");
        return;
    };

    let target = nest_position.0;

    if transform.translation.truncate() != target {
        transform.translation.x = target.x;
        transform.translation.y = target.y;
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
    /// Register the simulation resources, the stream wiring and the fixed-step
    /// chain.
    ///
    /// [`SimSet`] declares the order once — `Clock -> NestSync -> Density ->
    /// Lifecycle -> Spawn -> Collide -> Delivery -> Move -> Deposit -> Decay ->
    /// Deplete -> Visuals` — and every system is placed in its set where it is
    /// registered, so no cross-plugin ordering edges exist. Headless tests
    /// call this without [`SimulationPlugin::build`], which is why it also
    /// initializes the resources via [`register_sim_resources`] and wires the
    /// environment clock, nest geometry and lifecycle/task-pool systems.
    /// Registering a stream hook here means it runs in every harness too.
    pub(crate) fn add_fixed_step_systems(app: &mut App) {
        register_sim_resources(app);

        // Stream hooks that belong to the simulation chain. They must be
        // registered exactly once, so `build` leaves them to this method.
        lifecycle::register(app);
        environment::register(app);
        nest::register(app);

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
        // The nest transform follows the authoritative `NestPosition` in
        // `Update` (not in the fixed chain), so it is correct while paused.
        app.add_systems(Update, sync_nest_transform.after(GameSet::Editor));

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
    use crate::simulation::ant::AntRng;
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

    #[test]
    fn nest_position_set_clamps_to_the_play_area() {
        let limit = Vec2::new(
            PLAY_AREA_WIDTH / 2.0 - NEST_RADIUS,
            PLAY_AREA_HEIGHT / 2.0 - NEST_RADIUS,
        );
        let mut nest = NestPosition::default();
        assert_eq!(nest.0, Vec2::new(NEST_X, NEST_Y));

        nest.set(Vec2::new(1e6, -1e6));
        assert_eq!(nest.0, Vec2::new(limit.x, -limit.y));

        nest.set(Vec2::new(3.0, 4.0));
        assert_eq!(nest.0, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn nest_transform_follows_the_authoritative_position() {
        let mut app = App::new();
        app.init_resource::<NestPosition>()
            .add_systems(Update, sync_nest_transform);

        let nest = app
            .world_mut()
            .spawn((Nest, Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();

        app.world_mut()
            .resource_mut::<NestPosition>()
            .set(Vec2::new(12.0, -34.0));
        app.update();

        assert_eq!(
            app.world()
                .get::<Transform>(nest)
                .unwrap()
                .translation
                .truncate(),
            Vec2::new(12.0, -34.0)
        );
    }

    /// The Move set must capture `prev_pos` before `move_ants` rewrites the
    /// transform, so the deposit pass can reconstruct the real segment.
    #[test]
    fn move_set_captures_the_pre_move_position() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(PheromonePlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / 64.0,
            )));
        SimulationPlugin::add_fixed_step_systems(&mut app);

        let start = Vec2::new(10.0, -5.0);
        let ant = Ant::test_ant(0.0);
        let entity = app
            .world_mut()
            .spawn((
                ant,
                AntRng::for_spawn(0),
                Transform::from_xyz(start.x, start.y, 0.0),
            ))
            .id();

        app.update();
        app.update();

        let ant = app.world().get::<Ant>(entity).unwrap();
        let transform = app.world().get::<Transform>(entity).unwrap();

        assert_eq!(
            ant.prev_pos, start,
            "prev_pos must be the pre-move position"
        );
        assert!(
            transform.translation.x > start.x,
            "the ant must have moved in +x, position {:?}",
            transform.translation
        );
    }
}
