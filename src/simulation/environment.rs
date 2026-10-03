//! Environment state: the simulation clock, its activity factor and static
//! obstacles (F16).
//!
//! [`SimClock::t`] advances one fixed step at a time in
//! [`crate::core::sets::SimSet::Clock`] and [`SimClock::activity`] follows a
//! smoothed day-night sinusoid over [`DAY_LENGTH`] with a floor of
//! [`ACTIVITY_MIN`]. The cycle starts at peak activity, so the short
//! regression runs (45 s trail check, 15 s determinism replay) see essentially
//! full activity while a longer session experiences dusk and dawn.
//!
//! Temperature is deliberately not modelled yet: activity is a pure,
//! deterministic function of `t` (no randomness, no hidden state), which keeps
//! replays bit-identical. It is the simple, documented version of F14.
//!
//! [`Obstacles`] is the static terrain: a short list of circles/AABBs with
//! pure geometry (push-out and segment intersection) that the movement stream
//! uses for collision response and the sensor stream for occlusion. The set is
//! immutable during a run, so the parallel movement pass can share it without
//! locks; only a handful of obstacles are expected, so every query is a linear
//! scan with a bounding-circle early out.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::constants::environment::{ACTIVITY_MIN, DAY_LENGTH};
use crate::core::layers::Z_OBSTACLE;
use crate::core::sets::{SimSet, StartupSet};

/// Simulated wall-clock time and the current foraging-activity factor.
#[derive(Resource)]
pub struct SimClock {
    /// Seconds of simulated time elapsed.
    pub t: f32,
    /// Foraging-activity multiplier in `[ACTIVITY_MIN, 1.0]`, recomputed from
    /// [`SimClock::t`] every fixed step.
    pub activity: f32,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            t: 0.0,
            // The cycle starts at its peak, so `default()` matches
            // `activity_at(0.0)` and the clock is neutral (1.0) until the first
            // fixed step.
            activity: 1.0,
        }
    }
}

/// Foraging activity at simulated time `t` seconds.
///
/// A raised cosine over [`DAY_LENGTH`] starting at peak (`t = 0` is noon,
/// `t = DAY_LENGTH / 2` is midnight), shaped by a smoothstep so activity
/// plateaus near its peak and near the floor instead of swinging linearly
/// through the middle. The result is always in `[ACTIVITY_MIN, 1.0]` and is
/// exactly periodic with [`DAY_LENGTH`].
///
/// `rem_euclid` folds `t` into one cycle, so precision does not degrade as
/// the session grows and the function stays periodic for arbitrarily large
/// `t`.
pub fn activity_at(t: f32) -> f32 {
    let phase = (t / DAY_LENGTH).rem_euclid(1.0);
    let wave = 0.5 + 0.5 * (TAU * phase).cos();
    let smoothed = wave * wave * (3.0 - 2.0 * wave);

    ACTIVITY_MIN + (1.0 - ACTIVITY_MIN) * smoothed
}

/// Advance [`SimClock::t`] by one fixed step and recompute
/// [`SimClock::activity`] from it.
///
/// Reading [`Time<Fixed>`] keeps the clock stepped exactly once per simulation
/// tick (64 Hz), independent of frame rate, and pauses with the simulation.
fn advance_sim_clock(time: Res<Time<Fixed>>, mut clock: ResMut<SimClock>) {
    clock.t += time.delta_secs();
    clock.activity = activity_at(clock.t);
}

/// Shape of a static obstacle in the play area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObstacleShape {
    /// Solid disc at `center` with `radius`.
    Circle { center: Vec2, radius: f32 },
    /// Solid axis-aligned box at `center` with `half_extents`.
    Aabb { center: Vec2, half_extents: Vec2 },
}

/// One static obstacle: movement cannot enter it and sensing cannot see
/// through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    /// Geometry of the obstacle.
    pub shape: ObstacleShape,
}

impl Obstacle {
    /// A solid disc obstacle.
    pub const fn circle(center: Vec2, radius: f32) -> Self {
        Self {
            shape: ObstacleShape::Circle { center, radius },
        }
    }

    /// A solid axis-aligned box obstacle.
    pub const fn aabb(center: Vec2, half_extents: Vec2) -> Self {
        Self {
            shape: ObstacleShape::Aabb {
                center,
                half_extents,
            },
        }
    }

