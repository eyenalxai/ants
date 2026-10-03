//! Editor input: exclusive explore/food/nest modes.

pub mod cursor;
pub mod food;
pub mod nest;

use bevy::prelude::*;

use crate::core::sets::GameSet;

/// Single exclusive editor mode; food and nest tools never trigger together.
#[derive(Resource, Default)]
pub struct EditorMode(pub EditorModeKind);

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum EditorModeKind {
    #[default]
    Explore,
    Food,
    Nest,
}

/// Owns the editor resources and input systems (run in `Update`, also while
/// paused).
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorMode>()
            .init_resource::<nest::NestDrag>()
            .add_systems(
                Update,
                (
                    cursor::update_food_cursor,
                    food::handle_food_clicks,
                    cursor::update_nest_cursor,
                    nest::handle_nest_drag,
                )
                    .chain()
                    .in_set(GameSet::Editor),
            );
    }
}
