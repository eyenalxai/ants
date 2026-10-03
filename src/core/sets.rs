//! Shared scheduling sets and global run state.

use bevy::prelude::*;

/// Top-level scheduling sets shared by all feature plugins.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameSet {
    /// Simulation systems, stepped in `FixedUpdate`.
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
    /// Nest-geometry-dependent fixed-step systems.
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

/// Global pause flag. Pausing itself is implemented by pausing
/// `Time<Virtual>`, which freezes fixed stepping and virtual deltas; this
/// resource mirrors that state for the pause button tint.
#[derive(Resource, Default)]
pub struct Paused(pub bool);
