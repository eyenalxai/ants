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

/// Global pause flag. Pausing itself is implemented by pausing
/// `Time<Virtual>`, which freezes fixed stepping and virtual deltas; this
/// resource mirrors that state for the pause button tint.
#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// Shared run condition for systems that must not run while paused.
pub fn not_paused(paused: Res<Paused>) -> bool {
    !paused.0
}
