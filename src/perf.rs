//! Lightweight wall-clock instrumentation for the fixed-step chain and the
//! render schedule.
//!
//! Disabled unless the `ANTS_PERF` environment variable is set to a non-zero
//! value (`ANTS_PERF=1`). When enabled, [`PerfPlugin`] wraps the whole
//! [`GameSet::Sim`] chain with two marker systems and reads the frame time
//! from Bevy's diagnostics; [`crate::ui::fps`] appends the numbers to the F12
//! panel. The per-tick cost is two `Instant::now()` calls and one `elapsed()`,
//! with no allocations.
//!
//! # In-app render profile
//!
//! Run the GUI with the counters on and press F12:
//!
//! ```sh
//! ANTS_PERF=1 cargo run --release
//! ```
//!
//! The panel line reads
//! `sim 6.27 ms | frame 15.00 ms | render 3.42 ms (ex 1.21 q 1.26 s 0.37 p 0.13 g 0.28)`:
//!
//! - `render` — wall time of the whole `Render` schedule in the render world,
//!   from just before [`RenderSystems::ExtractCommands`] to just after
//!   [`RenderSystems::PostCleanup`]. **This is the number to compare at ~30k
//!   ants** when deciding whether the batched-mesh refactor is worth it
//!   (the F4 audit measured ≈3.4 ms/frame of render-schedule CPU).
//! - `ex` — `extract_sprites` in the `ExtractSchedule`
//!   ([`SpriteSystems::ExtractSprites`]). Bevy 0.19.1 has **no
//!   `RenderSystems::Extract` set**: extraction is a separate schedule run by
//!   `render_app.extract()` and has no global set to wrap, so this marker pair
//!   times the per-sprite system itself, which dominates extraction at 30k
//!   sprites.
//! - `q`, `s`, `p`, `g` — the [`RenderSystems::Queue`],
//!   [`RenderSystems::PhaseSort`], [`RenderSystems::Prepare`] and
//!   [`RenderSystems::Render`] sets.
//!
//! The `render` total excludes the main-world transform propagation (the other
//! large item in the F4 breakdown) and all GPU time. With pipelined rendering
//! the values describe the last completed render frame, about one frame behind
//! the main world; every value shown is exponentially smoothed.
//!
//! # How it is wired
//!
//! `bevy_render` 0.19.1 moves the render world to the pipelined render thread,
//! so a render-world resource cannot be read from the main world directly.
//! Following `bevy_render`'s own `RenderDiagnosticsMutex` pattern,
//! [`RenderPerfShared`] holds an `Arc<Mutex<RenderPerfFrame>>` inserted into
//! both worlds: marker systems in the render world write completed stage times
//! into it and a main-world `PreUpdate` system copies them into [`PerfStats`].
//! The marker systems are registered only when `ANTS_PERF=1`, so a disabled
//! run has no render instrumentation at all (no allocations, no timing calls).
//!
//! The headless smoke test builds the same marker graph on a fake render
//! sub-app and exercises it without a GPU:
//!
//! ```sh
//! cargo test --release -- --ignored --nocapture perf_render
//! ```

use bevy::app::SubApp;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::sprite_render::SpriteSystems;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::core::sets::GameSet;

/// Weight of the newest sample in every smoothed [`PerfStats`] value.
const CHAIN_SMOOTHING: f32 = 0.1;

/// Slots of [`RenderPerfStarts::starts`].
mod slot {
    /// `extract_sprites` in the `ExtractSchedule`.
    pub(super) const EXTRACT: usize = 0;
    /// [`RenderSystems::Queue`].
    pub(super) const QUEUE: usize = 1;
    /// [`RenderSystems::PhaseSort`].
    pub(super) const SORT: usize = 2;
    /// [`RenderSystems::Prepare`].
    pub(super) const PREPARE: usize = 3;
    /// [`RenderSystems::Render`].
    pub(super) const DRAW: usize = 4;
    /// The whole `Render` schedule.
    pub(super) const TOTAL: usize = 5;
    /// Number of slots.
    pub(super) const COUNT: usize = 6;
}

/// One completed render frame's stage wall times, in seconds.
///
/// The render world fills the fields as the frame progresses and only bumps
/// [`Self::frames`] in the total-end marker, after every other field of the
/// frame has been written; the main world therefore never reads a partial
/// frame.
#[derive(Clone, Copy, Default)]
struct RenderPerfFrame {
    extract_secs: f32,
    queue_secs: f32,
    sort_secs: f32,
    prepare_secs: f32,
    draw_secs: f32,
    total_secs: f32,
    /// Completed render frames recorded so far.
    frames: u64,
}

