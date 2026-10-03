//! One sprite per pheromone cell, tinted from the grid (overlay F3).

use bevy::prelude::*;

use crate::constants::pheromone::{PHEROMONE_MAX_INTENSITY, PHEROMONE_VISUAL_ALPHA};
use crate::constants::world::{GRID_HEIGHT, GRID_SIZE, GRID_WIDTH};
use crate::core::grid::grid_to_world;
use crate::core::layers::Z_PHEROMONE;
use crate::overlays::PheromoneDisplayState;
use crate::pheromone::grid::PheromoneGrid;

/// Marker for one overlay sprite, addressed by cell.
#[derive(Component)]
pub struct PheromoneCell {
    pub cell: UVec2,
}

/// Spawn a transparent sprite for every grid cell.
pub fn setup_pheromone_cells(mut commands: Commands) {
    for y in 0..GRID_HEIGHT as u32 {
        for x in 0..GRID_WIDTH as u32 {
            let cell = UVec2::new(x, y);
            let world = grid_to_world(cell);

            commands.spawn((
                PheromoneCell { cell },
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.0),
                    custom_size: Some(Vec2::new(GRID_SIZE, GRID_SIZE)),
                    ..default()
                },
                Transform::from_xyz(world.x, world.y, Z_PHEROMONE),
            ));
        }
    }
}

pub fn update_pheromone_visuals(
    mut cell_query: Query<(&PheromoneCell, &mut Sprite)>,
    pheromone_grid: Res<PheromoneGrid>,
    display_state: Res<PheromoneDisplayState>,
) {
    if !display_state.enabled {
        if display_state.is_changed() {
            for (_cell, mut sprite) in &mut cell_query {
                sprite.color = Color::srgba(0.0, 0.0, 0.0, 0.0);
            }
        }
        return;
    }

    for (cell, mut sprite) in &mut cell_query {
        if let Some(pheromone) = pheromone_grid.get(cell.cell) {
            let to_food_intensity = (pheromone.to_food / PHEROMONE_MAX_INTENSITY).min(1.0);
            let to_nest_intensity = (pheromone.to_nest / PHEROMONE_MAX_INTENSITY).min(1.0);

            let red = to_food_intensity;
            let blue = to_nest_intensity;
            let alpha = (to_food_intensity + to_nest_intensity).min(1.0) * PHEROMONE_VISUAL_ALPHA;

            let new_color = Color::srgba(red, 0.0, blue, alpha);

            if sprite.color != new_color {
                sprite.color = new_color;
            }
        }
    }
}
