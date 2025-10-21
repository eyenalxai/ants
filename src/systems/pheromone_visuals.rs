use crate::components::PheromoneCell;
use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use crate::resources::PheromoneDisplayState;
use bevy::prelude::*;

pub fn update_pheromone_visuals(
    mut cell_query: Query<(&PheromoneCell, &mut Sprite)>,
    pheromone_grid: Res<PheromoneGrid>,
    display_state: Res<PheromoneDisplayState>,
) {
    for (cell, mut sprite) in &mut cell_query {
        if !display_state.enabled {
            sprite.color = Color::srgba(0.0, 0.0, 0.0, 0.0);
            continue;
        }

        if let Some(pheromone) = pheromone_grid.get(cell.grid_x, cell.grid_y) {
            let to_food_intensity = (pheromone.to_food / 100.0).min(1.0);
            let to_nest_intensity = (pheromone.to_nest / 100.0).min(1.0);

            let red = to_food_intensity;
            let blue = to_nest_intensity;
            let alpha = (to_food_intensity + to_nest_intensity).min(1.0) * PHEROMONE_VISUAL_ALPHA;

            sprite.color = Color::srgba(red, 0.0, blue, alpha);
        }
    }
}