    /// Centre of the shape.
    pub fn center(&self) -> Vec2 {
        match self.shape {
            ObstacleShape::Circle { center, .. } | ObstacleShape::Aabb { center, .. } => center,
        }
    }

    /// Radius of the smallest circle around the shape that contains it.
    ///
    /// Used as the broad-phase bound for collision and occlusion queries.
    pub fn bounding_radius(&self) -> f32 {
        match self.shape {
            ObstacleShape::Circle { radius, .. } => radius,
            ObstacleShape::Aabb { half_extents, .. } => half_extents.length(),
        }
    }

    /// Resolve a disc of `radius` centred at `pos` out of the shape.
    ///
    /// Returns the outward unit normal and the penetration depth when the disc
    /// overlaps the shape, `None` otherwise. The caller pushes the disc out by
    /// `depth` along the normal; the normal is exact for circles and for discs
    /// outside an AABB (closest-point direction) and the nearest-face axis for
    /// a disc whose centre is inside an AABB.
    pub fn penetration(&self, pos: Vec2, radius: f32) -> Option<(Vec2, f32)> {
        match self.shape {
            ObstacleShape::Circle {
                center,
                radius: obstacle_radius,
            } => {
                let delta = pos - center;
                let min_distance = obstacle_radius + radius;
                let distance_sq = delta.length_squared();

                if distance_sq >= min_distance * min_distance {
                    return None;
                }

                let distance = distance_sq.sqrt();
                // A disc exactly on the centre has no defined normal; pick a
                // fixed axis so the response stays deterministic.
                let normal = if distance > f32::EPSILON {
                    delta / distance
                } else {
                    Vec2::X
                };

                Some((normal, min_distance - distance))
            }
            ObstacleShape::Aabb {
                center,
                half_extents,
            } => {
                let local = pos - center;
                let closest = local.clamp(-half_extents, half_extents);
                let delta = local - closest;
                let distance_sq = delta.length_squared();

                if distance_sq >= radius * radius {
                    return None;
                }

                if distance_sq > f32::EPSILON {
                    // Centre outside the box: push along the closest point.
                    let distance = distance_sq.sqrt();
                    let normal = delta / distance;
                    Some((normal, radius - distance))
                } else {
                    // Centre inside the box: leave through the nearest face.
                    let x_depth = half_extents.x - local.x.abs();
                    let y_depth = half_extents.y - local.y.abs();

                    if x_depth <= y_depth {
                        let sign = if local.x >= 0.0 { 1.0 } else { -1.0 };
                        Some((Vec2::new(sign, 0.0), x_depth + radius))
                    } else {
                        let sign = if local.y >= 0.0 { 1.0 } else { -1.0 };
                        Some((Vec2::new(0.0, sign), y_depth + radius))
                    }
                }
            }
        }
    }

    /// Whether the segment `from -> to` crosses the shape.
    ///
    /// Exact for both shapes: distance-to-segment for circles, a slab test for
    /// AABBs. A segment that only touches the surface counts as blocked.
    pub fn blocks_segment(&self, from: Vec2, to: Vec2) -> bool {
        match self.shape {
            ObstacleShape::Circle { center, radius } => {
                segment_distance_squared(from, to, center) <= radius * radius
            }
            ObstacleShape::Aabb {
                center,
                half_extents,
            } => segment_intersects_aabb(from - center, to - center, half_extents),
        }
    }
}

/// Squared distance from `point` to the segment `from -> to`.
fn segment_distance_squared(from: Vec2, to: Vec2, point: Vec2) -> f32 {
    let segment = to - from;
    let length_sq = segment.length_squared();

    if length_sq <= f32::EPSILON {
        return (point - from).length_squared();
    }

    let t = ((point - from).dot(segment) / length_sq).clamp(0.0, 1.0);
    (from + segment * t - point).length_squared()
}

/// Slab test for the segment `from -> to` against a box centred on the origin.
fn segment_intersects_aabb(from: Vec2, to: Vec2, half_extents: Vec2) -> bool {
    let direction = to - from;
    let mut t_min = 0.0_f32;
    let mut t_max = 1.0_f32;

    for (origin, direction, half) in [
        (from.x, direction.x, half_extents.x),
        (from.y, direction.y, half_extents.y),
    ] {
        if direction.abs() <= f32::EPSILON {
            // Parallel to this pair of slabs: either always inside or outside.
            if origin.abs() > half {
                return false;
            }
            continue;
        }

        let inverse = 1.0 / direction;
        let mut near = (-half - origin) * inverse;
        let mut far = (half - origin) * inverse;

        if near > far {
            std::mem::swap(&mut near, &mut far);
        }

        t_min = t_min.max(near);
        t_max = t_max.min(far);

        if t_min > t_max {
            return false;
        }
    }

    true
}

