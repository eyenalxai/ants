//! Debug overlays: pheromone visualization and the selected-ant sensor cone.

pub mod pheromone;
pub mod sensor_cone;

use bevy::prelude::*;

use crate::core::sets::GameSet;
use crate::simulation::ant::Ant;

#[derive(Resource)]
pub struct PheromoneDisplayState {
    pub enabled: bool,
}

#[derive(Resource)]
pub struct SelectedAnt {
    pub entity: Option<Entity>,
}

/// Owns the overlay resources, the overlay sprite and overlay systems.
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PheromoneDisplayState { enabled: false })
            .insert_resource(SelectedAnt { entity: None })
            .add_systems(
                Startup,
                (
                    pheromone::setup_pheromone_overlay,
                    sensor_cone::setup_sensor_cone,
                ),
            )
            .add_systems(
                Update,
                (
                    toggle_pheromone_display,
                    pheromone::update_pheromone_visuals,
                    sensor_cone::draw_sensor_cone,
                )
                    .chain()
                    .in_set(GameSet::Overlay),
            );
    }
}

/// F3 toggles the pheromone overlay and picks a random ant to inspect.
pub fn toggle_pheromone_display(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut display_state: ResMut<PheromoneDisplayState>,
    mut selected_ant: ResMut<SelectedAnt>,
    ant_query: Query<Entity, With<Ant>>,
) {
    if keyboard.just_pressed(KeyCode::F3) {
        display_state.enabled = !display_state.enabled;

        if display_state.enabled {
            if selected_ant.entity.is_none() {
                selected_ant.entity = random_ant(ant_query.iter());
            }
        } else {
            selected_ant.entity = None;
        }
    }
}

/// Uniformly sample one ant with reservoir sampling, without collecting the
/// whole population into a `Vec`.
pub fn random_ant(ants: impl Iterator<Item = Entity>) -> Option<Entity> {
    let mut chosen = None;

    for (seen, entity) in ants.enumerate() {
        if fastrand::usize(0..=seen) == 0 {
            chosen = Some(entity);
        }
    }

    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pheromone::grid::PheromoneGrid;
    use bevy::ecs::system::RunSystemOnce;

    /// The overlay systems mix read-only ant queries with write access to the
    /// cone's `Transform`s; initializing them catches conflicting queries that
    /// only surface at app startup.
    #[test]
    fn overlay_systems_initialize_without_param_conflicts() {
        let mut world = World::new();
        world.init_resource::<PheromoneGrid>();
        world.insert_resource(PheromoneDisplayState { enabled: false });
        world.insert_resource(SelectedAnt { entity: None });
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(Time::<()>::default());
        world.insert_resource(ButtonInput::<KeyCode>::default());

        world
            .run_system_once(pheromone::update_pheromone_visuals)
            .unwrap();
        world
            .run_system_once(sensor_cone::draw_sensor_cone)
            .unwrap();
        world.run_system_once(toggle_pheromone_display).unwrap();
    }
}
