//! Lightweight wall-clock instrumentation for the fixed-step chain.
//!
//! Disabled unless the `ANTS_PERF` environment variable is set to a non-zero
//! value (`ANTS_PERF=1`). When enabled, [`PerfPlugin`] wraps the whole
//! [`GameSet::Sim`] chain with two marker systems and reads the frame time
//! from Bevy's diagnostics; [`crate::ui::fps`] appends the numbers to the F12
//! panel. The per-tick cost is two `Instant::now()` calls and one `elapsed()`,
//! with no allocations.
//!
//! The module is declared at the crate root in `src/main.rs`, which also adds
//! [`PerfPlugin`] next to the feature plugins.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use std::time::Instant;

use crate::core::sets::GameSet;

/// Weight of the newest sample in [`PerfStats::smoothed_chain_secs`].
const CHAIN_SMOOTHING: f32 = 0.1;

/// Whether perf counters collect and display. `ANTS_PERF` set to anything
/// other than `0` or an empty string enables them; an absent variable means
/// off (the default).
#[derive(Resource)]
pub struct PerfEnabled(pub bool);

impl PerfEnabled {
    pub(crate) fn from_env() -> Self {
        let enabled =
            std::env::var("ANTS_PERF").is_ok_and(|value| !value.is_empty() && value != "0");

        Self(enabled)
    }
}

/// Wall-clock timings for the fixed-step chain and the rendered frame, in
/// seconds. Only updated while [`PerfEnabled`] holds.
#[derive(Resource, Default)]
pub struct PerfStats {
    /// Exponential moving average of the fixed-step chain wall time; equals
    /// the last sample until the second measured tick.
    pub smoothed_chain_secs: f32,
    /// Smoothed frame time from Bevy's `frame_time` diagnostic.
    pub frame_secs: f32,
    /// Fixed ticks measured so far.
    pub ticks: u64,
    /// Start of the chain currently being measured; `None` between ticks.
    start: Option<Instant>,
}

/// Measures the fixed-step chain and frame time; see the module docs.
pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerfStats>()
            .insert_resource(PerfEnabled::from_env())
            .add_systems(
                FixedUpdate,
                (
                    perf_chain_start.before(GameSet::Sim),
                    perf_chain_end.after(GameSet::Sim),
                )
                    .run_if(perf_enabled),
            )
            .add_systems(Update, perf_frame_time.run_if(perf_enabled));
    }
}

fn perf_enabled(enabled: Res<PerfEnabled>) -> bool {
    enabled.0
}

/// Runs immediately before [`GameSet::Sim`].
fn perf_chain_start(mut stats: ResMut<PerfStats>) {
    stats.start = Some(Instant::now());
}

/// Runs after every [`GameSet::Sim`] system and folds the elapsed wall time
/// into the smoothed stats.
fn perf_chain_end(mut stats: ResMut<PerfStats>) {
    let Some(start) = stats.start.take() else {
        return;
    };

    let secs = start.elapsed().as_secs_f32();
    stats.smoothed_chain_secs = if stats.ticks == 0 {
        secs
    } else {
        stats.smoothed_chain_secs + (secs - stats.smoothed_chain_secs) * CHAIN_SMOOTHING
    };
    stats.ticks += 1;
}