/// The static obstacles of the play area.
///
/// The resource is created once at register time and never mutated by the
/// simulation, so the parallel movement pass can share it read-only. Obstacles
/// must not overlap each other and must stay at least [`crate::constants::ant::ANT_SIZE`]
/// inside the play area (the collision pass resolves one obstacle at a time in
/// list order).
#[derive(Resource)]
pub struct Obstacles {
    items: Vec<Obstacle>,
}

impl Obstacles {
    /// An arena with no obstacles.
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    /// Obstacles from an explicit list.
    pub fn new(items: Vec<Obstacle>) -> Self {
        Self { items }
    }

    /// Whether there is nothing to collide with or see through.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of obstacles.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Iterate the obstacles in their stable list order.
    pub fn iter(&self) -> impl Iterator<Item = &Obstacle> + '_ {
        self.items.iter()
    }

    /// Whether any obstacle's bounding circle comes within `range` of `pos`.
    ///
    /// Broad phase: a cheap early out so systems only run the exact
    /// segment/collision tests near an obstacle.
    pub fn any_within(&self, pos: Vec2, range: f32) -> bool {
        self.items.iter().any(|obstacle| {
            let reach = range + obstacle.bounding_radius();
            (obstacle.center() - pos).length_squared() <= reach * reach
        })
    }

    /// Whether any obstacle blocks the segment `from -> to`.
    pub fn blocks_segment(&self, from: Vec2, to: Vec2) -> bool {
        self.items
            .iter()
            .any(|obstacle| obstacle.blocks_segment(from, to))
    }
}

impl Default for Obstacles {
    fn default() -> Self {
        Self {
            items: DEFAULT_OBSTACLES.to_vec(),
        }
    }
}

/// Default obstacle set of the game map (F16).
///
/// All three sit in the north or south band of the arena, clear of the
/// nest-to-food corridor (`y` within ±24 in the default map and the trail
/// regression), so the straight-trail measurements are untouched while the
/// rest of the map has visible terrain to walk around. Harnesses that need a
/// clean arena insert [`Obstacles::empty`] instead.
pub const DEFAULT_OBSTACLES: [Obstacle; 3] = [
    // Round boulder in the north-west quadrant.
    Obstacle::circle(Vec2::new(-120.0, 170.0), 34.0),
    // Low ridge in the south-west quadrant.
    Obstacle::aabb(Vec2::new(40.0, -180.0), Vec2::new(70.0, 12.0)),
    // Pillar in the north-east, below the wall.
    Obstacle::aabb(Vec2::new(240.0, 200.0), Vec2::new(14.0, 70.0)),
];

/// Colour of obstacle sprites.
const OBSTACLE_COLOR: Color = Color::srgb(0.42, 0.34, 0.26);

/// Spawn one visible entity per obstacle in [`Obstacles`].
///
/// AABBs are plain `Sprite` rectangles and need no assets, so the system runs
/// unchanged in headless harnesses. Circles become `Mesh2d` discs when the
/// render asset stores exist (the real app) and fall back to a square `Sprite`
/// otherwise (tests), keeping the data and the rendering independent.
fn spawn_obstacle_visuals(
    mut commands: Commands,
    obstacles: Res<Obstacles>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<ColorMaterial>>>,
) {
    for obstacle in obstacles.iter() {
        match obstacle.shape {
            ObstacleShape::Aabb {
                center,
                half_extents,
            } => {
                commands.spawn((
                    Sprite {
                        color: OBSTACLE_COLOR,
                        custom_size: Some(half_extents * 2.0),
                        ..default()
                    },
                    Transform::from_xyz(center.x, center.y, Z_OBSTACLE),
                ));
            }
            ObstacleShape::Circle { center, radius } => {
                match (meshes.as_mut(), materials.as_mut()) {
                    (Some(meshes), Some(materials)) => {
                        commands.spawn((
                            Mesh2d(meshes.add(Circle::new(radius))),
                            MeshMaterial2d(
                                materials.add(ColorMaterial::from_color(OBSTACLE_COLOR)),
                            ),
                            Transform::from_xyz(center.x, center.y, Z_OBSTACLE),
                        ));
                    }
                    _ => {
                        commands.spawn((
                            Sprite {
                                color: OBSTACLE_COLOR,
                                custom_size: Some(Vec2::splat(radius * 2.0)),
                                ..default()
                            },
                            Transform::from_xyz(center.x, center.y, Z_OBSTACLE),
                        ));
                    }
                }
            }
        }
    }
}

