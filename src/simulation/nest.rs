//! Nest structure: entrance and refuse-pile geometry.
//!
//! [`NestGeometry`] is kept on the nest rim: the entrance faces the initial
//! food patch ([`FOOD_X`]/[`FOOD_Y`]) and the refuse pile sits on the opposite
//! rim. It reads the authoritative [`NestPosition`] and runs in `Update` after
//! [`GameSet::Editor`], so a dragged nest gets its new entrance in the same
//! frame (also while paused).
//!
//! The entrance bottleneck itself (spawning at the entrance, requiring dropoff
//! within [`ENTRANCE_RADIUS`]) is owned by the colony stream, which reads
//! [`NestGeometry`] from `collide.rs`/`ant.rs`; this module only publishes the
//! geometry. The default stays collapsed onto the nest centre (full-radius
//! mouth) for harnesses that have no nest entity, preserving the old disc-only
//! behavior until a nest exists.
//!
//! Obstacles and terrain (F16) are staged for a later milestone; see
//! [`crate::simulation::environment`] for where the obstacle data will live.

use bevy::prelude::*;

use crate::constants::environment::ENTRANCE_RADIUS;
use crate::constants::world::{
    FOOD_X, FOOD_Y, NEST_RADIUS, NEST_X, NEST_Y, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH,
};
use crate::core::sets::GameSet;
use crate::simulation::{Nest, NestPosition};

/// Where the nest entrance and refuse pile sit in world space.
///
/// The default collapses the entrance and the refuse pile onto the nest centre
/// and gives the entrance the full nest radius, which reproduces today's
/// disc-only dropoff/spawn behavior for harnesses without a nest entity. The
/// nest-geometry system overwrites it with rim geometry as soon as a [`Nest`]
/// entity exists.
#[derive(Resource)]
pub struct NestGeometry {
    /// World position of the nest entrance.
    pub entrance: Vec2,
    /// Radius around [`NestGeometry::entrance`] treated as the nest mouth.
    pub entrance_radius: f32,
    /// World position of the refuse pile.
    pub refuse: Vec2,
}

impl Default for NestGeometry {
    fn default() -> Self {
        let center = Vec2::new(NEST_X, NEST_Y);

        Self {
            entrance: center,
            entrance_radius: NEST_RADIUS,
            refuse: center,
        }
    }
}

/// Unit direction from `center` toward the initial food patch.
///
/// Falls back to `+x` when the nest sits exactly on the food patch, where the
/// direction is undefined.
fn entrance_direction(center: Vec2) -> Vec2 {
    (Vec2::new(FOOD_X, FOOD_Y) - center)
        .try_normalize()
        .unwrap_or(Vec2::X)
}

/// A point on the nest rim in `direction` from `center`, clamped to the play
/// area so directly-written (unclamped) nest positions cannot push the
/// entrance or refuse pile through a wall.
fn rim_point(center: Vec2, direction: Vec2) -> Vec2 {
    let half = Vec2::new(PLAY_AREA_WIDTH / 2.0, PLAY_AREA_HEIGHT / 2.0);
    (center + direction * NEST_RADIUS).clamp(-half, half)
}

/// Recompute [`NestGeometry`] from [`NestPosition`] whenever the nest moves.
///
/// The entrance is the rim point toward the initial food patch and the refuse
/// pile the opposite rim point; [`NestGeometry::entrance_radius`] becomes
/// [`ENTRANCE_RADIUS`]. Fields are compared before writing so an unmoved nest
/// does not mark the resource changed. Without a nest entity the geometry is
/// left untouched, which keeps [`NestGeometry::default`] (the old disc-only
/// behavior) for headless harnesses.
fn update_nest_geometry(
    nest: Query<(), With<Nest>>,
    nest_position: Res<NestPosition>,
    mut geometry: ResMut<NestGeometry>,
) {
    if nest.is_empty() {
        return;
    }

    let center = nest_position.0;
    let direction = entrance_direction(center);
    let entrance = rim_point(center, direction);
    let refuse = rim_point(center, -direction);

    if geometry.entrance != entrance {
        geometry.entrance = entrance;
    }
    if geometry.refuse != refuse {
        geometry.refuse = refuse;
    }
    if geometry.entrance_radius != ENTRANCE_RADIUS {
        geometry.entrance_radius = ENTRANCE_RADIUS;
    }
}

