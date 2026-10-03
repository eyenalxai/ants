use bevy::prelude::*;

use crate::constants::world::NEST_SIZE;
use crate::editor::cursor::{cursor_world_pos, pointer_over_ui};
use crate::editor::{EditorMode, EditorModeKind};
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::Nest;

/// Drag state for the nest tool (kept separate from the exclusive editor mode).
#[derive(Resource, Default)]
pub struct NestDrag {
    pub dragging: bool,
}

/// Whether a pointer at `pointer` is close enough to `nest_pos` to grab it.
pub fn hits_nest(nest_pos: Vec2, pointer: Vec2) -> bool {
    nest_pos.distance(pointer) <= NEST_SIZE / 2.0
}

/// Cancel an in-progress drag when the nest tool is left (runs on mode change).
pub fn cancel_nest_drag_on_mode_exit(mode: Res<EditorMode>, mut drag: ResMut<NestDrag>) {
    if mode.0 != EditorModeKind::Nest {
        drag.dragging = false;
    }
}

/// Drag the nest with the left mouse button while in nest mode. A drag only
/// starts when the press lands inside the nest radius; releasing anywhere ends
/// it, and the nest follows the cursor until then.
pub fn handle_nest_drag(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut drag: ResMut<NestDrag>,
    mut nest_query: Query<&mut Transform, With<Nest>>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    ui_query: Query<&Interaction>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
) {
    // Releasing anywhere (over UI, outside the play area or the window) ends
    // the drag.
    if !mouse_button.pressed(MouseButton::Left) {
        drag.dragging = false;
        return;
    }

    let over_ui = pointer_over_ui(&ui_query);
    let (camera, camera_transform) = camera.into_inner();
    let Some(world_pos) = cursor_world_pos(window.into_inner(), camera, camera_transform) else {
        drag.dragging = false;
        return;
    };

    if !drag.dragging {
        let start = mouse_button.just_pressed(MouseButton::Left)
            && !over_ui
            && nest_query
                .single_mut()
                .is_ok_and(|transform| hits_nest(transform.translation.truncate(), world_pos));

        if !start {
            return;
        }

        drag.dragging = true;
    }

    // Keep dragging while the pointer is over UI, but don't slide the nest
    // underneath it.
    if over_ui {
        return;
    }

    let Ok(mut nest_transform) = nest_query.single_mut() else {
        drag.dragging = false;
        return;
    };

    let moved =
        nest_transform.translation.x != world_pos.x || nest_transform.translation.y != world_pos.y;

    if moved {
        nest_transform.translation.x = world_pos.x;
        nest_transform.translation.y = world_pos.y;

        // The old to-nest trail points at the previous nest location.
        pheromone_grid.clear_to_nest();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_uses_half_the_nest_size() {
        assert!(hits_nest(Vec2::ZERO, Vec2::new(NEST_SIZE / 2.0 - 0.1, 0.0)));
        assert!(hits_nest(Vec2::ZERO, Vec2::new(NEST_SIZE / 2.0, 0.0)));
        assert!(!hits_nest(
            Vec2::ZERO,
            Vec2::new(NEST_SIZE / 2.0 + 0.1, 0.0)
        ));
        assert!(hits_nest(
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 10.0 + NEST_SIZE / 2.0)
        ));
    }
}
