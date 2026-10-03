use bevy::prelude::*;
use std::f32::consts::PI;

use crate::constants::sensor::*;
use crate::core::layers::{Z_SENSOR_CONE_LINE, Z_SENSOR_CONE_MARKER};
use crate::overlays::{SelectedAnt, random_ant};
use crate::simulation::ant::Ant;

/// Marker for every persistent sensor-cone entity.
#[derive(Component)]
pub struct SensorConeMarker;

/// Which piece of the cone a persistent entity renders.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorConePart {
    /// Thin line from the ant to sensor `usize`.
    Line(usize),
    /// Dot at sensor `usize`.
    Sensor(usize),
    /// Red dot on the selected ant itself.
    Ant,
}

/// Selected-ant lookup (read-only, disjoint from the cone entities).
type AntQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Ant, &'static Transform), (With<Ant>, Without<SensorConePart>)>;

/// Persistent cone entities (write access, disjoint from ants).
type ConePartsQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static SensorConePart,
        &'static mut Transform,
        &'static mut Visibility,
    ),
    (With<SensorConePart>, Without<Ant>),
>;

/// Spawn the sensor cone once; [`draw_sensor_cone`] only moves the entities.
pub fn setup_sensor_cone(mut commands: Commands) {
    for index in 0..NUM_SENSORS {
        commands.spawn((
            SensorConeMarker,
            SensorConePart::Line(index),
            Sprite {
                color: Color::srgba(0.0, 1.0, 0.0, SENSOR_CONE_LINE_ALPHA),
                custom_size: Some(Vec2::new(SENSOR_CONE_LINE_WIDTH, SENSOR_DISTANCE)),
                ..default()
            },
            Transform::default(),
            Visibility::Hidden,
        ));

        commands.spawn((
            SensorConeMarker,
            SensorConePart::Sensor(index),
            Sprite {
                color: Color::srgba(0.0, 1.0, 0.0, SENSOR_CONE_MARKER_ALPHA),
                custom_size: Some(Vec2::new(SENSOR_CONE_MARKER_SIZE, SENSOR_CONE_MARKER_SIZE)),
                ..default()
            },
            Transform::default(),
            Visibility::Hidden,
        ));
    }

    commands.spawn((
        SensorConeMarker,
        SensorConePart::Ant,
        Sprite {
            color: Color::srgba(1.0, 0.0, 0.0, SENSOR_CONE_ANT_ALPHA),
            custom_size: Some(Vec2::new(
                SENSOR_CONE_ANT_MARKER_SIZE,
                SENSOR_CONE_ANT_MARKER_SIZE,
            )),
            ..default()
        },
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Move the persistent cone entities over the selected ant. The selection is
/// re-rolled (without allocation) only when it disappears; with no valid
/// selection every part is hidden.
pub fn draw_sensor_cone(
    ant_query: AntQuery,
    mut selected_ant: ResMut<SelectedAnt>,
    mut parts: ConePartsQuery,
) {
    let Some(selected) = selected_ant.entity else {
        hide_all(&mut parts);
        return;
    };

    let entity = if ant_query.contains(selected) {
        selected
    } else {
        // The selected ant despawned: fall back to a fresh random ant.
        let picked = random_ant(ant_query.iter().map(|(entity, _, _)| entity));
        selected_ant.entity = picked;

        let Some(picked) = picked else {
            hide_all(&mut parts);
            return;
        };

        picked
    };

    let Ok((_, ant, ant_transform)) = ant_query.get(entity) else {
        return;
    };

    let ant_pos = Vec2::new(ant_transform.translation.x, ant_transform.translation.y);
    let sensor_step = (2.0 * SENSOR_ANGLE) / (NUM_SENSORS - 1) as f32;

    for (part, mut part_transform, mut visibility) in &mut parts {
        *visibility = Visibility::Visible;

        match *part {
            SensorConePart::Line(index) => {
                let angle_offset = -SENSOR_ANGLE + index as f32 * sensor_step;
                let check_angle = ant.direction + angle_offset;
                let (sin, cos) = check_angle.sin_cos();

                *part_transform = Transform::from_xyz(
                    ant_pos.x + (cos * SENSOR_DISTANCE / 2.0),
                    ant_pos.y + (sin * SENSOR_DISTANCE / 2.0),
                    Z_SENSOR_CONE_LINE,
                )
                .with_rotation(Quat::from_rotation_z(check_angle - PI / 2.0));
            }
            SensorConePart::Sensor(index) => {
                let angle_offset = -SENSOR_ANGLE + index as f32 * sensor_step;
                let check_angle = ant.direction + angle_offset;
                let (sin, cos) = check_angle.sin_cos();

                *part_transform = Transform::from_xyz(
                    ant_pos.x + cos * SENSOR_DISTANCE,
                    ant_pos.y + sin * SENSOR_DISTANCE,
                    Z_SENSOR_CONE_MARKER,
                );
            }
            SensorConePart::Ant => {
                *part_transform = Transform::from_xyz(ant_pos.x, ant_pos.y, Z_SENSOR_CONE_MARKER);
            }
        }
    }
}

/// Hide every persistent cone part without despawning it.
fn hide_all(parts: &mut ConePartsQuery) {
    for (_, _, mut visibility) in parts.iter_mut() {
        *visibility = Visibility::Hidden;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::ant::AntPhase;
    use bevy::ecs::system::RunSystemOnce;

    fn test_ant() -> Ant {
        Ant {
            direction: 0.0,
            has_food: false,
            home: Vec2::ZERO,
            age: 1.0,
            max_lifetime: 1.0,
            energy: 1.0,
            phase: AntPhase::Foraging,
            handling_timer: 0.0,
            base_speed: 1.0,
            speed: 1.0,
            trips_completed: 0,
        }
    }

    #[test]
    fn cone_follows_selected_ant_and_hides_without_it() {
        let mut world = World::new();
        world.insert_resource(SelectedAnt { entity: None });
        let ant = world
            .spawn((test_ant(), Transform::from_xyz(10.0, 20.0, 0.0)))
            .id();
        let part = world
            .spawn((
                SensorConePart::Ant,
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();

        // No selection: nothing is shown and no ant is picked.
        world.run_system_once(draw_sensor_cone).unwrap();
        assert!(world.resource::<SelectedAnt>().entity.is_none());
        assert_eq!(*world.get::<Visibility>(part).unwrap(), Visibility::Hidden);

        // A live selection is followed.
        world.resource_mut::<SelectedAnt>().entity = Some(ant);
        world.run_system_once(draw_sensor_cone).unwrap();
        assert_eq!(*world.get::<Visibility>(part).unwrap(), Visibility::Visible);
        assert_eq!(
            world.get::<Transform>(part).unwrap().translation,
            Vec3::new(10.0, 20.0, Z_SENSOR_CONE_MARKER)
        );

        // A despawned selection is re-rolled to a live ant.
        world.despawn(ant);
        let replacement = world
            .spawn((test_ant(), Transform::from_xyz(1.0, 2.0, 0.0)))
            .id();
        world.run_system_once(draw_sensor_cone).unwrap();
        assert_eq!(world.resource::<SelectedAnt>().entity, Some(replacement));

        // Clearing the selection hides the cone again.
        world.resource_mut::<SelectedAnt>().entity = None;
        world.run_system_once(draw_sensor_cone).unwrap();
        assert_eq!(*world.get::<Visibility>(part).unwrap(), Visibility::Hidden);
    }
}
