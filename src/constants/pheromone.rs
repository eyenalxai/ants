//! Pheromone tuning constants (decay, diffusion, deposit kernel, visualization).

/// Seconds after which the recruitment trail (`ToFood`) keeps half of its
/// intensity. Substrate trail pheromone is non-volatile: evaporation, not
/// diffusion, governs how long a trail stays usable.
pub const PHEROMONE_HALF_LIFE_SECS: f32 = 10.0;
/// Half-life of the `ToNest` home-range mark. Slower than recruitment because
/// it is a coarse "this way is home" signal, not a decaying recruitment trail.
pub const PHEROMONE_TO_NEST_HALF_LIFE_SECS: f32 = 30.0;
/// Diffusion strength applied per normalized 60 Hz step.
///
/// Real trail pheromone barely diffuses (a trail stays a few body lengths
/// wide for its whole lifetime), so this is two orders of magnitude below the
/// old `0.1`: a kernel deposit keeps an RMS radius under two cells over one
/// half-life. The 3x3 deposit kernel already sets the trail width.
pub const PHEROMONE_DIFFUSION: f32 = 0.002;
pub const PHEROMONE_DEPOSIT_RATE: f32 = 5.0;
/// Storage clamp for a single cell/channel; a perception or visualization
/// scale, not an operational intensity.
pub const PHEROMONE_MAX_INTENSITY: f32 = 500.0;
/// Intensity below which a channel snaps to zero, so active cells can retire.
///
/// Must stay far below a single fresh pass (peak raw ~0.04) or the snap turns
/// an isolated deposit into a no-op after a fraction of a second; at `1e-4` a
/// single pass survives the full half-life while retired cells still free
/// themselves.
pub const PHEROMONE_MIN_THRESHOLD: f32 = 1e-4;
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

/// Base multiplier of the `ToNest` home-range mark laid by foragers.
///
/// The `ToNest` channel is deliberately not a recruitment trail: it is a weak
/// "home is this way" mark, so outbound traffic cannot build a strong
/// direction signal before any food has been found. Recruitment deposits
/// (`ToFood`) are gated on a successful trip instead.
pub const PHEROMONE_TO_NEST_BASE: f32 = 0.2;
/// Quality scaling of a laden ant's recruitment deposit:
/// `0.5 + 0.5 * carrying_quality` (1.0 for average food).
pub const PHEROMONE_QUALITY_BASE: f32 = 0.5;
/// See [`PHEROMONE_QUALITY_BASE`].
pub const PHEROMONE_QUALITY_SCALE: f32 = 0.5;
/// Upper bound of `carrying_quality` accepted by the deposit scaling, so a
/// runaway quality value cannot amplify deposits without bound.
pub const PHEROMONE_QUALITY_MAX: f32 = 2.0;
/// Distance scaling of a laden ant's recruitment deposit:
/// `0.3 + 0.7 * clamp(dist_to_nest / route_length, 0, 1)`, i.e. more pheromone
/// near the food and less near the nest (Czaczkes et al. 2024).
pub const PHEROMONE_DISTANCE_BASE: f32 = 0.3;
/// See [`PHEROMONE_DISTANCE_BASE`].
pub const PHEROMONE_DISTANCE_SCALE: f32 = 0.7;
/// Suppression of a deposit on a route the ant can already follow:
/// `1 / (1 + k * followed_strength)`, so an established trail is not
/// double-marked and reinforcement spreads to unmarked ground.
pub const PHEROMONE_TRAIL_SUPPRESSION: f32 = 2.0;
/// Floor of the nest-proximity suppression for outbound ants. Inside the nest
/// ants should not lay recruitment trails, but a small bootstrap deposit must
/// always survive so an emerging trail is never starved.
pub const PHEROMONE_NEST_PROXIMITY_FLOOR: f32 = 0.15;
