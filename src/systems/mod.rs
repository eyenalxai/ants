mod collision;
mod move_ants;
mod pheromone_decay;
mod pheromone_toggle;
mod pheromone_visuals;
mod sensor_cone;
mod setup;
mod spawn_ants;

pub use collision::check_collisions;
pub use move_ants::move_ants;
pub use pheromone_decay::decay_pheromones;
pub use pheromone_toggle::toggle_pheromone_display;
pub use pheromone_visuals::update_pheromone_visuals;
pub use sensor_cone::draw_sensor_cone;
pub use setup::setup;
pub use spawn_ants::{spawn_ants, update_ant_lifetime};
