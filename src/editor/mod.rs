//! Editor input: exclusive explore/food/nest modes.

pub mod cursor;
pub mod food;
pub mod nest;

use bevy::prelude::*;

use crate::core::sets::GameSet;

/// Single exclusive editor mode; food and nest tools never trigger together.
#[derive(Resource, Default)]
pub struct EditorMode(pub EditorModeKind);

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorModeKind {
    #[default]
    Explore,
    Food,
    Nest,
}

/// Run condition: the food tool owns the pointer.
pub fn in_food_mode(mode: Res<EditorMode>) -> bool {
    mode.0 == EditorModeKind::Food
}

/// Run condition: the nest tool owns the pointer.
pub fn in_nest_mode(mode: Res<EditorMode>) -> bool {
    mode.0 == EditorModeKind::Nest
}

/// Mode after pressing the tool button for `kind`: activates it, or returns to
/// [`EditorModeKind::Explore`] when it is already active.
pub fn toggled_mode(current: EditorModeKind, kind: EditorModeKind) -> EditorModeKind {
    if current == kind {
        EditorModeKind::Explore
    } else {
        kind
    }
}

/// Owns the editor resources and input systems (run in `Update`, also while
/// paused).
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorMode>()
            .init_resource::<nest::NestDrag>()
            .add_message::<nest::NestMoved>()
            .add_systems(Startup, cursor::setup_cursors)
            .add_systems(
                Update,
                (
                    cursor::update_food_cursor,
                    cursor::update_nest_cursor,
                    food::handle_food_clicks.run_if(in_food_mode),
                    nest::handle_nest_drag.run_if(in_nest_mode),
                    nest::apply_nest_move,
                    nest::cancel_nest_drag_on_mode_exit.run_if(resource_changed::<EditorMode>),
                )
                    .chain()
                    .in_set(GameSet::Editor),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_the_active_tool_returns_to_explore() {
        assert_eq!(
            toggled_mode(EditorModeKind::Food, EditorModeKind::Food),
            EditorModeKind::Explore
        );
        assert_eq!(
            toggled_mode(EditorModeKind::Nest, EditorModeKind::Nest),
            EditorModeKind::Explore
        );
    }

    #[test]
    fn selecting_a_tool_is_exclusive() {
        assert_eq!(
            toggled_mode(EditorModeKind::Explore, EditorModeKind::Food),
            EditorModeKind::Food
        );
        assert_eq!(
            toggled_mode(EditorModeKind::Food, EditorModeKind::Nest),
            EditorModeKind::Nest
        );
        assert_eq!(
            toggled_mode(EditorModeKind::Nest, EditorModeKind::Food),
            EditorModeKind::Food
        );
    }
}
