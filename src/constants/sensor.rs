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
/// Half-saturation constant of the perception curve `raw / (raw + k)`.
///
/// The curve is independent of the storage clamp, graded at operational
/// values and saturating: a fresh pass (raw ~0.04) reads ~0.02, a busy trail
/// (raw 20-70) reads 0.91-0.97. Smaller values saturate too early and larger
/// ones flatten the response to a weak trail.
pub const SENSOR_HALF_SATURATION: f32 = 2.0;
/// Normalized readings below this divider are treated as no signal.
///
/// With the perception curve and ring attenuation, this detects a single fresh
/// pass at the inner ring (raw ~0.04 -> ~0.013 at 6 u) while the 14 u and 28 u
/// rings need an established trail (5+ passes), so a discovery is followable
/// without point-sampling noise triggering steering.
pub const SENSOR_SIGNAL_THRESHOLD: f32 = 0.01;
pub const SENSOR_CONE_MARKER_SIZE: f32 = 3.0;
pub const SENSOR_CONE_ANT_MARKER_SIZE: f32 = 5.0;
pub const SENSOR_CONE_LINE_WIDTH: f32 = 1.0;
pub const SENSOR_CONE_MARKER_ALPHA: f32 = 0.6;
pub const SENSOR_CONE_LINE_ALPHA: f32 = 0.2;
pub const SENSOR_CONE_ANT_ALPHA: f32 = 0.8;
