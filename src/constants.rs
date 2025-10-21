pub const WINDOW_WIDTH: u32 = 800;
pub const WINDOW_HEIGHT: u32 = 600;

pub const GRID_SIZE: f32 = 4.0;
pub const GRID_WIDTH: usize = (WINDOW_WIDTH as f32 / GRID_SIZE) as usize;
pub const GRID_HEIGHT: usize = (WINDOW_HEIGHT as f32 / GRID_SIZE) as usize;

pub const MAX_ANTS: usize = 30000;
pub const ANT_SPAWN_INTERVAL: f32 = 0.05;
pub const ANT_BATCH_SIZE: usize = 100;
pub const ANT_SPEED: f32 = 50.0;
pub const ANT_SIZE: f32 = 2.0;
pub const ANT_ALPHA: f32 = 0.005;
pub const ANT_LIFETIME: f32 = 30.0;
pub const ANT_LIFETIME_VARIATION_MIN: f32 = 0.5;
pub const ANT_SPEED_VARIATION_MIN: f32 = 0.5;
pub const ANT_YOUTH_DEPOSIT_MIN: f32 = 0.2;
pub const ANT_YOUTH_DEPOSIT_MAX: f32 = 0.8;

pub const ANT_TURN_RATE: f32 = 9.0;
pub const ANT_RANDOM_TURN_CHANCE: f32 = 0.8;
pub const ANT_PHEROMONE_FOLLOW_RANDOMNESS: f32 = 0.5;
pub const ANT_EXPLORATION_CHANCE: f32 = 0.2;
pub const ANT_PROBABILISTIC_STEERING_CHANCE: f32 = 0.6;
pub const ANT_STEERING_NOISE_FACTOR: f32 = 0.8;
pub const ANT_TURN_INTENSITY_BASE: f32 = 0.6;
pub const ANT_TURN_INTENSITY_SCALE: f32 = 0.4;
pub const ANT_EXPLORATION_STRENGTH_BASE: f32 = 0.6;

pub const SENSOR_DISTANCE: f32 = 40.0;
pub const SENSOR_ANGLE: f32 = std::f32::consts::PI / 2.0;
pub const NUM_SENSORS: usize = 9;
pub const FULL_SCAN_SENSORS: usize = 16;
pub const TARGET_LOCK_DISTANCE: f32 = 40.0;
pub const SENSOR_CONE_MARKER_SIZE: f32 = 3.0;
pub const SENSOR_CONE_ANT_MARKER_SIZE: f32 = 5.0;
pub const SENSOR_CONE_LINE_WIDTH: f32 = 1.0;
pub const SENSOR_CONE_MARKER_ALPHA: f32 = 0.6;
pub const SENSOR_CONE_LINE_ALPHA: f32 = 0.2;
pub const SENSOR_CONE_ANT_ALPHA: f32 = 0.8;

pub const PHEROMONE_DECAY_RATE: f32 = 0.995;
pub const PHEROMONE_DEPOSIT_RATE: f32 = 5.0;
pub const PHEROMONE_MAX_INTENSITY: f32 = 100.0;
pub const PHEROMONE_VISUAL_ALPHA: f32 = 0.5;

pub const NEST_SIZE: f32 = 40.0;
pub const FOOD_SIZE: f32 = 15.0;
pub const NEST_X: f32 = -350.0;
pub const NEST_Y: f32 = 0.0;
pub const FOOD_X: f32 = 320.0;
pub const FOOD_Y: f32 = 0.0;

pub const WALL_BOUNCE_MIN_ANGLE: f32 = 30.0;
