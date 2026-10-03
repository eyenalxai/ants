//! Nest tool: drag the nest (clamped inside the play area) and re-home ants.

use bevy::log::debug;
use bevy::prelude::*;

use crate::constants::world::NEST_SIZE;
use crate::editor::cursor::PointerInput;
use crate::editor::{EditorMode, EditorModeKind};
use crate::pheromone::grid::PheromoneGrid;
use crate::simulation::NestPosition;
use crate::simulation::ant::Ant;

/// Written by [`handle_nest_drag`] when the nest actually moves and read by
/// [`apply_nest_move`], which re-homes every ant.
#[derive(Message)]
pub struct NestMoved {
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
    NestPosition::clamped(position)
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
///
/// The drag writes the authoritative [`NestPosition`] and emits
/// [`NestMoved`]; the nest transform is derived in `Update` by the simulation
/// plugin.
pub fn handle_nest_drag(
    pointer: PointerInput,
    mut drag: ResMut<NestDrag>,
    mut nest_position: ResMut<NestPosition>,
    mut pheromone_grid: ResMut<PheromoneGrid>,
    mut messages: MessageWriter<NestMoved>,
) {
    // Releasing anywhere (over UI, outside the play area or the window) ends
    // the drag.
    if !pointer.left_pressed() {
        drag.dragging = false;
        return;
    }

    let Some(world_pos) = pointer.world_pos() else {
        drag.dragging = false;
        return;
    };

    let over_ui = pointer.over_ui();

    if !drag.dragging {
        let start =
            pointer.left_just_pressed() && !over_ui && hits_nest(nest_position.0, world_pos);

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

    let from = nest_position.0;
    nest_position.set(world_pos);
    let to = nest_position.0;

    if from != to {
        // The old to-nest trail points at the previous nest location.
        pheromone_grid.clear_to_nest();
        messages.write(NestMoved { to });
        debug!("nest moved from {from:?} to {to:?}");
    }
}

/// Re-home every ant to the moved nest. The nest transform is written by the
/// `Update` sync system from [`NestPosition`], not here.
pub fn apply_nest_move(mut messages: MessageReader<NestMoved>, mut ants: Query<&mut Ant>) {
    for message in messages.read() {
        for mut ant in &mut ants {
            ant.home = message.to;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::world::{NEST_RADIUS, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};
    use crate::editor::cursor::test_support::{
        clear_mouse_frame, pointer_world, press_mouse, release_mouse, set_cursor, world_to_window,
    };
    use crate::simulation::Nest;
    use bevy::ecs::system::RunSystemOnce;

    /// A world with the nest drag state and the pointer fixtures.
    fn drag_world(cursor: Vec2) -> World {
        let mut world = pointer_world(cursor);
        world.init_resource::<NestDrag>();
        world.init_resource::<NestPosition>();
        world.init_resource::<PheromoneGrid>();
        world.init_resource::<Messages<NestMoved>>();
        world
    }

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
    fn nest_move_message_rehomes_every_ant_without_touching_the_transform() {
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

        let _ = app.world_mut().write_message(NestMoved { to });
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
            from,
            "the transform is written by the Update sync, not by apply_nest_move"
        );
    }

    #[test]
    fn nest_drag_starts_inside_the_nest_and_follows_the_cursor() {
        let start = NestPosition::default().0;
        let mut world = drag_world(world_to_window(start));

        press_mouse(&mut world, MouseButton::Left);
        world.run_system_once(handle_nest_drag).unwrap();

        assert!(world.resource::<NestDrag>().dragging);
        assert_eq!(
            world.resource::<NestPosition>().0,
            start,
            "pressing without moving must not move the nest"
        );

        let target = Vec2::new(-100.0, 42.0);
        set_cursor(&mut world, world_to_window(target));
        clear_mouse_frame(&mut world);
        world.run_system_once(handle_nest_drag).unwrap();

        assert!(world.resource::<NestDrag>().dragging);
        assert!(
            world.resource::<NestPosition>().0.distance(target) < 1e-3,
            "the nest follows the cursor to {target:?}"
        );
        assert_eq!(
            world.resource::<Messages<NestMoved>>().len(),
            1,
            "the move is announced"
        );
    }

    #[test]
    fn nest_drag_cancels_on_release() {
        let start = NestPosition::default().0;
        let mut world = drag_world(world_to_window(start));

        press_mouse(&mut world, MouseButton::Left);
        world.run_system_once(handle_nest_drag).unwrap();
        assert!(world.resource::<NestDrag>().dragging);

        release_mouse(&mut world);
        world.run_system_once(handle_nest_drag).unwrap();

        assert!(!world.resource::<NestDrag>().dragging);
        assert_eq!(world.resource::<NestPosition>().0, start);
    }

    #[test]
    fn nest_drag_does_not_start_outside_the_nest_or_over_ui() {
        // Far from the nest: the press is not a grab.
        let mut world = drag_world(world_to_window(Vec2::ZERO));
        press_mouse(&mut world, MouseButton::Left);
        world.run_system_once(handle_nest_drag).unwrap();
        assert!(!world.resource::<NestDrag>().dragging);

        // Inside the nest radius, but the press lands on a UI widget.
        let start = NestPosition::default().0;
        let mut world = drag_world(world_to_window(start));
        world.spawn(Interaction::Pressed);
        press_mouse(&mut world, MouseButton::Left);
        world.run_system_once(handle_nest_drag).unwrap();
        assert!(!world.resource::<NestDrag>().dragging);
    }
}
