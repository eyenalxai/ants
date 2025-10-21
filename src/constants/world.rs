pub const WINDOW_WIDTH: u32 = 800;
pub const WINDOW_HEIGHT: u32 = 600;

pub const PLAY_AREA_WIDTH: f32 = 800.0;
pub const PLAY_AREA_HEIGHT: f32 = 600.0;

pub const GRID_SIZE: f32 = 4.0;
pub const GRID_WIDTH: usize = (PLAY_AREA_WIDTH / GRID_SIZE) as usize;
pub const GRID_HEIGHT: usize = (PLAY_AREA_HEIGHT / GRID_SIZE) as usize;

pub const NEST_SIZE: f32 = 40.0;
pub const NEST_X: f32 = -350.0;
pub const NEST_Y: f32 = 0.0;
pub const FOOD_X: f32 = 320.0;
pub const FOOD_Y: f32 = 0.0;

pub const WALL_BOUNCE_MIN_ANGLE: f32 = 30.0;
pub const WALL_THICKNESS: f32 = 2.0;

pub const INITIAL_FOOD_AMOUNT: f32 = 100.0;
pub const FOOD_DEPLETION_RATE: f32 = 0.05;
