//! Persistent tool cursors and shared pointer helpers for the editor systems.

use bevy::ecs::system::SystemParam;
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

/// Shared read-only pointer inputs for the editor systems: the primary window,
/// the 2D camera, UI interaction state and mouse buttons.
///
/// Bundling them keeps the four pointer systems' signatures small and gives
/// the missing-window/camera warn-once handling a single home.
#[derive(SystemParam)]
pub struct PointerInput<'w, 's> {
    window: Option<Single<'w, 's, &'static Window>>,
    camera: Option<Single<'w, 's, (&'static Camera, &'static GlobalTransform)>>,
    ui: Query<'w, 's, &'static Interaction>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
}

impl PointerInput<'_, '_> {
    /// Project the pointer into world space through the 2D camera.
    ///
    /// Returns `None` (warning once) when the window or camera is missing,
    /// and `None` without warning when the cursor is outside the window or the
    /// camera cannot map the position.
    pub fn world_pos(&self) -> Option<Vec2> {
        let (Some(window), Some(camera)) = (&self.window, &self.camera) else {
            warn_once!("editor pointer unavailable: window or camera is missing");
            return None;
        };

        let (camera, camera_transform) = **camera;
        cursor_world_pos(**window, camera, camera_transform)
    }

    /// True while any UI widget (panel or button) is hovered or pressed.
    pub fn over_ui(&self) -> bool {
        pointer_over_ui(&self.ui)
    }

    /// Whether the left mouse button is held.
    pub fn left_pressed(&self) -> bool {
        self.mouse.pressed(MouseButton::Left)
    }

    /// Whether the left mouse button was pressed this frame.
    pub fn left_just_pressed(&self) -> bool {
        self.mouse.just_pressed(MouseButton::Left)
    }

    /// Whether the right mouse button is held.
    pub fn right_pressed(&self) -> bool {
        self.mouse.pressed(MouseButton::Right)
    }
}

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
    pointer: PointerInput,
    cursor: Option<FoodCursorQuery>,
) {
    let Some(cursor) = cursor else {
        warn_once!("food cursor not updated: cursor entity is missing");
        return;
    };

    let (mut transform, mut visibility) = cursor.into_inner();

    let world_pos = if mode.0 == EditorModeKind::Food && !pointer.over_ui() {
        pointer.world_pos()
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
    pointer: PointerInput,
    cursor: Option<NestCursorQuery>,
) {
    let Some(cursor) = cursor else {
        warn_once!("nest cursor not updated: cursor entity is missing");
        return;
    };

    let (mut transform, mut sprite, mut visibility) = cursor.into_inner();

    let world_pos = if mode.0 == EditorModeKind::Nest && !pointer.over_ui() {
        pointer.world_pos()
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

/// Headless pointer fixtures shared by the editor click tests.
#[cfg(test)]
pub(crate) mod test_support {
    use bevy::camera::{RenderTargetInfo, ScalingMode, Viewport};
    use bevy::prelude::*;
    use bevy::window::WindowResolution;

    /// Window size shared by the pointer fixtures, in logical pixels.
    pub(crate) const TEST_WINDOW_SIZE: Vec2 = Vec2::new(800.0, 600.0);

    /// Build a headless world with one window (cursor at `cursor`) and one
    /// camera that maps window pixels 1:1 to world units, centered on the
    /// window: window position `(0, 0)` is world `(-400, 300)` and the window
    /// center is world `(0, 0)`.
    ///
    /// The camera is configured the way Bevy's own camera tests do it, since
    /// `camera_system` does not run headless: a viewport with a target size
    /// and a matching orthographic projection matrix.
    pub(crate) fn pointer_world(cursor: Vec2) -> World {
        let mut world = World::new();

        let mut window = Window {
            resolution: WindowResolution::new(TEST_WINDOW_SIZE.x as u32, TEST_WINDOW_SIZE.y as u32),
            ..default()
        };
        window.set_cursor_position(Some(cursor));
        world.spawn(window);

        let viewport = Viewport {
            physical_size: TEST_WINDOW_SIZE.as_uvec2(),
            ..default()
        };
        let mut camera = Camera {
            viewport: Some(viewport),
            ..default()
        };
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: TEST_WINDOW_SIZE.as_uvec2(),
            scale_factor: 1.0,
        });
        let mut projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_2d()
        });
        projection.update(TEST_WINDOW_SIZE.x, TEST_WINDOW_SIZE.y);
        camera.computed.clip_from_view = projection.get_clip_from_view();
        world.spawn((camera, GlobalTransform::default()));

        world.init_resource::<ButtonInput<MouseButton>>();
        world
    }

    /// World position of a window cursor position in [`pointer_world`].
    pub(crate) fn window_to_world(cursor: Vec2) -> Vec2 {
        Vec2::new(
            cursor.x - TEST_WINDOW_SIZE.x / 2.0,
            TEST_WINDOW_SIZE.y / 2.0 - cursor.y,
        )
    }

    /// Window cursor position that [`pointer_world`] maps to `world`.
    pub(crate) fn world_to_window(world: Vec2) -> Vec2 {
        Vec2::new(
            world.x + TEST_WINDOW_SIZE.x / 2.0,
            TEST_WINDOW_SIZE.y / 2.0 - world.y,
        )
    }

    /// Move the cursor of the single window in `world`.
    pub(crate) fn set_cursor(world: &mut World, cursor: Vec2) {
        let window = {
            let mut query = world.query_filtered::<Entity, With<Window>>();
            query.single(world).expect("test window")
        };
        world
            .get_mut::<Window>(window)
            .expect("window component")
            .set_cursor_position(Some(cursor));
    }

    /// Replace the mouse state with a fresh press of `button`.
    pub(crate) fn press_mouse(world: &mut World, button: MouseButton) {
        let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
        mouse.reset_all();
        mouse.press(button);
    }

    /// Keep the held buttons but clear this frame's just-pressed flags.
    pub(crate) fn clear_mouse_frame(world: &mut World) {
        world.resource_mut::<ButtonInput<MouseButton>>().clear();
    }

    /// Release every mouse button.
    pub(crate) fn release_mouse(world: &mut World) {
        world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
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
        world.insert_resource(ButtonInput::<MouseButton>::default());
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
        world.init_resource::<ButtonInput<MouseButton>>();

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
