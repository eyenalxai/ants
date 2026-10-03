//! Z positions for world-space entities (2D draw ordering).
//!
//! The full-arena pheromone overlay is deliberately below every gameplay
//! entity, including the nest, so the nest disc is never swallowed by the
//! overlay sprite.

pub const Z_PHEROMONE: f32 = -0.1;
pub const Z_NEST: f32 = 0.0;
pub const Z_WALL: f32 = 0.1;
/// Static obstacles (F16), above the walls but below food and ants.
pub const Z_OBSTACLE: f32 = 0.2;
pub const Z_FOOD: f32 = 0.5;
pub const Z_CURSOR: f32 = 0.6;
pub const Z_ANT: f32 = 1.0;
pub const Z_SENSOR_CONE_LINE: f32 = 1.5;
pub const Z_SENSOR_CONE_MARKER: f32 = 2.0;
