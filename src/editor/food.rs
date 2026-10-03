use bevy::log::warn_once;
use bevy::prelude::*;

use crate::constants::ui::FOOD_BRUSH_DIAMETER;
use crate::constants::world::INITIAL_FOOD_AMOUNT;
use crate::core::grid::{in_bounds, world_to_grid};
use crate::editor::cursor::{cursor_world_pos, pointer_over_ui};
use crate::simulation::food::FoodGrid;

/// Cells covered by the square food brush anchored at `origin` (top-left
/// corner), clipped to the grid.
pub fn brush_cells(origin: UVec2) -> impl Iterator<Item = UVec2> + Clone {
    (0..FOOD_BRUSH_DIAMETER as u32).flat_map(move |dy| {
        (0..FOOD_BRUSH_DIAMETER as u32).filter_map(move |dx| {
            let cell = origin + UVec2::new(dx, dy);
            in_bounds(cell).then_some(cell)
        })
    })
}

/// Cell offset from the brush origin to the cell at the center of the brush.
pub fn brush_center_offset() -> UVec2 {
    UVec2::splat((FOOD_BRUSH_DIAMETER as u32).saturating_sub(1) / 2)
}

/// Left click paints a 3×3 food patch, right click removes it. Runs only while
/// in food mode (see the plugin run condition); clicks over UI are ignored.
pub fn handle_food_clicks(
    mut commands: Commands,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut food_grid: ResMut<FoodGrid>,
    ui_query: Query<&Interaction>,
    window: Option<Single<&Window>>,
    camera: Option<Single<(&Camera, &GlobalTransform)>>,
) {
    if pointer_over_ui(&ui_query) {
        return;
    }

    let (Some(window), Some(camera)) = (window, camera) else {
        warn_once!("food clicks ignored: window or camera is unavailable");
        return;
    };

    let (camera, camera_transform) = camera.into_inner();
    let Some(world_pos) = cursor_world_pos(window.into_inner(), camera, camera_transform) else {
        return;
    };
    let Some(origin) = world_to_grid(world_pos) else {
        return;
    };

    if mouse_button.pressed(MouseButton::Left) {
        for cell in brush_cells(origin) {
            if !food_grid.contains(cell) {
                food_grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
            }
        }
    } else if mouse_button.pressed(MouseButton::Right) {
        for cell in brush_cells(origin) {
            food_grid.remove(&mut commands, cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};

    #[test]
    fn brush_covers_a_square_in_the_interior() {
        let origin = UVec2::new(5, 5);
        let cells: Vec<UVec2> = brush_cells(origin).collect();

        assert_eq!(
            cells.len(),
            (FOOD_BRUSH_DIAMETER * FOOD_BRUSH_DIAMETER) as usize
        );
        assert!(cells.contains(&origin));
        assert!(cells.contains(&(origin + UVec2::splat(2))));
        assert_eq!(brush_center_offset(), UVec2::splat(1));
    }

    #[test]
    fn brush_clips_to_grid_bounds() {
        let last = UVec2::new(GRID_WIDTH as u32 - 1, GRID_HEIGHT as u32 - 1);
        assert_eq!(brush_cells(last).collect::<Vec<_>>(), vec![last]);

        // A brush hanging over the right edge keeps only its in-bounds column.
        let edge = UVec2::new(GRID_WIDTH as u32 - 1, 0);
        let cells: Vec<UVec2> = brush_cells(edge).collect();
        assert_eq!(cells.len(), FOOD_BRUSH_DIAMETER as usize);
        assert!(
            cells
                .iter()
                .all(|cell| in_bounds(*cell) && cell.x == edge.x)
        );
    }
}
