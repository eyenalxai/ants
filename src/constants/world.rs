pub const WINDOW_WIDTH: u32 = 800;
pub const WINDOW_HEIGHT: u32 = 600;

pub const PLAY_AREA_WIDTH: f32 = 800.0;
pub const PLAY_AREA_HEIGHT: f32 = 600.0;

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

pub const INITIAL_FOOD_AMOUNT: f32 = 100.0;
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
pub const DENSITY_CROWDED_THRESHOLD: u32 = 5;
/// Speed multiplier while crowded.
pub const DENSITY_SLOWDOWN_FACTOR: f32 = 0.8;
/// Fraction of the max turn rate used for crowd-avoidance turns.
pub const DENSITY_AVOIDANCE_TURN_FACTOR: f32 = 0.6;
/// Deposit suppression strength per occupying ant: `1 / (1 + k * density)`.
pub const DENSITY_DEPOSIT_SUPPRESSION: f32 = 1.5;
