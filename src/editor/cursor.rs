//! Persistent tool cursors and shared pointer helpers for the editor systems.

use bevy::log::warn_once;
use bevy::prelude::*;

use crate::constants::ui::{
    FOOD_BRUSH_DIAMETER, FOOD_CURSOR_COLOR, NEST_CURSOR_DRAG_COLOR, NEST_CURSOR_IDLE_COLOR,
    NEST_CURSOR_SIZE,
};
use crate::constants::world::GRID_SIZE;
use crate::core::grid::{grid_to_world, world_to_grid};
use crate::core::layers::Z_CURSOR;
use crate::editor::food::brush_center_offset;
use crate::editor::nest::{NestDrag, clamp_nest_position};
use crate::editor::{EditorMode, EditorModeKind};

/// The persistent food paint cursor entity.
#[derive(Component)]
pub struct FoodCursor;

/// The persistent nest drag cursor entity.
#[derive(Component)]
pub struct NestCursor;

/// Write access to the food cursor entity.
type FoodCursorQuery<'w, 's> =
    Single<'w, 's, (&'static mut Transform, &'static mut Visibility), With<FoodCursor>>;

/// Write access to the nest cursor entity.
type NestCursorQuery<'w, 's> = Single<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Sprite,
        &'static mut Visibility,
    ),
    With<NestCursor>,
>;

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
    window: Option<Single<&Window>>,
    camera: Option<Single<(&Camera, &GlobalTransform)>>,
    cursor: Option<FoodCursorQuery>,
) {
    let (Some(window), Some(camera), Some(cursor)) = (window, camera, cursor) else {
        warn_once!("food cursor not updated: window, camera or cursor entity is missing");
        return;
    };

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
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }

        return;
    };

    if transform.translation.x != center.x || transform.translation.y != center.y {
        transform.translation.x = center.x;
        transform.translation.y = center.y;
    }

    if *visibility != Visibility::Visible {
        *visibility = Visibility::Visible;
    }
}

/// Move and tint the nest cursor; hide it when the tool is inactive or the
/// pointer is over UI.
pub fn update_nest_cursor(
    mode: Res<EditorMode>,
    drag: Res<NestDrag>,
    ui_query: Query<&Interaction>,
    window: Option<Single<&Window>>,
    camera: Option<Single<(&Camera, &GlobalTransform)>>,
    cursor: Option<NestCursorQuery>,
) {
    let (Some(window), Some(camera), Some(cursor)) = (window, camera, cursor) else {
        warn_once!("nest cursor not updated: window, camera or cursor entity is missing");
        return;
    };

    let (mut transform, mut sprite, mut visibility) = cursor.into_inner();
    let (camera, camera_transform) = camera.into_inner();

    let world_pos = if mode.0 == EditorModeKind::Nest && !pointer_over_ui(&ui_query) {
        cursor_world_pos(window.into_inner(), camera, camera_transform)
    } else {
        None
    };

    let Some(world_pos) = world_pos else {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }

        return;
    };

    // Preview where the clamped nest would land.
    let position = clamp_nest_position(world_pos);

    if transform.translation.x != position.x || transform.translation.y != position.y {
        transform.translation.x = position.x;
        transform.translation.y = position.y;
    }

    let color = if drag.dragging {
        NEST_CURSOR_DRAG_COLOR
    } else {
        NEST_CURSOR_IDLE_COLOR
    };

    if sprite.color != color {
        sprite.color = color;
    }

    if *visibility != Visibility::Visible {
        *visibility = Visibility::Visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::change_detection::Tick;
    use bevy::ecs::system::RunSystemOnce;

    fn cursor_world() -> World {
        let mut world = World::new();
        world.init_resource::<EditorMode>();
        world.insert_resource(NestDrag::default());
        world.spawn(Window::default());
        world.spawn((Camera::default(), GlobalTransform::default()));
        world.run_system_once(setup_cursors).unwrap();

        world
    }

    fn cursor_entities(world: &mut World) -> (Entity, Entity) {
        let mut food = world.query_filtered::<Entity, With<FoodCursor>>();
        let food = food.single(world).expect("food cursor entity");
        let mut nest = world.query_filtered::<Entity, With<NestCursor>>();
        let nest = nest.single(world).expect("nest cursor entity");

        (food, nest)
    }

    fn visibility_tick(world: &World, entity: Entity) -> Tick {
        world
            .entity(entity)
            .get_change_ticks::<Visibility>()
            .expect("cursor has visibility")
            .changed
    }

    #[test]
    fn inactive_cursors_are_hidden_once_and_not_remarked() {
        let mut world = cursor_world();
        let (food, nest) = cursor_entities(&mut world);

        world.run_system_once(update_food_cursor).unwrap();
        world.run_system_once(update_nest_cursor).unwrap();

        assert_eq!(*world.get::<Visibility>(food).unwrap(), Visibility::Hidden);
        assert_eq!(*world.get::<Visibility>(nest).unwrap(), Visibility::Hidden);

        let food_tick = visibility_tick(&world, food);
        let nest_tick = visibility_tick(&world, nest);

        for _ in 0..3 {
            world.run_system_once(update_food_cursor).unwrap();
            world.run_system_once(update_nest_cursor).unwrap();
        }

        assert_eq!(food_tick, visibility_tick(&world, food));
        assert_eq!(nest_tick, visibility_tick(&world, nest));
    }

    #[test]
    fn cursors_are_hidden_when_their_tool_is_inactive() {
        let mut world = cursor_world();
        let (food, nest) = cursor_entities(&mut world);

        // Pretend both cursors were left visible by an active tool.
        *world.get_mut::<Visibility>(food).unwrap() = Visibility::Visible;
        *world.get_mut::<Visibility>(nest).unwrap() = Visibility::Visible;

        world.run_system_once(update_food_cursor).unwrap();
        world.run_system_once(update_nest_cursor).unwrap();

        assert_eq!(*world.get::<Visibility>(food).unwrap(), Visibility::Hidden);
        assert_eq!(*world.get::<Visibility>(nest).unwrap(), Visibility::Hidden);
    }

    #[test]
    fn cursor_systems_run_without_a_window_camera_or_cursor() {
        // F15: with `Option<Single<...>>` the systems must run and no-op
        // instead of failing parameter validation and stalling silently.
        let mut world = World::new();
        world.init_resource::<EditorMode>();
        world.insert_resource(NestDrag::default());

        world.run_system_once(update_food_cursor).unwrap();
        world.run_system_once(update_nest_cursor).unwrap();
    }

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
