use bevy::prelude::*;

use crate::editor::cursor::CursorWorldPosition;
use crate::editor::{EditorMode, EditorModeKind};
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::Nest;

/// Drag state for the nest tool (kept separate from the exclusive editor mode).
#[derive(Resource, Default)]
pub struct NestDrag {
    pub dragging: bool,
}

/// Drag the nest with the left mouse button while in nest mode.
pub fn handle_nest_drag(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mode: Res<EditorMode>,
    mut drag: ResMut<NestDrag>,
    cursor: CursorWorldPosition,
    mut nest_query: Query<&mut Transform, With<Nest>>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    ui_query: Query<&Interaction>,
) {
    if mode.0 != EditorModeKind::Nest {
        drag.dragging = false;
        return;
    }

    if ui_query
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        return;
    }

    let Some(world_pos) = cursor.world_pos() else {
        drag.dragging = false;
        return;
    };

    if mouse_button.just_pressed(MouseButton::Left) {
        drag.dragging = true;
    }

    if mouse_button.just_released(MouseButton::Left) {
        drag.dragging = false;
    }

    if drag.dragging
        && mouse_button.pressed(MouseButton::Left)
        && let Ok(mut nest_transform) = nest_query.single_mut()
    {
        let moved = nest_transform.translation.x != world_pos.x
            || nest_transform.translation.y != world_pos.y;

        if moved {
            nest_transform.translation.x = world_pos.x;
            nest_transform.translation.y = world_pos.y;
            pheromone_grid.clear_to_nest();
        }
    }
}
