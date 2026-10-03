//! Shared scheduling sets and global run state.

use bevy::prelude::*;

/// Top-level scheduling sets shared by all feature plugins.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameSet {
    /// Simulation systems, stepped in `FixedUpdate` through the [`SimSet`]
    /// chain.
    Sim,
    /// Editor input/state systems, run in `Update` (also while paused).
    Editor,
    /// Debug overlays, run in `Update`.
    Overlay,
    /// HUD/UI systems, run in `Update`.
    Ui,
}

/// Fixed-step sub-schedule, run in `FixedUpdate` in exactly this chain order.
///
/// The chain is configured once in
/// [`crate::simulation::SimulationPlugin::add_fixed_step_systems`] and is the
/// single ordering contract for the simulation: systems are placed in their
/// set where they are registered, so no cross-plugin `.before`/`.after` edges
/// are needed.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimSet {
    /// Environment clock (`SimClock`) advancement.
    Clock,
    /// Nest-geometry-dependent fixed-step systems. Empty for now: the nest
    /// transform is synced in `Update` so it stays correct while paused.
    NestSync,
    /// Rebuild the ant-density grid.
    Density,
    /// Age, handling, energy and lifecycle bookkeeping.
    Lifecycle,
    /// Recruitment and spawning.
    Spawn,
    /// Food pickup and nest dropoff.
    Collide,
    /// Delivery-rate EMA.
    Delivery,
    /// Steering and movement, including the pre-move position capture.
    Move,
    /// Pheromone deposition.
    Deposit,
    /// Pheromone evaporation and diffusion.
    Decay,
    /// Food depletion.
    Deplete,
    /// Per-tick visual synchronization.
    Visuals,
}

/// One-shot `Startup` sub-schedule, configured in exactly this chain order.
///
/// Command buffers of systems that ran concurrently are applied in
/// completion order, so entity index allocation depends on executor timing
/// unless startup spawners are ordered. Entity indices feed query iteration
/// order, which the determinism replay compares, so every startup system that
/// spawns entities belongs to one of these sets. The chain is configured once
/// in [`crate::simulation::SimulationPlugin::add_fixed_step_systems`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum StartupSet {
    /// Camera, play-area walls and the nest marker.
    World,
    /// The initial food patch and its visual markers.
    Food,
    /// Static obstacle visuals.
    Environment,
    /// The founding colony and its brood.
    Colony,
}

/// Read-only mirror of `Time<Virtual>`'s pause state.
///
/// `Time<Virtual>` is the single source of truth: the HUD pause button toggles
/// it and [`crate::ui::hud::sync_paused_indicator`] is the only writer of this
/// mirror, which exists so widget styling can read a plain resource. Do not
/// write it anywhere else.
#[derive(Resource, Default)]
pub struct Paused(pub bool);
