//! Persistent tool cursors and shared pointer helpers for the editor systems.

use bevy::prelude::*;

use crate::constants::ui::{
    FOOD_BRUSH_DIAMETER, FOOD_CURSOR_COLOR, NEST_CURSOR_DRAG_COLOR, NEST_CURSOR_IDLE_COLOR,
    NEST_CURSOR_SIZE,
};
use crate::constants::world::GRID_SIZE;
use crate::core::grid::{grid_to_world, world_to_grid};
use crate::core::layers::Z_CURSOR;
use crate::editor::food::brush_center_offset;
use crate::editor::nest::NestDrag;
use crate::editor::{EditorMode, EditorModeKind};

/// The persistent food paint cursor entity.
#[derive(Component)]
pub struct FoodCursor;

/// The persistent nest drag cursor entity.
#[derive(Component)]
pub struct NestCursor;

/// True while any UI widget (panel or button) is hovered or pressed.
pub fn pointer_over_ui(ui_query: &Query<&Interaction>) -> bool {
    any_interaction_active(ui_query.iter())
}

/// Pure helper behind [`pointer_over_ui`], kept testable without a `World`.
pub fn any_interaction_active<'a>(interactions: impl IntoIterator<Item = &'a Interaction>) -> bool {
    interactions
        .into_iter()
        .any(|interaction| *interaction != Interaction::None)
}

/// Project the window cursor into world space through the given 2D camera.
pub fn cursor_world_pos(
    window: &Window,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<Vec2> {
    let cursor_pos = window.cursor_position()?;
    camera
        .viewport_to_world_2d(camera_transform, cursor_pos)
        .ok()
}

/// Spawn both cursor entities once; they are hidden until their tool is active.
pub fn setup_cursors(mut commands: Commands) {
    let brush_size = FOOD_BRUSH_DIAMETER as f32 * GRID_SIZE;

    commands.spawn((
        FoodCursor,
        Sprite {
            color: FOOD_CURSOR_COLOR,
            custom_size: Some(Vec2::splat(brush_size)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, Z_CURSOR),
        Visibility::Hidden,
    ));

    commands.spawn((
        NestCursor,
        Sprite {
            color: NEST_CURSOR_IDLE_COLOR,
            custom_size: Some(Vec2::splat(NEST_CURSOR_SIZE)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, Z_CURSOR),
        Visibility::Hidden,
    ));
}

/// Move the food cursor to the brush center; hide it when the tool is inactive
/// or the pointer is over UI or outside the play area.
pub fn update_food_cursor(
    mode: Res<EditorMode>,
    ui_query: Query<&Interaction>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    cursor: Single<(&mut Transform, &mut Visibility), With<FoodCursor>>,
) {
    let (mut transform, mut visibility) = cursor.into_inner();
    let (camera, camera_transform) = camera.into_inner();

    let world_pos = if mode.0 == EditorModeKind::Food && !pointer_over_ui(&ui_query) {
        cursor_world_pos(window.into_inner(), camera, camera_transform)
    } else {
        None
    };

    let Some(center) = world_pos
        .and_then(world_to_grid)
        .map(|origin| grid_to_world(origin + brush_center_offset()))
    else {
        *visibility = Visibility::Hidden;
        return;
    };

    transform.translation.x = center.x;
    transform.translation.y = center.y;
    *visibility = Visibility::Visible;
}

/// Move and tint the nest cursor; hide it when the tool is inactive or the
/// pointer is over UI.
pub fn update_nest_cursor(
    mode: Res<EditorMode>,
    drag: Res<NestDrag>,
    ui_query: Query<&Interaction>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    cursor: Single<(&mut Transform, &mut Sprite, &mut Visibility), With<NestCursor>>,
) {
    let (mut transform, mut sprite, mut visibility) = cursor.into_inner();
    let (camera, camera_transform) = camera.into_inner();

    let world_pos = if mode.0 == EditorModeKind::Nest && !pointer_over_ui(&ui_query) {
        cursor_world_pos(window.into_inner(), camera, camera_transform)
    } else {
        None
    };

    let Some(world_pos) = world_pos else {
        *visibility = Visibility::Hidden;
        return;
    };

    transform.translation.x = world_pos.x;
    transform.translation.y = world_pos.y;
    sprite.color = if drag.dragging {
        NEST_CURSOR_DRAG_COLOR
    } else {
        NEST_CURSOR_IDLE_COLOR
    };
    *visibility = Visibility::Visible;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interaction_state_detection() {
        let idle = [Interaction::None, Interaction::None];
        assert!(!any_interaction_active(idle.iter()));

        let hovered = [Interaction::None, Interaction::Hovered];
        assert!(any_interaction_active(hovered.iter()));

        let pressed = [Interaction::Pressed];
        assert!(any_interaction_active(pressed.iter()));

        let empty: [Interaction; 0] = [];
        assert!(!any_interaction_active(empty.iter()));
    }
}