/// Wiring hook for the environment stream: registers the nest-geometry system
/// in `Update`, ordered after the editor's nest moves ([`GameSet::Editor`]).
pub fn register(app: &mut App) {
    app.add_systems(Update, update_nest_geometry.after(GameSet::Editor));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bare app with the nest resources and the register hook, mirroring the
    /// real `Update` ordering (the editor set is configured in `main.rs`).
    fn build_app(with_nest: bool) -> App {
        let mut app = App::new();
        app.init_resource::<NestPosition>()
            .init_resource::<NestGeometry>()
            .configure_sets(Update, GameSet::Editor);
        register(&mut app);

        if with_nest {
            app.world_mut()
                .spawn((Nest, Transform::from_xyz(NEST_X, NEST_Y, 0.0)));
        }

        app
    }

    #[test]
    fn entrance_faces_the_initial_food_and_refuse_is_opposite() {
        let mut app = build_app(true);
        app.update();

        let geometry = app.world().resource::<NestGeometry>();

        // The initial food patch is due east of the initial nest.
        assert_eq!(geometry.entrance, Vec2::new(NEST_X + NEST_RADIUS, NEST_Y));
        assert_eq!(geometry.refuse, Vec2::new(NEST_X - NEST_RADIUS, NEST_Y));
        assert_eq!(geometry.entrance_radius, ENTRANCE_RADIUS);

        let toward_food = (Vec2::new(FOOD_X, FOOD_Y) - Vec2::new(NEST_X, NEST_Y)).normalize();
        assert!(
            (geometry.entrance - Vec2::new(NEST_X, NEST_Y))
                .normalize()
                .dot(toward_food)
                > 0.999
        );
    }

    #[test]
    fn geometry_follows_a_moved_nest() {
        let mut app = build_app(true);
        app.update();

        let center = Vec2::new(0.0, 100.0);
        app.world_mut().resource_mut::<NestPosition>().set(center);
        app.update();

        let geometry = app.world().resource::<NestGeometry>();
        let toward_food = (Vec2::new(FOOD_X, FOOD_Y) - center).normalize();

        assert!(
            ((geometry.entrance - center).length() - NEST_RADIUS).abs() < 1e-4,
            "entrance must stay on the rim"
        );
        assert!(
            (geometry.entrance - center).normalize().dot(toward_food) > 0.999,
            "entrance must face the initial food patch after the move"
        );
        assert!(
            (geometry.refuse - center).normalize().dot(-toward_food) > 0.999,
            "refuse must stay on the opposite rim"
        );
        assert_eq!(geometry.entrance_radius, ENTRANCE_RADIUS);
    }

    #[test]
    fn defaults_are_preserved_without_a_nest_entity() {
        let mut app = build_app(false);

        // Even a moved authoritative position must not write geometry when
        // there is no nest entity to attach it to.
        app.world_mut()
            .resource_mut::<NestPosition>()
            .set(Vec2::new(0.0, 100.0));
        app.update();

        let geometry = app.world().resource::<NestGeometry>();
        let default = NestGeometry::default();

        assert_eq!(geometry.entrance, default.entrance);
        assert_eq!(geometry.refuse, default.refuse);
        assert_eq!(geometry.entrance_radius, default.entrance_radius);
    }

    #[test]
    fn rim_points_stay_inside_the_play_area() {
        let mut app = build_app(true);

        // A clamped nest corner: the rim points are still on the wall at most.
        let corner = Vec2::new(
            PLAY_AREA_WIDTH / 2.0 - NEST_RADIUS,
            PLAY_AREA_HEIGHT / 2.0 - NEST_RADIUS,
        );
        app.world_mut().resource_mut::<NestPosition>().set(corner);
        app.update();

        let half = Vec2::new(PLAY_AREA_WIDTH / 2.0, PLAY_AREA_HEIGHT / 2.0);
        let geometry = app.world().resource::<NestGeometry>();

        for point in [geometry.entrance, geometry.refuse] {
            assert!(
                point.abs().cmple(half).all(),
                "rim point {point} escaped the play area"
            );
        }
    }
}