/// Channel-free handoff for the render timings.
///
/// The same `Arc` is inserted into the main world and the render world, the
/// pattern `bevy_render` uses for its own `RenderDiagnosticsMutex`. Locking is
/// the only synchronisation; there are no channels and no per-frame
/// allocations.
#[derive(Resource, Clone, Default)]
pub struct RenderPerfShared(Arc<Mutex<RenderPerfFrame>>);

/// Render-world start instants for the stage markers. Render-world only; the
/// completed durations travel through [`RenderPerfShared`].
#[derive(Resource, Default)]
struct RenderPerfStarts {
    starts: [Option<Instant>; slot::COUNT],
}

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

/// Wall-clock timings for the fixed-step chain, the rendered frame and the
/// render schedule, in seconds. Only updated while [`PerfEnabled`] holds.
#[derive(Resource, Default)]
pub struct PerfStats {
    /// Exponential moving average of the fixed-step chain wall time; equals
    /// the last sample until the second measured tick.
    pub smoothed_chain_secs: f32,
    /// Smoothed frame time from Bevy's `frame_time` diagnostic.
    pub frame_secs: f32,
    /// Smoothed wall time of the whole `Render` schedule (render world).
    pub smoothed_render_secs: f32,
    /// Smoothed wall time of `extract_sprites` in the `ExtractSchedule`.
    pub smoothed_render_extract_secs: f32,
    /// Smoothed wall time of the [`RenderSystems::Queue`] set.
    pub smoothed_render_queue_secs: f32,
    /// Smoothed wall time of the [`RenderSystems::PhaseSort`] set.
    pub smoothed_render_sort_secs: f32,
    /// Smoothed wall time of the [`RenderSystems::Prepare`] set.
    pub smoothed_render_prepare_secs: f32,
    /// Smoothed wall time of the [`RenderSystems::Render`] set.
    pub smoothed_render_draw_secs: f32,
    /// Fixed ticks measured so far.
    pub ticks: u64,
    /// Completed render frames folded in so far.
    pub render_frames: u64,
    /// Start of the chain currently being measured; `None` between ticks.
    start: Option<Instant>,
}

/// Measures the fixed-step chain, the frame time and the render schedule; see
/// the module docs.
pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        let enabled = PerfEnabled::from_env().0;

        app.init_resource::<PerfStats>()
            .insert_resource(PerfEnabled(enabled))
            .add_systems(
                FixedUpdate,
                (
                    perf_chain_start.before(GameSet::Sim),
                    perf_chain_end.after(GameSet::Sim),
                )
                    .run_if(perf_enabled),
            )
            .add_systems(Update, perf_frame_time.run_if(perf_enabled))
            .add_systems(PreUpdate, perf_render_sync.run_if(perf_enabled));

        if enabled {
            install_render_perf_plugin(app);
        }
    }
}

/// Share one [`RenderPerfShared`] with the render world and register the stage
/// markers there. Only called when `ANTS_PERF=1`; a headless app (no
/// [`RenderApp`]) keeps the main-world handle but installs nothing.
fn install_render_perf_plugin(app: &mut App) {
    let shared = RenderPerfShared::default();
    app.insert_resource(shared.clone());

    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        install_render_perf(render_app, shared);
    }
}

