//! Sensing geometry shared by movement sensors and debug overlays.

/// Outer sampling ring, also the length drawn by the sensor-cone overlay.
pub const SENSOR_DISTANCE: f32 = 28.0;
/// Half-angle of the sensor fan (`±60°`).
pub const SENSOR_ANGLE: f32 = std::f32::consts::PI / 3.0;
pub const NUM_SENSORS: usize = 9;
/// Sampling rings from closest to farthest. The outer ring is [`SENSOR_DISTANCE`]
/// so the overlay lines match what ants actually sample.
pub const SENSOR_RING_DISTANCES: [f32; 3] = [6.0, 14.0, SENSOR_DISTANCE];
/// Exponential attenuation length used across rings: `exp(-d / this)`. Long
/// enough that the outer ring still carries a usable fraction of the signal.
pub const SENSOR_RING_ATTENUATION: f32 = 16.0;
/// Normalized readings below this divider are treated as no signal. Low enough
/// that a single fresh pass is detectable at the inner ring: with the ring
/// attenuation and `sqrt(raw / PHEROMONE_MAX_INTENSITY)` compression this
/// corresponds to ~0.03 raw at 6 u, ~0.07 at 14 u and ~0.4 at 28 u.
pub const SENSOR_SIGNAL_THRESHOLD: f32 = 0.005;
pub const SENSOR_CONE_MARKER_SIZE: f32 = 3.0;
pub const SENSOR_CONE_ANT_MARKER_SIZE: f32 = 5.0;
pub const SENSOR_CONE_LINE_WIDTH: f32 = 1.0;
pub const SENSOR_CONE_MARKER_ALPHA: f32 = 0.6;
pub const SENSOR_CONE_LINE_ALPHA: f32 = 0.2;
pub const SENSOR_CONE_ANT_ALPHA: f32 = 0.8;