/// Read the smoothed frame time from Bevy's diagnostics once per frame.
fn perf_frame_time(mut stats: ResMut<PerfStats>, diagnostics: Option<Res<DiagnosticsStore>>) {
    let Some(diagnostics) = diagnostics else {
        return;
    };

    if let Some(millis) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|frame_time| frame_time.smoothed())
    {
        stats.frame_secs = (millis / 1000.0) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::world::{
        GRID_HEIGHT, GRID_WIDTH, NEST_X, NEST_Y, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH,
    };
    use crate::core::layers::Z_ANT;
    use crate::pheromone::PheromonePlugin;
    use crate::pheromone::grid::PheromoneGrid;
    use crate::simulation::Nest;
    use crate::simulation::ant::{Ant, AntPhase};
    use crate::simulation::{SimulationPlugin, food};
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    /// Headless smoke load: enough ants to be non-trivial, few enough to run
    /// in seconds even in a debug build.
    const SMOKE_ANTS: usize = 3_000;
    const SMOKE_TICKS: u32 = 40;

    #[derive(Resource, Default)]
    struct SmokeTicks(u32);

    fn count_smoke_ticks(mut count: ResMut<SmokeTicks>) {
        count.0 += 1;
    }

    fn manual_time(dt: f32) -> TimeUpdateStrategy {
        TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(dt))
    }

    fn smoke_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(PheromonePlugin)
            .add_plugins(PerfPlugin)
            .init_resource::<SmokeTicks>()
            .insert_resource(manual_time(1.0 / 64.0))
            .insert_resource(PerfEnabled(true))
            .add_systems(FixedUpdate, count_smoke_ticks)
            .add_systems(Startup, food::setup_food_patch);

        SimulationPlugin::add_fixed_step_systems(&mut app);

        let world = app.world_mut();
        world.spawn((Nest, Transform::from_xyz(NEST_X, NEST_Y, 0.0)));

        let mut rng = fastrand::Rng::with_seed(0xA11);
        for index in 0..SMOKE_ANTS {
            let mut ant = Ant::test_ant(rng.f32() * std::f32::consts::TAU);

            match index % 10 {
                0 => ant.phase = AntPhase::Nursing,
                1..=4 => {
                    ant.phase = AntPhase::Returning;
                    ant.energy = 0.2;
                }
                5 => ant.has_food = true,
                _ => {}
            }

            let position = Vec2::new(
                rng.f32() * PLAY_AREA_WIDTH - PLAY_AREA_WIDTH / 2.0,
                rng.f32() * PLAY_AREA_HEIGHT - PLAY_AREA_HEIGHT / 2.0,
            );
            world.spawn((ant, Transform::from_xyz(position.x, position.y, Z_ANT)));
        }

        // Warm a sparse trail so decay and diffusion do real work.
        let mut grid = world.resource_mut::<PheromoneGrid>();
        for y in (2..GRID_HEIGHT as u32).step_by(3) {
            for x in (2..GRID_WIDTH as u32).step_by(3) {
                grid.add_kernel(UVec2::new(x, y), 5.0, 5.0);
            }
        }

        app
    }

    #[test]
    fn perf_stats_measure_the_sim_chain_when_enabled() {
        #[derive(Resource, Default)]
        struct Busy(u32);

        fn busy(mut busy: ResMut<Busy>) {
            busy.0 += 1;
            std::thread::sleep(Duration::from_millis(1));
        }

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(PerfPlugin)
            .insert_resource(PerfEnabled(true))
            .init_resource::<Busy>()
            .add_systems(FixedUpdate, busy.in_set(GameSet::Sim))
            .insert_resource(manual_time(1.0 / 64.0));

        for _ in 0..2 {
            app.update();
        }

        let stats = app.world().resource::<PerfStats>();
        assert!(stats.ticks >= 1, "expected a measured tick");
        assert!(
            stats.smoothed_chain_secs >= 0.001,
            "chain time should cover the 1 ms busy system, got {} s",
            stats.smoothed_chain_secs
        );
    }

    #[test]
    fn perf_counters_stay_off_when_disabled() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(PerfPlugin)
            .insert_resource(PerfEnabled(false))
            .add_systems(FixedUpdate, (|| {}).in_set(GameSet::Sim))
            .insert_resource(manual_time(1.0 / 64.0));

        app.update();

        let stats = app.world().resource::<PerfStats>();
        assert_eq!(stats.ticks, 0);
        assert!(stats.start.is_none());
    }

    /// Runs the real fixed-step chain (simulation + pheromone) with the perf
    /// markers installed. Ignored by default; run manually with
    /// `cargo test --release -- --ignored --nocapture`.
    #[test]
    #[ignore = "perf smoke: run manually with cargo test --release -- --ignored --nocapture"]
    fn perf_smoke_chain_runs() {
        let mut app = smoke_app();

        for _ in 0..SMOKE_TICKS {
            app.update();
        }

        let ticks = app.world().resource::<SmokeTicks>().0;
        let stats = app.world().resource::<PerfStats>();

        println!(
            "perf smoke: {ticks} fixed ticks, {SMOKE_ANTS} ants, smoothed chain {:.3} ms, frame {:.3} ms",
            stats.smoothed_chain_secs * 1000.0,
            stats.frame_secs * 1000.0,
        );

        assert!(
            ticks >= SMOKE_TICKS - 1,
            "expected at least {} fixed ticks, got {ticks}",
            SMOKE_TICKS - 1
        );
        assert_eq!(
            stats.ticks,
            u64::from(ticks),
            "every tick should be measured"
        );
        assert!(
            stats.smoothed_chain_secs > 0.0 && stats.smoothed_chain_secs < 1.0,
            "chain wall time outside the generous sanity window: {} s",
            stats.smoothed_chain_secs
        );
    }
}
