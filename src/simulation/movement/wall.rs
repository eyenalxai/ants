//! Play-area wall collision and bounce response.

use bevy::prelude::*;

use crate::constants::world::{PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH, WALL_BOUNCE_MIN_ANGLE};
use crate::simulation::ant::Ant;

/// The four play-area walls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallSide {
    Left,
    Right,
    Bottom,
    Top,
}

impl WallSide {
    /// Unit vector pointing from the wall into the play area.
    fn inward_normal(self) -> Vec2 {
        match self {
            WallSide::Left => Vec2::X,
            WallSide::Right => Vec2::NEG_X,
            WallSide::Bottom => Vec2::Y,
            WallSide::Top => Vec2::NEG_Y,
        }
    }
}

/// Reflect `incoming` off `wall` and keep the outgoing heading at least
/// [`WALL_BOUNCE_MIN_ANGLE`] away from the wall plane (no wall-hugging).
///
/// The sign of the velocity component parallel to the wall is preserved; the
/// normal component always ends up pointing back into the play area.
pub fn bounce_direction(incoming: f32, wall: WallSide) -> f32 {
    let velocity = Vec2::new(incoming.cos(), incoming.sin());
    let normal = wall.inward_normal();
    let reflected = velocity - 2.0 * velocity.dot(normal) * normal;

    let min_inward = WALL_BOUNCE_MIN_ANGLE.to_radians().sin();
    let inward = reflected.dot(normal).max(min_inward);

    let tangent = Vec2::new(-normal.y, normal.x);
    let tangent_sign = if reflected.dot(tangent) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let tangent_component = tangent_sign * (1.0 - inward * inward).max(0.0).sqrt();

    let outgoing = normal * inward + tangent * tangent_component;
    outgoing.y.atan2(outgoing.x)
}

/// Clamp the ant to the play area and bounce its heading off any wall crossed.
pub fn handle_wall_collision(ant: &mut Ant, transform: &mut Transform) {
    let half_width = PLAY_AREA_WIDTH / 2.0;
    let half_height = PLAY_AREA_HEIGHT / 2.0;

    if transform.translation.x > half_width {
        transform.translation.x = half_width;
        ant.direction = bounce_direction(ant.direction, WallSide::Right);
    } else if transform.translation.x < -half_width {
        transform.translation.x = -half_width;
        ant.direction = bounce_direction(ant.direction, WallSide::Left);
    }

    if transform.translation.y > half_height {
        transform.translation.y = half_height;
        ant.direction = bounce_direction(ant.direction, WallSide::Top);
    } else if transform.translation.y < -half_height {
        transform.translation.y = -half_height;
        ant.direction = bounce_direction(ant.direction, WallSide::Bottom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const EPS: f32 = 1e-4;

    fn direction_vector(direction: f32) -> Vec2 {
        Vec2::new(direction.cos(), direction.sin())
    }

    fn angle_from_wall_plane(direction: f32, wall: WallSide) -> f32 {
        let normal = wall.inward_normal();
        direction_vector(direction)
            .dot(normal)
            .abs()
            .clamp(0.0, 1.0)
            .asin()
    }

    fn tangent_component(direction: f32, wall: WallSide) -> f32 {
        let normal = wall.inward_normal();
        let tangent = Vec2::new(-normal.y, normal.x);
        direction_vector(direction).dot(tangent)
    }

    /// Headings that actually travel into each wall.
    fn incoming_headings(wall: WallSide) -> [f32; 5] {
        match wall {
            WallSide::Right => [0.0, 0.5, 1.0, -0.5, -1.0],
            WallSide::Left => [PI, PI - 0.5, PI + 0.5, 2.5, -2.5],
            WallSide::Top => [PI / 2.0, 0.5, PI - 0.5, 1.0, 2.0],
            WallSide::Bottom => [-PI / 2.0, -0.5, 0.5 - PI, -1.0, -2.0],
        }
    }

    #[test]
    fn bounces_never_hug_any_wall() {
        let min_angle = WALL_BOUNCE_MIN_ANGLE.to_radians();

        for wall in [
            WallSide::Left,
            WallSide::Right,
            WallSide::Bottom,
            WallSide::Top,
        ] {
            for incoming in incoming_headings(wall) {
                let outgoing = bounce_direction(incoming, wall);
                let away = angle_from_wall_plane(outgoing, wall);

                assert!(
                    away >= min_angle - EPS,
                    "{wall:?}: incoming {incoming} -> outgoing {outgoing} leaves only {away} rad"
                );

                // Outgoing must point back into the play area.
                assert!(
                    direction_vector(outgoing).dot(wall.inward_normal()) >= 0.0,
                    "{wall:?}: outgoing {outgoing} still points out of the play area"
                );

                // The parallel component keeps its sign.
                let before = tangent_component(incoming, wall);
                let after = tangent_component(outgoing, wall);
                if before.abs() > 1e-3 && after.abs() > 1e-3 {
                    assert!(
                        before.signum() == after.signum(),
                        "{wall:?}: parallel sign flipped ({before} -> {after})"
                    );
                }
            }
        }
    }

    #[test]
    fn perpendicular_bounces_flip_the_normal_component() {
        assert!((bounce_direction(0.0, WallSide::Right) - PI).abs() < EPS);
        assert!((bounce_direction(PI, WallSide::Left)).abs() < EPS);
        assert!((bounce_direction(PI / 2.0, WallSide::Top) + PI / 2.0).abs() < EPS);
        assert!((bounce_direction(-PI / 2.0, WallSide::Bottom) - PI / 2.0).abs() < EPS);
    }

    #[test]
    fn grazing_bounce_is_pushed_inward() {
        // Almost parallel to the right wall and moving rightwards.
        let incoming = -0.05_f32;
        let outgoing = bounce_direction(incoming, WallSide::Right);

        assert!(
            angle_from_wall_plane(outgoing, WallSide::Right)
                >= WALL_BOUNCE_MIN_ANGLE.to_radians() - EPS
        );
        assert!(
            outgoing.sin() < 0.0,
            "parallel direction should be preserved"
        );
        assert!(
            outgoing.cos() < 0.0,
            "outgoing heading must point back inside"
        );
    }

    #[test]
    fn stale_outward_heading_is_forced_back_in() {
        // Ant already moving away from the right wall but still overlapping it.
        let outgoing = bounce_direction(PI, WallSide::Right);
        assert!(
            direction_vector(outgoing).dot(WallSide::Right.inward_normal()) >= min_inward_epsilon()
        );
    }

    fn min_inward_epsilon() -> f32 {
        (WALL_BOUNCE_MIN_ANGLE.to_radians().sin() - EPS).max(0.0)
    }
}
