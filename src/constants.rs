pub const GRID_SIZE: f32 = 8.0;
pub const WINDOW_WIDTH: u32 = 800;
pub const WINDOW_HEIGHT: u32 = 600;
pub const GRID_WIDTH: usize = (WINDOW_WIDTH as f32 / GRID_SIZE) as usize;
pub const GRID_HEIGHT: usize = (WINDOW_HEIGHT as f32 / GRID_SIZE) as usize;
pub const MAX_ANTS: usize = 10000;
pub const ANT_SPAWN_INTERVAL: f32 = 0.05;
pub const ANT_SPEED: f32 = 50.0;
