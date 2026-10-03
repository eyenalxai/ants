//! Debug overlay for the selected ant's three-ring sensor fan.

use bevy::prelude::*;
use std::f32::consts::PI;

use crate::constants::sensor::*;
use crate::core::grid::world_to_grid;
use crate::core::layers::{Z_SENSOR_CONE_LINE, Z_SENSOR_CONE_MARKER};
use crate::overlays::pheromone::normalized_visual;
use crate::overlays::{SelectedAnt, random_ant};
use crate::pheromone::grid::{PheromoneGrid, PheromoneKind};
use crate::simulation::ant::Ant;
// Canonical sensor-angle helper (F9): the simulation owns the formula, the
// overlay only renders it. If that signature moves, this import is the one
// place to update.
use crate::simulation::movement::sensors::sensor_offset;

/// Marker for every persistent sensor-cone entity.
#[derive(Component)]
pub struct SensorConeMarker;

/// Which piece of the cone a persistent entity renders.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorConePart {
    /// Thin line from the ant to sensor `usize` on the outer ring.
    Line(usize),
    /// Sampling marker for ray `ray` on ring `ring` (an index into
    /// [`SENSOR_RING_DISTANCES`]).
    Sensor {
        /// Ray index in `0..NUM_SENSORS`.
        ray: usize,
        /// Ring index in `0..SENSOR_RING_DISTANCES.len()`.
        ring: usize,
    },
    /// Red dot on the selected ant itself.
    Ant,
}

/// Cached visibility of the persistent cone entities. While it is `false`,
/// [`draw_sensor_cone`] can return without iterating (or re-marking) any of
/// them when there is no selection.
#[derive(Resource, Default)]
pub struct SensorConeState {
    visible: bool,
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
        &'static mut Sprite,
        &'static mut Visibility,
    ),
    (With<SensorConePart>, Without<Ant>),
>;

