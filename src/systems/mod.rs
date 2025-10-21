mod collision;
mod food_management;
mod movement;
mod pause;
mod pheromone_decay;
mod pheromone_toggle;
mod pheromone_visuals;
mod sensor_cone;
mod setup;
mod spawn_ants;

pub use collision::check_collisions;
pub use food_management::{
    handle_food_clicks, setup_food_button, toggle_food_management, update_food_cursor,
    update_food_depletion, update_food_visuals,
};
pub use movement::move_ants;
pub use pause::{setup_pause_button, toggle_pause};
pub use pheromone_decay::decay_pheromones;
pub use pheromone_toggle::toggle_pheromone_display;
pub use pheromone_visuals::update_pheromone_visuals;
pub use sensor_cone::draw_sensor_cone;
pub use setup::setup;
pub use spawn_ants::{spawn_ants, update_ant_lifetime};
