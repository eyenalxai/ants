/// Seconds after which a pheromone channel keeps half of its intensity.
pub const PHEROMONE_HALF_LIFE_SECS: f32 = 10.0;
/// Diffusion strength applied per normalized 60 Hz step.
pub const PHEROMONE_DIFFUSION: f32 = 0.1;
pub const PHEROMONE_DEPOSIT_RATE: f32 = 5.0;
pub const PHEROMONE_MAX_INTENSITY: f32 = 500.0;
pub const PHEROMONE_VISUAL_ALPHA: f32 = 0.5;

/// Weight of the center cell in the 3x3 deposit kernel.
pub const PHEROMONE_KERNEL_CENTER: f32 = 0.36;
/// Weight of the four orthogonal neighbors in the deposit kernel.
pub const PHEROMONE_KERNEL_ORTHOGONAL: f32 = 0.12;
/// Weight of the four diagonal neighbors in the deposit kernel.
///
/// The weights sum to 1: `center + 4 * orthogonal + 4 * diagonal`.
pub const PHEROMONE_KERNEL_DIAGONAL: f32 = 0.04;
