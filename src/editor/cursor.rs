use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::constants::world::GRID_SIZE;
use crate::core::grid::{grid_to_world, world_to_grid};
use crate::core::layers::Z_CURSOR;
use crate::editor::{EditorMode, EditorModeKind};

#[derive(Component)]
pub struct FoodCursorMarker;

#[derive(Component)]
pub struct NestCursorMarker;

/// Shared cursor-to-world lookup for editor systems.
#[derive(SystemParam)]
pub struct CursorWorldPosition<'w, 's> {
    windows: Query<'w, 's, &'static Window>,
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform)>,
}

impl CursorWorldPosition<'_, '_> {
    pub fn world_pos(&self) -> Option<Vec2> {
        let window = self.windows.iter().next()?;
        let cursor_pos = window.cursor_position()?;
        let (camera, camera_transform) = self.cameras.iter().next()?;
        camera
            .viewport_to_world_2d(camera_transform, cursor_pos)
            .ok()
    }
}

/// Per-frame food paint cursor (despawn + respawn, as before).
pub fn update_food_cursor(
    mut commands: Commands,
    mode: Res<EditorMode>,
    cursor: CursorWorldPosition,
    existing_cursors: Query<Entity, With<FoodCursorMarker>>,
    ui_query: Query<&Interaction>,
) {
    for entity in &existing_cursors {
        commands.entity(entity).despawn();
    }

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

    let size = 3.0 * GRID_SIZE;
    let center = grid_to_world(UVec2::new(origin.x + 1, origin.y + 1));

    commands.spawn((
        FoodCursorMarker,
        Sprite {
            color: Color::srgba(0.2, 0.8, 0.2, 0.5),
            custom_size: Some(Vec2::new(size, size)),
            ..default()
        },
        Transform::from_xyz(center.x, center.y, Z_CURSOR),
    ));
}

/// Per-frame nest drag cursor.
pub fn update_nest_cursor(
    mut commands: Commands,
    mode: Res<EditorMode>,
    drag: Res<crate::editor::nest::NestDrag>,
    cursor: CursorWorldPosition,
    existing_cursors: Query<Entity, With<NestCursorMarker>>,
    ui_query: Query<&Interaction>,
) {
    for entity in &existing_cursors {
        commands.entity(entity).despawn();
    }

    if mode.0 != EditorModeKind::Nest {
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

    let cursor_color = if drag.dragging {
        Color::srgba(1.0, 0.0, 0.0, 0.7)
    } else {
        Color::srgba(1.0, 0.0, 0.0, 0.4)
    };

    commands.spawn((
        NestCursorMarker,
        Sprite {
            color: cursor_color,
            custom_size: Some(Vec2::new(20.0, 20.0)),
            ..default()
        },
        Transform::from_xyz(world_pos.x, world_pos.y, Z_CURSOR),
    ));
}