/// Registers the render-world stage markers and the shared cell.
///
/// The render app must already have the `ExtractSchedule` and the base
/// `Render` schedule (both added by `RenderPlugin`'s `ExtractPlugin`), with
/// [`SpriteSystems::ExtractSprites`] configured in `ExtractSchedule` (added by
/// `SpritePlugin`). All of them exist by the time `PerfPlugin` is built after
/// `DefaultPlugins`.
fn install_render_perf(render_app: &mut SubApp, shared: RenderPerfShared) {
    render_app
        .insert_resource(shared)
        .init_resource::<RenderPerfStarts>()
        .add_systems(
            ExtractSchedule,
            (
                extract_sprites_start.before(SpriteSystems::ExtractSprites),
                extract_sprites_end.after(SpriteSystems::ExtractSprites),
            )
                .chain(),
        )
        .add_systems(
            Render,
            (
                render_total_start.before(RenderSystems::ExtractCommands),
                queue_start.before(RenderSystems::Queue),
                queue_end.after(RenderSystems::Queue),
                sort_start.before(RenderSystems::PhaseSort),
                sort_end.after(RenderSystems::PhaseSort),
                prepare_start.before(RenderSystems::Prepare),
                prepare_end.after(RenderSystems::Prepare),
                draw_start.before(RenderSystems::Render),
                draw_end.after(RenderSystems::Render),
                render_total_end.after(RenderSystems::PostCleanup),
            )
                .chain(),
        );
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
    stats.smoothed_chain_secs = smooth(stats.smoothed_chain_secs, secs, stats.ticks == 0);
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

/// Copy the last completed render frame from the shared cell into
/// [`PerfStats`], smoothing every stage. Runs in `PreUpdate` so the F12 panel
/// sees fresh numbers in the same frame.
fn perf_render_sync(shared: Option<Res<RenderPerfShared>>, mut stats: ResMut<PerfStats>) {
    let Some(shared) = shared else {
        return;
    };
    let Ok(frame) = shared.0.lock() else {
        return;
    };

    if frame.frames == stats.render_frames {
        return;
    }

    let first = stats.render_frames == 0;
    stats.smoothed_render_secs = smooth(stats.smoothed_render_secs, frame.total_secs, first);
    stats.smoothed_render_extract_secs = smooth(
        stats.smoothed_render_extract_secs,
        frame.extract_secs,
        first,
    );
    stats.smoothed_render_queue_secs =
        smooth(stats.smoothed_render_queue_secs, frame.queue_secs, first);
    stats.smoothed_render_sort_secs =
        smooth(stats.smoothed_render_sort_secs, frame.sort_secs, first);
    stats.smoothed_render_prepare_secs = smooth(
        stats.smoothed_render_prepare_secs,
        frame.prepare_secs,
        first,
    );
    stats.smoothed_render_draw_secs =
        smooth(stats.smoothed_render_draw_secs, frame.draw_secs, first);
    stats.render_frames = frame.frames;
}

/// Exponential moving average: the first sample is taken as-is.
fn smooth(current: f32, sample: f32, first: bool) -> f32 {
    if first {
        sample
    } else {
        current + (sample - current) * CHAIN_SMOOTHING
    }
}

/// Record `Instant::now()` into `starts[slot]`.
fn mark_start(starts: &mut RenderPerfStarts, slot: usize) {
    starts.starts[slot] = Some(Instant::now());
}

/// Take the elapsed time since `starts[slot]` and hand it to `write` under the
/// shared lock. A missing start records 0.0.
fn mark_end(
    starts: &mut RenderPerfStarts,
    shared: &RenderPerfShared,
    slot: usize,
    write: impl FnOnce(&mut RenderPerfFrame, f32),
) {
    let secs = starts.starts[slot]
        .take()
        .map_or(0.0, |start| start.elapsed().as_secs_f32());

    if let Ok(mut frame) = shared.0.lock() {
        write(&mut frame, secs);
    }
}

fn extract_sprites_start(mut starts: ResMut<RenderPerfStarts>) {
    mark_start(&mut starts, slot::EXTRACT);
}

fn extract_sprites_end(mut starts: ResMut<RenderPerfStarts>, shared: Res<RenderPerfShared>) {
    mark_end(&mut starts, &shared, slot::EXTRACT, |frame, secs| {
        frame.extract_secs = secs;
    });
}

fn render_total_start(mut starts: ResMut<RenderPerfStarts>) {
    mark_start(&mut starts, slot::TOTAL);
}

fn queue_start(mut starts: ResMut<RenderPerfStarts>) {
    mark_start(&mut starts, slot::QUEUE);
}

fn queue_end(mut starts: ResMut<RenderPerfStarts>, shared: Res<RenderPerfShared>) {
    mark_end(&mut starts, &shared, slot::QUEUE, |frame, secs| {
        frame.queue_secs = secs;
    });
}

fn sort_start(mut starts: ResMut<RenderPerfStarts>) {
    mark_start(&mut starts, slot::SORT);
}

fn sort_end(mut starts: ResMut<RenderPerfStarts>, shared: Res<RenderPerfShared>) {
    mark_end(&mut starts, &shared, slot::SORT, |frame, secs| {
        frame.sort_secs = secs;
    });
}

fn prepare_start(mut starts: ResMut<RenderPerfStarts>) {
    mark_start(&mut starts, slot::PREPARE);
}

fn prepare_end(mut starts: ResMut<RenderPerfStarts>, shared: Res<RenderPerfShared>) {
    mark_end(&mut starts, &shared, slot::PREPARE, |frame, secs| {
        frame.prepare_secs = secs;
    });
}

fn draw_start(mut starts: ResMut<RenderPerfStarts>) {
    mark_start(&mut starts, slot::DRAW);
}

fn draw_end(mut starts: ResMut<RenderPerfStarts>, shared: Res<RenderPerfShared>) {
    mark_end(&mut starts, &shared, slot::DRAW, |frame, secs| {
        frame.draw_secs = secs;
    });
}

fn render_total_end(mut starts: ResMut<RenderPerfStarts>, shared: Res<RenderPerfShared>) {
    mark_end(&mut starts, &shared, slot::TOTAL, |frame, secs| {
        frame.total_secs = secs;
        frame.frames += 1;
    });
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
    use crate::simulation::ant::{Ant, AntPhase, AntRng};
    use crate::simulation::{SimulationPlugin, food};
    use bevy::ecs::schedule::Schedule;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    /// Headless smoke load: enough ants to be non-trivial, few enough to run
    /// in seconds even in a debug build.
    const SMOKE_ANTS: usize = 3_000;
    const SMOKE_TICKS: u32 = 40;
    const SMOKE_RENDER_FRAMES: u32 = 40;

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
            world.spawn((
                ant,
                AntRng::for_spawn(index as u64),
                Transform::from_xyz(position.x, position.y, Z_ANT),
            ));
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

    /// A render sub-app with the same schedule structure as `RenderPlugin`'s
    /// but no GPU work: `Render::base_schedule` plus one no-op system per
    /// timed set, so the marker graph can be built and run headlessly.
    fn fake_render_app() -> SubApp {
        let mut render_app = SubApp::new();
        render_app
            .add_schedule(Render::base_schedule())
            .add_schedule(Schedule::new(ExtractSchedule))
            .add_systems(
                ExtractSchedule,
                (|| {}).in_set(SpriteSystems::ExtractSprites),
            )
            .add_systems(
                Render,
                (
                    (|| {}).in_set(RenderSystems::ExtractCommands),
                    (|| {}).in_set(RenderSystems::Queue),
                    (|| {}).in_set(RenderSystems::PhaseSort),
                    (|| {}).in_set(RenderSystems::Prepare),
                    (|| {}).in_set(RenderSystems::Render),
                    (|| {}).in_set(RenderSystems::PostCleanup),
                ),
            );
        render_app
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

    /// The render-world markers fold each stage into the shared frame; the
    /// total-end marker is the only one that bumps the frame counter, so the
    /// main world can never read a partial frame.
    #[test]
    fn render_markers_fold_stage_times_into_the_shared_frame() {
        let mut world = World::new();
        world.init_resource::<RenderPerfStarts>();
        let shared = RenderPerfShared::default();
        world.insert_resource(shared.clone());

        world.run_system_once(render_total_start).unwrap();
        world.run_system_once(queue_start).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        world.run_system_once(queue_end).unwrap();

        {
            let frame = shared.0.lock().unwrap();
            assert_eq!(
                frame.frames, 0,
                "frame is not complete before the total end"
            );
            assert!(
                frame.queue_secs >= 0.002,
                "queue must cover the 2 ms sleep, got {} s",
                frame.queue_secs
            );
        }

        world.run_system_once(prepare_start).unwrap();
        world.run_system_once(prepare_end).unwrap();
        world.run_system_once(render_total_end).unwrap();

        let frame = *shared.0.lock().unwrap();
        assert_eq!(frame.frames, 1);
        assert!(frame.total_secs >= frame.queue_secs);
        assert!(frame.prepare_secs >= 0.0);
    }

    /// The full marker graph must build and run against the real Bevy set
    /// names on a headless render sub-app (no GPU, no window).
    #[test]
    fn render_markers_build_and_run_headless() {
        let mut render_app = fake_render_app();
        let shared = RenderPerfShared::default();
        install_render_perf(&mut render_app, shared.clone());

        for _ in 0..3 {
            render_app.world_mut().run_schedule(ExtractSchedule);
            render_app.world_mut().run_schedule(Render);
        }

        let frame = *shared.0.lock().unwrap();
        assert_eq!(frame.frames, 3, "one completed render frame per run");
        assert!(frame.total_secs >= 0.0 && frame.total_secs < 1.0);
        assert!(frame.extract_secs >= 0.0 && frame.extract_secs < 1.0);
    }

    /// `install_render_perf_plugin` must put one shared cell into the main
    /// world and the render world, and register the markers there.
    #[test]
    fn install_render_perf_plugin_shares_one_cell_with_the_render_world() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_sub_app(RenderApp, fake_render_app());

        install_render_perf_plugin(&mut app);

        assert!(app.world().get_resource::<RenderPerfShared>().is_some());

        let render_app = app.get_sub_app_mut(RenderApp).unwrap();
        assert!(
            render_app
                .world()
                .get_resource::<RenderPerfStarts>()
                .is_some()
        );
        render_app.world_mut().run_schedule(ExtractSchedule);
        render_app.world_mut().run_schedule(Render);

        let shared = app.world().resource::<RenderPerfShared>();
        assert_eq!(shared.0.lock().unwrap().frames, 1);
    }

    /// The main-world sync folds a new shared frame exactly once and smooths
    /// later frames instead of jumping to them.
    #[test]
    fn perf_render_sync_folds_each_frame_once_and_smooths() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(PerfEnabled(true))
            .insert_resource(RenderPerfShared::default())
            .init_resource::<PerfStats>()
            .add_systems(PreUpdate, perf_render_sync.run_if(perf_enabled));

        let shared = app.world().resource::<RenderPerfShared>().clone();
        {
            let mut frame = shared.0.lock().unwrap();
            frame.total_secs = 0.003;
            frame.queue_secs = 0.001;
            frame.frames = 1;
        }

        app.update();

        let stats = app.world().resource::<PerfStats>();
        assert_eq!(stats.render_frames, 1);
        assert!((stats.smoothed_render_secs - 0.003).abs() < 1e-6);
        assert!((stats.smoothed_render_queue_secs - 0.001).abs() < 1e-6);

        // The same completed frame must not be folded twice.
        app.update();
        assert_eq!(
            app.world().resource::<PerfStats>().smoothed_render_secs,
            0.003
        );

        // A new frame moves the EMA part of the way.
        {
            let mut frame = shared.0.lock().unwrap();
            frame.total_secs = 0.005;
            frame.queue_secs = 0.003;
            frame.frames = 2;
        }
        app.update();

        let stats = app.world().resource::<PerfStats>();
        assert_eq!(stats.render_frames, 2);
        assert!(stats.smoothed_render_secs > 0.003 && stats.smoothed_render_secs < 0.005);
        assert!(
            stats.smoothed_render_queue_secs > 0.001 && stats.smoothed_render_queue_secs < 0.003
        );
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

    /// Headless render-instrumentation smoke: verifies that the sync is inert
    /// while disabled, that a disabled app installs no render instrumentation,
    /// and that the marker graph runs without panicking on a fake render
    /// sub-app. The full GPU `Render` schedule cannot run headless, so this
    /// exercises the schedule structure and marker wiring only. Ignored by
    /// default; run with
    /// `cargo test --release -- --ignored --nocapture perf_render`.
    #[test]
    #[ignore = "perf smoke: run manually with cargo test --release -- --ignored --nocapture"]
    fn perf_render_smoke_headless() {
        // Disabled: a primed shared cell must not be folded into PerfStats.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<PerfStats>()
            .insert_resource(PerfEnabled(false))
            .insert_resource(RenderPerfShared::default())
            .add_systems(PreUpdate, perf_render_sync.run_if(perf_enabled));
        {
            let shared = app.world().resource::<RenderPerfShared>();
            let mut frame = shared.0.lock().unwrap();
            frame.total_secs = 0.123;
            frame.frames = 7;
        }
        for _ in 0..3 {
            app.update();
        }
        let stats = app.world().resource::<PerfStats>();
        assert_eq!(stats.render_frames, 0, "disabled sync must be inert");
        assert_eq!(stats.smoothed_render_secs, 0.0);

        // Disabled plugin: no render instrumentation resources are installed.
        let bare = fake_render_app();
        assert!(bare.world().get_resource::<RenderPerfStarts>().is_none());
        assert!(bare.world().get_resource::<RenderPerfShared>().is_none());

        // Enabled: the marker graph runs on the fake render sub-app.
        let mut render_app = fake_render_app();
        let shared = RenderPerfShared::default();
        install_render_perf(&mut render_app, shared.clone());

        for _ in 0..SMOKE_RENDER_FRAMES {
            render_app.world_mut().run_schedule(ExtractSchedule);
            render_app.world_mut().run_schedule(Render);
        }

        let frame = *shared.0.lock().unwrap();
        println!(
            "perf render smoke: {} frames, total {:.3} ms (ex {:.3} q {:.3} s {:.3} p {:.3} g {:.3})",
            frame.frames,
            frame.total_secs * 1000.0,
            frame.extract_secs * 1000.0,
            frame.queue_secs * 1000.0,
            frame.sort_secs * 1000.0,
            frame.prepare_secs * 1000.0,
            frame.draw_secs * 1000.0,
        );

        assert_eq!(frame.frames, u64::from(SMOKE_RENDER_FRAMES));
        assert!(
            frame.total_secs >= 0.0 && frame.total_secs < 1.0,
            "render total outside the generous sanity window: {} s",
            frame.total_secs
        );
        assert!(frame.total_secs >= frame.queue_secs);
    }
}
