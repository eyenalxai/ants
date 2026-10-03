//! Play-area wall collision and obstacle bounce response.

use bevy::prelude::*;

use crate::constants::environment::{OBSTACLE_ANT_RADIUS, OBSTACLE_MIN_SEPARATION};
use crate::constants::world::{PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH, WALL_BOUNCE_MIN_ANGLE};
use crate::simulation::ant::Ant;
use crate::simulation::environment::Obstacles;

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

/// Reflect `incoming` off a surface whose outward unit `normal` points into
/// free space, keeping the outgoing heading at least [`WALL_BOUNCE_MIN_ANGLE`]
/// away from the surface plane (no surface-hugging).
///
/// The sign of the velocity component parallel to the surface is preserved;
/// the normal component always ends up positive. This is the general form of
/// the wall bounce and is reused for arbitrary obstacle normals.
pub fn reflect_direction(incoming: f32, normal: Vec2) -> f32 {
    let velocity = Vec2::new(incoming.cos(), incoming.sin());
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

/// Reflect `incoming` off `wall` and keep the outgoing heading at least
/// [`WALL_BOUNCE_MIN_ANGLE`] away from the wall plane (no wall-hugging).
///
/// The sign of the velocity component parallel to the wall is preserved; the
/// normal component always ends up pointing back into the play area.
pub fn bounce_direction(incoming: f32, wall: WallSide) -> f32 {
    reflect_direction(incoming, wall.inward_normal())
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

/// Push the ant out of any obstacle it overlaps and reflect its heading.
///
/// Runs after the movement step (before [`handle_wall_collision`], so the
/// play-area clamp always has the last word) and again after the contact
/// separation correction. The push is the shortest separation along the
/// obstacle normal plus [`OBSTACLE_MIN_SEPARATION`], which keeps the ant just
/// clear of the surface; a maximum move of
/// [`crate::constants::ant::ANT_SPEED`] × `dt` (0.78 u at 64 Hz) is far smaller
/// than the ant radius, so an ant cannot tunnel through an obstacle that is at
/// least [`crate::constants::ant::ANT_SIZE`] thick.
///
/// Cost: with no obstacles this is a single empty-slice check. With `k`
/// obstacles the pass is `O(k)` per ant (a distance-squared broad-phase check
/// per obstacle); only ants whose bounding circles actually overlap pay for
/// the narrow-phase push-out.
pub fn handle_obstacle_collision(ant: &mut Ant, transform: &mut Transform, obstacles: &Obstacles) {
    if obstacles.is_empty() {
        return;
    }

    let mut pos = Vec2::new(transform.translation.x, transform.translation.y);

    for obstacle in obstacles.iter() {
        // Broad phase: skip obstacles that cannot touch the ant body.
        let reach = obstacle.bounding_radius() + OBSTACLE_ANT_RADIUS;
        if (obstacle.center() - pos).length_squared() > reach * reach {
            continue;
        }

        let Some((normal, depth)) = obstacle.penetration(pos, OBSTACLE_ANT_RADIUS) else {
            continue;
        };

        let push = depth + OBSTACLE_MIN_SEPARATION;
        pos += normal * push;
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        ant.direction = reflect_direction(ant.direction, normal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::environment::Obstacle;
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

    #[test]
    fn reflect_direction_generalizes_the_wall_bounce() {
        let normals = [
            Vec2::X,
            Vec2::NEG_X,
            Vec2::Y,
            Vec2::NEG_Y,
            Vec2::new(1.0, 1.0).normalize(),
            Vec2::new(-2.0, 1.0).normalize(),
        ];
        let min_inward = WALL_BOUNCE_MIN_ANGLE.to_radians().sin();

        for normal in normals {
            for step in 0..32 {
                let incoming = -PI + step as f32 * (2.0 * PI / 32.0);
                let outgoing = reflect_direction(incoming, normal);
                let velocity = Vec2::new(outgoing.cos(), outgoing.sin());

                assert!(
                    velocity.dot(normal) >= min_inward - EPS,
                    "normal {normal}: incoming {incoming} leaves only {}",
                    velocity.dot(normal)
                );
            }
        }

        // The wall-facing helper is exactly the general form with the wall
        // normal, so every existing wall contract is preserved.
        for wall in [
            WallSide::Left,
            WallSide::Right,
            WallSide::Bottom,
            WallSide::Top,
        ] {
            for incoming in incoming_headings(wall) {
                assert_eq!(
                    bounce_direction(incoming, wall),
                    reflect_direction(incoming, wall.inward_normal())
                );
            }
        }
    }

    #[test]
    fn obstacle_overlap_is_pushed_out_and_reflected() {
        let obstacles = Obstacles::new(vec![Obstacle::aabb(Vec2::ZERO, Vec2::new(10.0, 10.0))]);

        // Deep inside the box, heading east into the +x face.
        let mut ant = Ant::test_ant(0.0);
        let mut transform = Transform::from_xyz(0.0, 0.0, 0.0);
        handle_obstacle_collision(&mut ant, &mut transform, &obstacles);

        let pos = transform.translation.truncate();
        assert!(
            pos.x >= 10.0 + OBSTACLE_ANT_RADIUS,
            "the ant must leave the box through a face, got {pos}"
        );
        assert!(
            (pos.y).abs() < EPS,
            "the shallowest face is +x, so y must not move, got {pos}"
        );

        let heading = Vec2::new(ant.direction.cos(), ant.direction.sin());
        assert!(
            heading.x >= WALL_BOUNCE_MIN_ANGLE.to_radians().sin() - EPS,
            "the heading must point away from the +x face, got {heading}"
        );
        assert!(
            obstacles
                .iter()
                .all(|obstacle| obstacle.penetration(pos, OBSTACLE_ANT_RADIUS).is_none()),
            "the ant must end up clear of every obstacle"
        );
    }

    #[test]
    fn circle_obstacle_push_out_clears_the_surface() {
        let obstacles = Obstacles::new(vec![Obstacle::circle(Vec2::new(20.0, 0.0), 5.0)]);
        let mut ant = Ant::test_ant(PI);
        let mut transform = Transform::from_xyz(18.0, 0.0, 0.0);

        handle_obstacle_collision(&mut ant, &mut transform, &obstacles);

        let pos = transform.translation.truncate();
        let distance = (pos - Vec2::new(20.0, 0.0)).length();
        assert!(
            distance >= 5.0 + OBSTACLE_ANT_RADIUS,
            "the ant must be pushed to the disc surface, distance {distance}"
        );

        let heading = Vec2::new(ant.direction.cos(), ant.direction.sin());
        assert!(
            heading.dot((pos - Vec2::new(20.0, 0.0)).normalize())
                >= WALL_BOUNCE_MIN_ANGLE.to_radians().sin() - EPS,
            "the heading must point away from the disc, got {heading}"
        );
    }

    #[test]
    fn empty_obstacles_leave_the_ant_untouched() {
        let obstacles = Obstacles::empty();
        let mut ant = Ant::test_ant(1.25);
        let mut transform = Transform::from_xyz(3.0, -4.0, 0.0);

        handle_obstacle_collision(&mut ant, &mut transform, &obstacles);

        assert_eq!(transform.translation.truncate(), Vec2::new(3.0, -4.0));
        assert_eq!(ant.direction, 1.25);
    }

    /// Ignored micro-benchmark of the obstacle collision pass at 30k ants.
    /// Calls the pass twice per ant (the real chain resolves obstacles after
    /// the move and again after contact separation). Run with:
    /// `cargo test --release --locked --bin ants obstacle_collision_micro_bench -- --ignored --nocapture`
    #[test]
    #[ignore = "perf micro-benchmark, run on demand"]
    fn obstacle_collision_micro_bench_30k() {
        use crate::simulation::environment::Obstacle;
        use std::time::Instant;

        const ANTS: usize = 30_000;
        const TICKS: u32 = 100;

        let obstacles = Obstacles::new(vec![
            Obstacle::circle(Vec2::new(-120.0, 170.0), 34.0),
            Obstacle::aabb(Vec2::new(40.0, -180.0), Vec2::new(70.0, 12.0)),
            Obstacle::aabb(Vec2::new(240.0, 200.0), Vec2::new(14.0, 70.0)),
        ]);
        let empty = Obstacles::empty();

        let mut rng = fastrand::Rng::with_seed(0x0B57_AC1E);
        let mut ants: Vec<(Ant, Transform)> = (0..ANTS)
            .map(|_| {
                let ant = Ant::test_ant(rng.f32() * std::f32::consts::TAU);
                let pos = Vec2::new(rng.f32() * 760.0 - 380.0, rng.f32() * 560.0 - 280.0);
                (ant, Transform::from_xyz(pos.x, pos.y, 0.0))
            })
            .collect();

        let mut bench = |obstacles: &Obstacles| {
            for _ in 0..5 {
                for (ant, transform) in ants.iter_mut() {
                    handle_obstacle_collision(ant, transform, obstacles);
                    handle_obstacle_collision(ant, transform, obstacles);
                }
            }

            let start = Instant::now();

            for _ in 0..TICKS {
                for (ant, transform) in ants.iter_mut() {
                    handle_obstacle_collision(ant, transform, obstacles);
                    handle_obstacle_collision(ant, transform, obstacles);
                }
            }

            start.elapsed().as_secs_f64() * 1000.0 / f64::from(TICKS)
        };

        let empty_ms = bench(&empty);
        let obstacle_ms = bench(&obstacles);

        println!(
            "obstacle collision micro-bench: {ANTS} ants, {TICKS} ticks, 2 passes/tick, \
             empty {empty_ms:.3} ms/tick, {} obstacles {obstacle_ms:.3} ms/tick (+{:.3} ms/tick)",
            obstacles.len(),
            obstacle_ms - empty_ms
        );
    }
}