/// Wiring hook for the environment stream: registers the clock/activity system
/// in [`SimSet::Clock`] (the first set of the fixed-step chain), the obstacle
/// resource and its startup visuals.
pub fn register(app: &mut App) {
    app.init_resource::<Obstacles>()
        .add_systems(FixedUpdate, advance_sim_clock.in_set(SimSet::Clock))
        .add_systems(
            Startup,
            spawn_obstacle_visuals.in_set(StartupSet::Environment),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    /// Duration of one 64 Hz fixed step.
    const STEP: f32 = 1.0 / 64.0;

    #[test]
    fn activity_is_bounded_by_the_floor_and_the_peak() {
        let samples = 2048;
        let mut min = f32::MAX;
        let mut max = f32::MIN;

        // Two full cycles sampled densely enough to hit both extrema.
        for i in 0..samples {
            let t = 2.0 * DAY_LENGTH * i as f32 / samples as f32;
            let activity = activity_at(t);

            assert!(
                (ACTIVITY_MIN..=1.0).contains(&activity),
                "activity {activity} out of [ACTIVITY_MIN, 1] at t = {t}"
            );

            min = min.min(activity);
            max = max.max(activity);
        }

        assert!(
            (min - ACTIVITY_MIN).abs() < 1e-4,
            "the floor must be reached, got min {min}"
        );
        assert!(
            (max - 1.0).abs() < 1e-4,
            "the peak must be reached, got max {max}"
        );
    }

    #[test]
    fn activity_is_periodic_and_a_full_cycle_returns_to_the_start() {
        assert!(
            (activity_at(DAY_LENGTH) - activity_at(0.0)).abs() < 1e-6,
            "one full cycle must return to the start value"
        );

        for i in 0..=64 {
            let t = DAY_LENGTH * i as f32 / 64.0;
            let next = t + DAY_LENGTH;

            assert!(
                (activity_at(t) - activity_at(next)).abs() < 1e-4,
                "activity at {t} and {next} must match"
            );
        }
    }

    #[test]
    fn clock_advances_in_fixed_steps_and_tracks_activity() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                STEP,
            )))
            .init_resource::<SimClock>();
        register(&mut app);

        let before = app.world().resource::<SimClock>().t;
        for _ in 0..64 {
            app.update();
        }

        let clock = app.world().resource::<SimClock>();
        assert!(clock.t > before, "t must advance every fixed step");
        assert!(
            (clock.t - 1.0).abs() < 0.05,
            "64 steps at 64 Hz should advance t by ~1 s, got {}",
            clock.t
        );
        assert!(
            (clock.activity - activity_at(clock.t)).abs() < 1e-6,
            "activity must be recomputed from t, got {} for t {}",
            clock.activity,
            clock.t
        );
    }

    #[test]
    fn circle_penetration_is_exact_and_clears_at_the_surface() {
        let obstacle = Obstacle::circle(Vec2::ZERO, 10.0);

        assert_eq!(obstacle.penetration(Vec2::new(12.0, 0.0), 1.0), None);
        assert_eq!(obstacle.penetration(Vec2::new(11.0, 0.0), 1.0), None);

        let (normal, depth) = obstacle
            .penetration(Vec2::new(9.0, 0.0), 1.0)
            .expect("overlap");
        assert!((normal - Vec2::X).length() < 1e-6);
        assert!((depth - 2.0).abs() < 1e-6);

        // A point on the centre still resolves deterministically.
        let (normal, depth) = obstacle.penetration(Vec2::ZERO, 1.0).expect("overlap");
        assert_eq!(normal, Vec2::X);
        assert!((depth - 11.0).abs() < 1e-6);
    }

    #[test]
    fn aabb_penetration_handles_faces_corners_and_inside() {
        let obstacle = Obstacle::aabb(Vec2::ZERO, Vec2::new(10.0, 4.0));

        // Face: pushed straight out along the shallow axis.
        let (normal, depth) = obstacle
            .penetration(Vec2::new(10.5, 0.0), 1.0)
            .expect("face overlap");
        assert!((normal - Vec2::X).length() < 1e-6);
        assert!((depth - 0.5).abs() < 1e-6);

        // Corner: pushed along the closest-point diagonal.
        let (normal, depth) = obstacle
            .penetration(Vec2::new(10.5, 4.5), 1.0)
            .expect("corner overlap");
        let expected = Vec2::new(0.5, 0.5).normalize();
        assert!((normal - expected).length() < 1e-6);
        assert!((depth - (1.0 - 0.5 * 2.0_f32.sqrt())).abs() < 1e-6);

        // Inside: the nearest face wins.
        let (normal, depth) = obstacle
            .penetration(Vec2::new(0.0, 3.0), 1.0)
            .expect("inside overlap");
        assert!((normal - Vec2::Y).length() < 1e-6);
        assert!((depth - 2.0).abs() < 1e-6);

        // Outside the expanded box: no collision.
        assert_eq!(obstacle.penetration(Vec2::new(11.0, 0.0), 1.0), None);
        assert_eq!(obstacle.penetration(Vec2::new(0.0, 5.0), 1.0), None);
    }

    #[test]
    fn segment_blocking_is_exact_for_both_shapes() {
        let circle = Obstacle::circle(Vec2::new(10.0, 0.0), 2.0);
        assert!(circle.blocks_segment(Vec2::ZERO, Vec2::new(20.0, 0.0)));
        assert!(circle.blocks_segment(Vec2::ZERO, Vec2::new(8.5, 0.0)));
        assert!(!circle.blocks_segment(Vec2::ZERO, Vec2::new(7.5, 0.0)));
        assert!(!circle.blocks_segment(Vec2::ZERO, Vec2::new(0.0, 20.0)));

        let aabb = Obstacle::aabb(Vec2::new(10.0, 0.0), Vec2::new(2.0, 5.0));
        assert!(aabb.blocks_segment(Vec2::ZERO, Vec2::new(20.0, 0.0)));
        assert!(aabb.blocks_segment(Vec2::new(10.0, -20.0), Vec2::new(10.0, 20.0)));
        // Passes in front of the box.
        assert!(!aabb.blocks_segment(Vec2::ZERO, Vec2::new(7.0, 0.0)));
        // Passes beside the box.
        assert!(!aabb.blocks_segment(Vec2::new(0.0, 6.0), Vec2::new(20.0, 6.0)));
        // Diagonal through the corner region is blocked; just past it is not.
        assert!(aabb.blocks_segment(Vec2::new(6.0, -6.0), Vec2::new(14.0, 6.0)));
        assert!(!aabb.blocks_segment(Vec2::new(0.0, -6.0), Vec2::new(0.0, 6.0)));
    }

    #[test]
    fn obstacles_default_to_the_map_set_and_can_be_emptied() {
        let default = Obstacles::default();
        assert_eq!(default.len(), DEFAULT_OBSTACLES.len());
        assert!(!default.is_empty());

        let empty = Obstacles::empty();
        assert!(empty.is_empty());
        assert!(!empty.blocks_segment(Vec2::ZERO, Vec2::new(300.0, 0.0)));
    }

    #[test]
    fn any_within_is_a_conservative_broad_phase() {
        let obstacles = Obstacles::new(vec![Obstacle::circle(Vec2::new(50.0, 0.0), 10.0)]);

        assert!(obstacles.any_within(Vec2::new(20.0, 0.0), 25.0));
        assert!(obstacles.any_within(Vec2::new(50.0, 0.0), 0.0));
        assert!(!obstacles.any_within(Vec2::new(-100.0, 0.0), 25.0));

        // The broad phase never rejects a segment the exact test would block.
        for x in -100..=100 {
            let from = Vec2::new(x as f32, 0.0);
            let to = Vec2::new(x as f32 + 5.0, 0.0);
            if obstacles.blocks_segment(from, to) {
                assert!(obstacles.any_within(from, 5.0));
            }
        }
    }
}
