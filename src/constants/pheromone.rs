//! Pheromone tuning constants (decay, diffusion, deposit kernel, visualization).

/// Seconds after which a pheromone channel keeps half of its intensity.
pub const PHEROMONE_HALF_LIFE_SECS: f32 = 10.0;
/// Diffusion strength applied per normalized 60 Hz step.
pub const PHEROMONE_DIFFUSION: f32 = 0.1;
pub const PHEROMONE_DEPOSIT_RATE: f32 = 5.0;
/// Storage clamp for a single cell/channel; a perception or visualization
/// scale, not an operational intensity.
pub const PHEROMONE_MAX_INTENSITY: f32 = 500.0;
/// Raw intensity that maps to full visual saturation in the pheromone overlay
/// and the sensor-cone tint. Chosen from operational cell values (a single
/// ant pass peaks near 0.04, a busy trail reaches roughly 20-70) rather than
/// from the storage clamp [`PHEROMONE_MAX_INTENSITY`].
pub const PHEROMONE_VISUAL_SCALE: f32 = 20.0;
/// Alpha multiplier applied to the combined normalized intensity.
pub const PHEROMONE_VISUAL_ALPHA: f32 = 0.5;

/// Weight of the center cell in the 3x3 deposit kernel.
pub const PHEROMONE_KERNEL_CENTER: f32 = 0.36;
/// Weight of the four orthogonal neighbors in the deposit kernel.
pub const PHEROMONE_KERNEL_ORTHOGONAL: f32 = 0.12;
/// Weight of the four diagonal neighbors in the deposit kernel.
///
/// The weights sum to 1: `center + 4 * orthogonal + 4 * diagonal`.
pub const PHEROMONE_KERNEL_DIAGONAL: f32 = 0.04;
