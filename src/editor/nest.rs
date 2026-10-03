//! Nest tool: drag the nest (clamped inside the play area) and re-home ants.

use bevy::log::warn_once;
use bevy::prelude::*;

use crate::constants::world::{NEST_RADIUS, NEST_SIZE, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};
use crate::editor::cursor::{cursor_world_pos, pointer_over_ui};
use crate::editor::{EditorMode, EditorModeKind};
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::Nest;
use crate::simulation::ant::Ant;

/// Written by [`handle_nest_drag`] when the nest actually moves and read by
/// [`apply_nest_move`], which re-homes every ant and keeps the nest transform
/// in sync.
#[derive(Message)]
pub struct NestMoved {
    /// Nest position before the move.
    pub from: Vec2,
    /// New (clamped) nest position.
    pub to: Vec2,
}

/// Drag state for the nest tool (kept separate from the exclusive editor mode).
#[derive(Resource, Default)]
pub struct NestDrag {
    pub dragging: bool,
}

/// Whether a pointer at `pointer` is close enough to `nest_pos` to grab it.
pub fn hits_nest(nest_pos: Vec2, pointer: Vec2) -> bool {
    nest_pos.distance(pointer) <= NEST_SIZE / 2.0
}

/// Clamp a candidate nest center so the whole nest circle stays inside the
/// play-area walls.
pub fn clamp_nest_position(position: Vec2) -> Vec2 {
    let limit = Vec2::new(
        PLAY_AREA_WIDTH / 2.0 - NEST_RADIUS,
        PLAY_AREA_HEIGHT / 2.0 - NEST_RADIUS,
    );

    position.clamp(-limit, limit)
}

/// Cancel an in-progress drag when the nest tool is left (runs on mode change).
pub fn cancel_nest_drag_on_mode_exit(mode: Res<EditorMode>, mut drag: ResMut<NestDrag>) {
    if mode.0 != EditorModeKind::Nest {
        drag.dragging = false;
    }
}

/// Drag the nest with the left mouse button while in nest mode. A drag only
/// starts when the press lands inside the nest radius; releasing anywhere ends
/// it, and the nest follows the cursor until then. The target is clamped so
/// the whole nest circle stays inside the play area.
// `window` and `camera` cannot be joined: they live on different entities.
#[allow(clippy::too_many_arguments)]
pub fn handle_nest_drag(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut drag: ResMut<NestDrag>,
    mut nest_query: Query<&mut Transform, With<Nest>>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    mut messages: MessageWriter<NestMoved>,
    ui_query: Query<&Interaction>,
    window: Option<Single<&Window>>,
    camera: Option<Single<(&Camera, &GlobalTransform)>>,
) {
    // Releasing anywhere (over UI, outside the play area or the window) ends
    // the drag.
    if !mouse_button.pressed(MouseButton::Left) {
        drag.dragging = false;
        return;
    }

    let (Some(window), Some(camera)) = (window, camera) else {
        warn_once!("nest drag skipped: window or camera is unavailable");
        drag.dragging = false;
        return;
    };

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

    let from = nest_transform.translation.truncate();
    let to = clamp_nest_position(world_pos);

    if from != to {
        nest_transform.translation.x = to.x;
        nest_transform.translation.y = to.y;

        // The old to-nest trail points at the previous nest location.
        pheromone_grid.clear_to_nest();
        messages.write(NestMoved { from, to });
    }
}

/// Re-home every ant to the moved nest and keep the nest transform in sync.
/// The transform write is idempotent for [`handle_nest_drag`], which already
/// moved the nest before writing the message.
pub fn apply_nest_move(
    mut messages: MessageReader<NestMoved>,
    mut ants: Query<&mut Ant>,
    mut nest_query: Query<&mut Transform, With<Nest>>,
) {
    for message in messages.read() {
        for mut ant in &mut ants {
            ant.home = message.to;
        }

        for mut transform in &mut nest_query {
            transform.translation.x = message.to.x;
            transform.translation.y = message.to.y;
        }
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

    #[test]
    fn clamp_keeps_the_nest_circle_inside_the_play_area() {
        let limit = Vec2::new(
            PLAY_AREA_WIDTH / 2.0 - NEST_RADIUS,
            PLAY_AREA_HEIGHT / 2.0 - NEST_RADIUS,
        );

        assert_eq!(
            clamp_nest_position(Vec2::new(1e6, -1e6)),
            Vec2::new(limit.x, -limit.y)
        );
        assert_eq!(clamp_nest_position(-limit), -limit);
        assert_eq!(
            clamp_nest_position(Vec2::new(3.0, 4.0)),
            Vec2::new(3.0, 4.0)
        );
    }

    #[test]
    fn nest_move_message_rehomes_every_ant_and_moves_the_nest() {
        let mut app = App::new();
        app.add_message::<NestMoved>()
            .add_systems(Update, apply_nest_move);

        let from = Vec2::new(10.0, -20.0);
        let to = Vec2::new(-100.0, 42.0);

        let ants: Vec<Entity> = (0..3)
            .map(|_| {
                let mut ant = Ant::test_ant(0.0);
                ant.home = from;
                app.world_mut().spawn(ant).id()
            })
            .collect();

        let nest = app
            .world_mut()
            .spawn((Nest, Transform::from_xyz(from.x, from.y, 0.0)))
            .id();

        let _ = app.world_mut().write_message(NestMoved { from, to });
        app.update();

        for entity in ants {
            assert_eq!(app.world().get::<Ant>(entity).unwrap().home, to);
        }
        assert_eq!(
            app.world()
                .get::<Transform>(nest)
                .unwrap()
                .translation
                .truncate(),
            to
        );
    }
}