/// Spawn the sensor cone once; [`draw_sensor_cone`] only moves and recolors
/// the entities. One line per ray plus one marker per ray and ring
/// ([`NUM_SENSORS`] x [`SENSOR_RING_DISTANCES`]).
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
    }

    for ring in 0..SENSOR_RING_DISTANCES.len() {
        for ray in 0..NUM_SENSORS {
            commands.spawn((
                SensorConeMarker,
                SensorConePart::Sensor { ray, ring },
                Sprite {
                    color: Color::srgba(0.0, 1.0, 0.0, SENSOR_CONE_MARKER_ALPHA),
                    custom_size: Some(Vec2::new(SENSOR_CONE_MARKER_SIZE, SENSOR_CONE_MARKER_SIZE)),
                    ..default()
                },
                Transform::default(),
                Visibility::Hidden,
            ));
        }
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
/// selection every part is hidden exactly once, so idle frames do not touch
/// (or re-mark) any of the 28 cone entities.
pub fn draw_sensor_cone(
    ant_query: AntQuery,
    mut selected_ant: ResMut<SelectedAnt>,
    pheromone_grid: Res<PheromoneGrid>,
    mut state: ResMut<SensorConeState>,
    mut parts: ConePartsQuery,
) {
    let Some(selected) = selected_ant.entity else {
        hide_all(&mut parts, &mut state);
        return;
    };

    let entity = if ant_query.contains(selected) {
        selected
    } else {
        // The selected ant despawned: fall back to a fresh random ant.
        let picked = random_ant(ant_query.iter().map(|(entity, _, _)| entity));
        selected_ant.entity = picked;

        let Some(picked) = picked else {
            hide_all(&mut parts, &mut state);
            return;
        };

        picked
    };

    let Ok((_, ant, ant_transform)) = ant_query.get(entity) else {
        return;
    };

    let ant_pos = Vec2::new(ant_transform.translation.x, ant_transform.translation.y);
    // Mirror the simulation's channel choice for the selected ant so the
    // markers show the same field the ant is following.
    let kind = if ant.follows_to_nest() {
        PheromoneKind::ToNest
    } else {
        PheromoneKind::ToFood
    };

    // Only write `Visibility` on the hidden -> visible transition.
    let was_visible = state.visible;

    if !was_visible {
        state.visible = true;
    }

    for (part, mut part_transform, mut sprite, mut visibility) in &mut parts {
        if !was_visible {
            *visibility = Visibility::Visible;
        }

        match *part {
            SensorConePart::Line(index) => {
                let check_angle = ant.direction + sensor_offset(index);
                let (sin, cos) = check_angle.sin_cos();

                *part_transform = Transform::from_xyz(
                    ant_pos.x + (cos * SENSOR_DISTANCE / 2.0),
                    ant_pos.y + (sin * SENSOR_DISTANCE / 2.0),
                    Z_SENSOR_CONE_LINE,
                )
                .with_rotation(Quat::from_rotation_z(check_angle - PI / 2.0));
            }
            SensorConePart::Sensor { ray, ring } => {
                let distance = SENSOR_RING_DISTANCES
                    .get(ring)
                    .copied()
                    .unwrap_or(SENSOR_DISTANCE);
                let check_angle = ant.direction + sensor_offset(ray);
                let (sin, cos) = check_angle.sin_cos();
                let position = ant_pos + Vec2::new(cos, sin) * distance;

                *part_transform = Transform::from_xyz(position.x, position.y, Z_SENSOR_CONE_MARKER);

                let raw =
                    world_to_grid(position).map_or(0.0, |cell| pheromone_grid.sample(cell, kind));

                sprite.color = sample_marker_color(kind, raw);
            }
            SensorConePart::Ant => {
                *part_transform = Transform::from_xyz(ant_pos.x, ant_pos.y, Z_SENSOR_CONE_MARKER);
            }
        }
    }
}

/// Tint for one sampled marker: the channel's primary color (red for
/// [`PheromoneKind::ToFood`], blue for [`PheromoneKind::ToNest`]) scaled by the
/// shared visual curve, fading a dim green base out as the reading grows so
/// empty markers stay visible.
pub fn sample_marker_color(kind: PheromoneKind, raw: f32) -> Color {
    let value = normalized_visual(raw);
    let base = 0.35 * (1.0 - value);

    match kind {
        PheromoneKind::ToFood => Color::srgba(value, base, 0.0, SENSOR_CONE_MARKER_ALPHA),
        PheromoneKind::ToNest => Color::srgba(0.0, base, value, SENSOR_CONE_MARKER_ALPHA),
    }
}

/// Hide every persistent cone part without despawning it. Later calls while
/// already hidden are no-ops, so the entities are not re-marked every frame.
fn hide_all(parts: &mut ConePartsQuery, state: &mut SensorConeState) {
    if !state.visible {
        return;
    }

    state.visible = false;

    for (_, _, _, mut visibility) in parts.iter_mut() {
        *visibility = Visibility::Hidden;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::pheromone::PHEROMONE_VISUAL_SCALE;
    use bevy::ecs::change_detection::Tick;
    use bevy::ecs::system::RunSystemOnce;

    const EPS: f32 = 1e-6;

    fn spawn_cone_world() -> World {
        let mut world = World::new();
        world.init_resource::<PheromoneGrid>();
        world.insert_resource(SelectedAnt { entity: None });
        world.insert_resource(SensorConeState::default());
        world.run_system_once(setup_sensor_cone).unwrap();

        world
    }

    fn visibility_tick(world: &World, entity: Entity) -> Tick {
        world
            .entity(entity)
            .get_change_ticks::<Visibility>()
            .expect("cone part has visibility")
            .changed
    }

    #[test]
    fn setup_spawns_nine_rays_across_three_rings() {
        let mut world = spawn_cone_world();

        let mut ring_markers = 0;
        let mut lines = 0;
        let mut ant_markers = 0;
        let mut query = world.query::<&SensorConePart>();

        for part in query.iter(&world) {
            match *part {
                SensorConePart::Sensor { ray, ring } => {
                    assert!(ray < NUM_SENSORS);
                    assert!(ring < SENSOR_RING_DISTANCES.len());
                    ring_markers += 1;
                }
                SensorConePart::Line(_) => lines += 1,
                SensorConePart::Ant => ant_markers += 1,
            }
        }

        assert_eq!(ring_markers, NUM_SENSORS * SENSOR_RING_DISTANCES.len());
        assert_eq!(ring_markers, 27);
        assert_eq!(lines, NUM_SENSORS);
        assert_eq!(ant_markers, 1);
    }

    #[test]
    fn marker_tint_uses_the_visual_curve_per_channel() {
        let empty_food = sample_marker_color(PheromoneKind::ToFood, 0.0).to_srgba();
        assert!((empty_food.red - 0.0).abs() < EPS);
        assert!((empty_food.green - 0.35).abs() < EPS);
        assert!((empty_food.blue - 0.0).abs() < EPS);

        let full_food =
            sample_marker_color(PheromoneKind::ToFood, PHEROMONE_VISUAL_SCALE).to_srgba();
        assert!((full_food.red - 1.0).abs() < EPS);
        assert!((full_food.green - 0.0).abs() < EPS);

        let full_nest =
            sample_marker_color(PheromoneKind::ToNest, PHEROMONE_VISUAL_SCALE).to_srgba();
        assert!((full_nest.red - 0.0).abs() < EPS);
        assert!((full_nest.blue - 1.0).abs() < EPS);

        let half_food =
            sample_marker_color(PheromoneKind::ToFood, PHEROMONE_VISUAL_SCALE * 0.25).to_srgba();
        assert!((half_food.red - 0.5).abs() < EPS);
    }

    #[test]
    fn cone_markers_track_ring_positions_and_grid_readings() {
        let mut world = spawn_cone_world();

        // Raw intensity at the outer ring point straight ahead of an ant
        // facing +x: the center ray (index `NUM_SENSORS / 2`) has offset 0.
        let target = Vec2::new(SENSOR_RING_DISTANCES[2], 0.0);
        let cell = world_to_grid(target).expect("target is inside the play area");
        world
            .resource_mut::<PheromoneGrid>()
            .add(cell, PHEROMONE_VISUAL_SCALE, 0.0);

        let ant = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        world.resource_mut::<SelectedAnt>().entity = Some(ant);

        world.run_system_once(draw_sensor_cone).unwrap();

        let mut outer_center = None;
        let mut inner_center = None;
        let mut query = world.query::<(&SensorConePart, &Transform, &Sprite)>();

        for (part, transform, sprite) in query.iter(&world) {
            if let SensorConePart::Sensor { ray, ring } = *part
                && ray == NUM_SENSORS / 2
            {
                match ring {
                    0 => inner_center = Some((transform.translation, sprite.color)),
                    2 => outer_center = Some((transform.translation, sprite.color)),
                    _ => {}
                }
            }
        }

        let (outer_transform, outer_color) = outer_center.expect("outer center marker");
        assert!((outer_transform.x - SENSOR_RING_DISTANCES[2]).abs() < EPS);
        assert!(outer_transform.y.abs() < EPS);

        let outer_srgba = outer_color.to_srgba();
        assert!((outer_srgba.red - 1.0).abs() < EPS);
        assert!((outer_srgba.green - 0.0).abs() < EPS);

        let (inner_transform, inner_color) = inner_center.expect("inner center marker");
        assert!((inner_transform.x - SENSOR_RING_DISTANCES[0]).abs() < EPS);

        let inner_srgba = inner_color.to_srgba();
        assert!((inner_srgba.red - 0.0).abs() < EPS);
        assert!((inner_srgba.green - 0.35).abs() < EPS);
    }

    #[test]
    fn sensor_offsets_span_the_configured_half_angle() {
        // Guards the shared formula the overlay renders (F9).
        assert!((sensor_offset(0) + SENSOR_ANGLE).abs() < EPS);
        assert!((sensor_offset(NUM_SENSORS - 1) - SENSOR_ANGLE).abs() < EPS);

        for index in 0..NUM_SENSORS {
            let offset = sensor_offset(index);
            assert!((-SENSOR_ANGLE - EPS..=SENSOR_ANGLE + EPS).contains(&offset));
        }
    }

    #[test]
    fn idle_cone_frames_do_not_touch_visibility() {
        let mut world = spawn_cone_world();

        let part = {
            let mut query = world.query_filtered::<Entity, With<SensorConePart>>();
            query.iter(&world).next().expect("cone parts")
        };

        // The cone starts hidden; the first idle frame must not re-mark it.
        world.run_system_once(draw_sensor_cone).unwrap();
        let hidden_tick = visibility_tick(&world, part);

        for _ in 0..3 {
            world.run_system_once(draw_sensor_cone).unwrap();
        }

        assert_eq!(hidden_tick, visibility_tick(&world, part));

        // While an ant is selected the visibility transition happens once;
        // later frames only move the parts.
        let ant = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        world.resource_mut::<SelectedAnt>().entity = Some(ant);

        world.run_system_once(draw_sensor_cone).unwrap();
        assert_eq!(*world.get::<Visibility>(part).unwrap(), Visibility::Visible);
        let visible_tick = visibility_tick(&world, part);

        world.run_system_once(draw_sensor_cone).unwrap();
        assert_eq!(visible_tick, visibility_tick(&world, part));
    }

    #[test]
    fn cone_follows_selected_ant_and_hides_without_it() {
        let mut world = World::new();
        world.init_resource::<PheromoneGrid>();
        world.insert_resource(SelectedAnt { entity: None });
        world.insert_resource(SensorConeState::default());
        let ant = world
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(10.0, 20.0, 0.0)))
            .id();
        let part = world
            .spawn((
                SensorConePart::Ant,
                Transform::default(),
                Sprite::default(),
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
            .spawn((Ant::test_ant(0.0), Transform::from_xyz(1.0, 2.0, 0.0)))
            .id();
        world.run_system_once(draw_sensor_cone).unwrap();
        assert_eq!(world.resource::<SelectedAnt>().entity, Some(replacement));

        // Clearing the selection hides the cone again.
        world.resource_mut::<SelectedAnt>().entity = None;
        world.run_system_once(draw_sensor_cone).unwrap();
        assert_eq!(*world.get::<Visibility>(part).unwrap(), Visibility::Hidden);
    }
}
