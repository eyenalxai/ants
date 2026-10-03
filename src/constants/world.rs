//! World geometry and shared simulation tuning constants.

/// Initial window width in pixels.
pub const WINDOW_WIDTH: u32 = 800;
/// Initial window height in pixels.
pub const WINDOW_HEIGHT: u32 = 600;

/// Play-area width in world units, derived from [`WINDOW_WIDTH`] so the two
/// cannot drift apart.
///
/// The simulation world stays fixed at this size even though the window is
/// resizable: ant coordinates are independent of the current window size.
pub const PLAY_AREA_WIDTH: f32 = WINDOW_WIDTH as f32;
/// Play-area height in world units, derived from [`WINDOW_HEIGHT`].
pub const PLAY_AREA_HEIGHT: f32 = WINDOW_HEIGHT as f32;

pub const GRID_SIZE: f32 = 4.0;
pub const GRID_WIDTH: usize = (PLAY_AREA_WIDTH / GRID_SIZE) as usize;
pub const GRID_HEIGHT: usize = (PLAY_AREA_HEIGHT / GRID_SIZE) as usize;

pub const NEST_SIZE: f32 = 40.0;
/// Radius of the nest circle; ants inside it refill, drop food and reset home.
pub const NEST_RADIUS: f32 = NEST_SIZE / 2.0;
pub const NEST_X: f32 = -350.0;
pub const NEST_Y: f32 = 0.0;
pub const FOOD_X: f32 = 320.0;
pub const FOOD_Y: f32 = 0.0;

pub const WALL_BOUNCE_MIN_ANGLE: f32 = 30.0;
pub const WALL_THICKNESS: f32 = 2.0;

/// Food units in each cell of the initial 3×3 patch at [`FOOD_X`], [`FOOD_Y`].
///
/// The patch therefore holds `9 × 1000 = 9000` units, i.e. 9000 full carry
/// trips: finite enough that depletion is observable within a session (at the
/// default colony size of up to 30 000 ants it lasts minutes at peak delivery
/// rates) while keeping the trail a reason to move. The per-cell quality
/// pattern is independent of this amount.
pub const INITIAL_FOOD_AMOUNT: f32 = 1000.0;
/// Lowest per-cell quality in the deterministic initial patch pattern.
pub const FOOD_QUALITY_MIN: f32 = 0.8;
/// Highest per-cell quality in the deterministic initial patch pattern.
pub const FOOD_QUALITY_MAX: f32 = 1.2;
pub const FOOD_CELL_RADIUS: f32 = GRID_SIZE * 0.3;

// Ant-density grid: coarse per-cell ant counts used for crowd avoidance and
// deposit suppression.
pub const DENSITY_CELL_SIZE: f32 = 8.0;
pub const DENSITY_GRID_WIDTH: usize = (PLAY_AREA_WIDTH / DENSITY_CELL_SIZE) as usize;
pub const DENSITY_GRID_HEIGHT: usize = (PLAY_AREA_HEIGHT / DENSITY_CELL_SIZE) as usize;
/// How far ahead of itself an ant probes for crowding.
pub const DENSITY_AHEAD_DISTANCE: f32 = 12.0;
/// Lateral probe offset at the forward probe point.
pub const DENSITY_SIDE_DISTANCE: f32 = 8.0;
/// Ants per density cell above which movement slows and turns away.
pub const DENSITY_CROWDED_THRESHOLD: u32 = 10;
/// Speed multiplier while crowded.
pub const DENSITY_SLOWDOWN_FACTOR: f32 = 0.8;
/// Fraction of the max turn rate used for crowd-avoidance turns.
pub const DENSITY_AVOIDANCE_TURN_FACTOR: f32 = 0.35;
/// Deposit suppression strength per occupying ant: `1 / (1 + k * density)`.
/// Kept small so an emerging (and therefore crowded) trail is not starved of
/// the deposits that created it.
pub const DENSITY_DEPOSIT_SUPPRESSION: f32 = 0.15;
/// Lower bound for [`DENSITY_DEPOSIT_SUPPRESSION`]; even the most crowded cell
/// keeps this fraction of a deposit so trails can always bootstrap.
pub const DENSITY_DEPOSIT_SUPPRESSION_FLOOR: f32 = 0.25;
