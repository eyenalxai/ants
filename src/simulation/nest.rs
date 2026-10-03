//! Nest structure: entrance and refuse-pile geometry.
//!
//! The environment stream fills this module (entrance bottleneck, refuse pile);
//! [`register`] is the wiring hook its systems grow into.

use bevy::prelude::*;

use crate::constants::world::{NEST_RADIUS, NEST_X, NEST_Y};

/// Where the nest entrance and refuse pile sit in world space.
///
/// The default collapses the entrance and the refuse pile onto the nest centre
/// and gives the entrance the full nest radius, which reproduces today's
/// disc-only dropoff/spawn behavior until the environment stream moves them.
#[derive(Resource)]
pub struct NestGeometry {
    /// World position of the nest entrance.
    pub entrance: Vec2,
    /// Radius around [`NestGeometry::entrance`] treated as the nest mouth.
    pub entrance_radius: f32,
    /// World position of the refuse pile.
    pub refuse: Vec2,
}

impl Default for NestGeometry {
    fn default() -> Self {
        let center = Vec2::new(NEST_X, NEST_Y);

        Self {
            entrance: center,
            entrance_radius: NEST_RADIUS,
            refuse: center,
        }
    }
}

/// Wiring hook for the environment stream: it registers nest-geometry systems
/// here. Empty until that stream lands.
pub fn register(_app: &mut App) {}
