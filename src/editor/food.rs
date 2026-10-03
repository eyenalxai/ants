use bevy::prelude::*;

use crate::constants::world::INITIAL_FOOD_AMOUNT;
use crate::core::grid::{in_bounds, world_to_grid};
use crate::editor::cursor::CursorWorldPosition;
use crate::editor::{EditorMode, EditorModeKind};
use crate::simulation::food::FoodGrid;

/// Left click paints a 3x3 food patch, right click removes it.
pub fn handle_food_clicks(
    mut commands: Commands,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mode: Res<EditorMode>,
    cursor: CursorWorldPosition,
    mut food_grid: ResMut<FoodGrid>,
    ui_query: Query<&Interaction>,
) {
    if mode.0 != EditorModeKind::Food {
        return;
    }

    if ui_query
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        return;
    }

    let Some(world_pos) = cursor.world_pos() else {
        return;
    };
    let Some(origin) = world_to_grid(world_pos) else {
        return;
    };

    if mouse_button.pressed(MouseButton::Left) {
        for dy in 0..3 {
            for dx in 0..3 {
                let cell = UVec2::new(origin.x + dx, origin.y + dy);

                if in_bounds(cell) && !food_grid.contains(cell) {
                    food_grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
                }
            }
        }
    } else if mouse_button.pressed(MouseButton::Right) {
        for dy in 0..3 {
            for dx in 0..3 {
                let cell = UVec2::new(origin.x + dx, origin.y + dy);

                if in_bounds(cell) {
                    food_grid.remove(&mut commands, cell);
                }
            }
        }
    }
}
