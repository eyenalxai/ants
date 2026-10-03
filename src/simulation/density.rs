//! Coarse ant-density grid for crowd avoidance and deposit suppression, plus
//! the fine contact grid used for physical separation and lane formation.

use bevy::prelude::*;

use crate::constants::ant::{
    CONTACT_CELL_SIZE, CONTACT_CORRECTION_SPLIT, CONTACT_HEAD_ON_DOT, CONTACT_LADEN_YIELD,
    CONTACT_MIN_DISTANCE, CONTACT_NEIGHBORS_MAX, CONTACT_OUTBOUND_YIELD, CONTACT_RADIUS,
    CONTACT_SAME_KIND_YIELD, CONTACT_SIDE_TURN_FACTOR, CONTACT_TURN_FACTOR,
};
use crate::constants::world::{
    DENSITY_CELL_SIZE, DENSITY_CROWDED_THRESHOLD, DENSITY_GRID_HEIGHT, DENSITY_GRID_WIDTH,
    DENSITY_SLOWDOWN_FACTOR, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH,
};
use crate::core::grid::{index_to_cell, world_to_index};
use crate::simulation::ant::Ant;

/// Fine contact-grid width in cells ([`CONTACT_CELL_SIZE`] divides the play
/// area exactly).
const CONTACT_GRID_WIDTH: usize = (PLAY_AREA_WIDTH / CONTACT_CELL_SIZE) as usize;
/// Fine contact-grid height in cells.
const CONTACT_GRID_HEIGHT: usize = (PLAY_AREA_HEIGHT / CONTACT_CELL_SIZE) as usize;

/// One recorded occupant of a fine contact cell. The first ant recorded in a
/// cell wins, so the grid needs no per-cell allocation and stays deterministic.
#[derive(Clone, Copy)]
struct ContactOccupant {
    pos: Vec2,
    direction: f32,
    /// Entity index of the occupant; `u32::MAX` marks an empty cell.
    index: u32,
    laden: bool,
}

/// Empty-cell sentinel for the contact grid.
const EMPTY_CONTACT: ContactOccupant = ContactOccupant {
    pos: Vec2::ZERO,
    direction: 0.0,
    index: u32::MAX,
    laden: false,
};

/// Flat per-cell ant counts, rebuilt once per fixed tick before movement.
///
/// `cells` is the coarse density grid; `contact` is the fine contact grid
/// (one occupant per 2 u cell) read by [`Self::contact_response`].
#[derive(Resource)]
pub struct AntDensity {
    cells: Box<[u32]>,
    contact: Box<[ContactOccupant]>,
}

impl Default for AntDensity {
    fn default() -> Self {
        Self::new()
    }
}

impl AntDensity {
    pub fn new() -> Self {
        Self {
            cells: vec![0; DENSITY_GRID_WIDTH * DENSITY_GRID_HEIGHT].into_boxed_slice(),
            contact: vec![EMPTY_CONTACT; CONTACT_GRID_WIDTH * CONTACT_GRID_HEIGHT]
                .into_boxed_slice(),
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(0);
        self.clear_contacts();
    }

    /// Clear only the fine contact grid; the movement pass rebuilds it from
    /// post-move positions after steering.
    pub fn clear_contacts(&mut self) {
        self.contact.fill(EMPTY_CONTACT);
    }

    /// Record one ant at `world`.
    pub fn add(&mut self, world: Vec2) {
        if let Some(index) = density_index(world) {
            self.cells[index] += 1;
        }
    }

    /// Record one ant in the fine contact grid. Only the first ant mapped to a
    /// cell is kept; later ants are ignored, which bounds memory and keeps the
    /// pass deterministic.
    pub fn add_contact(&mut self, index: u32, world: Vec2, direction: f32, laden: bool) {
        if let Some(cell) = contact_index(world) {
            let slot = &mut self.contact[cell];

            if slot.index == u32::MAX {
                *slot = ContactOccupant {
                    pos: world,
                    direction,
                    index,
                    laden,
                };
            }
        }
    }

    /// Ants recorded in the cell containing `world` (0 outside the play area).
    pub fn sample(&self, world: Vec2) -> u32 {
        density_index(world).map_or(0, |index| self.cells[index])
    }

    /// Contact response for the ant with entity index `self_index`: a
    /// positional correction and a lateral-turn weight, or `None` when no
    /// recorded neighbour is within [`CONTACT_RADIUS`].
    pub fn contact_response(
        &self,
        self_index: u32,
        pos: Vec2,
        direction: f32,
        laden: bool,
    ) -> Option<(Vec2, f32)> {
        let other = self.nearest_contact(self_index, pos)?;

        contact_response(
            pos,
            direction,
            laden,
            other.pos,
            other.direction,
            other.laden,
        )
    }

    /// Nearest recorded neighbour within the 3x3 cell neighbourhood. At most
    /// [`CONTACT_NEIGHBORS_MAX`] occupied cells are inspected, which bounds the
    /// per-ant cost independently of local density.
    fn nearest_contact(&self, self_index: u32, pos: Vec2) -> Option<ContactOccupant> {
        let center = contact_index(pos)?;
        let cell = index_to_cell(center, CONTACT_GRID_WIDTH);
        let cx = cell.x as i32;
        let cy = cell.y as i32;
        let mut inspected = 0usize;
        let mut best: Option<(ContactOccupant, f32)> = None;

        'cells: for dy in -1..=1 {
            let y = cy + dy;

            if y < 0 || y >= CONTACT_GRID_HEIGHT as i32 {
                continue;
            }

            for dx in -1..=1 {
                let x = cx + dx;

                if x < 0 || x >= CONTACT_GRID_WIDTH as i32 {
                    continue;
                }

                let occupant = self.contact[y as usize * CONTACT_GRID_WIDTH + x as usize];

                if occupant.index == u32::MAX || occupant.index == self_index {
                    continue;
                }

                inspected += 1;
                let d2 = occupant.pos.distance_squared(pos);

                if best.is_none_or(|(_, best_d2)| d2 < best_d2) {
                    best = Some((occupant, d2));
                }

                if inspected >= CONTACT_NEIGHBORS_MAX {
                    break 'cells;
                }
            }
        }

        best.filter(|(_, d2)| *d2 <= CONTACT_RADIUS * CONTACT_RADIUS)
            .map(|(occupant, _)| occupant)
    }
}

/// Flat index of the density cell containing `world`, if inside the play area.
pub fn density_index(world: Vec2) -> Option<usize> {
    world_to_index(
        world,
        DENSITY_CELL_SIZE,
        DENSITY_GRID_WIDTH,
        DENSITY_GRID_HEIGHT,
    )
}

/// Flat index of the fine contact cell containing `world`, if inside the play
/// area.
pub fn contact_index(world: Vec2) -> Option<usize> {
    world_to_index(
        world,
        CONTACT_CELL_SIZE,
        CONTACT_GRID_WIDTH,
        CONTACT_GRID_HEIGHT,
    )
}

/// Pure contact rule between two ants. Returns `(positional correction,
/// lateral-turn weight)` when they are within [`CONTACT_RADIUS`].
///
/// The correction splits the overlap. The turn is keep-right on head-on
/// encounters (the Couzin & Franks lane rule: each stream passes on its own
/// right) and away from the neighbour's side otherwise. The carrying state
/// sets the yield magnitude asymmetrically — outbound ants yield harder to
/// laden ants — which biases which side each stream settles on.
pub fn contact_response(
    self_pos: Vec2,
    self_direction: f32,
    self_laden: bool,
    other_pos: Vec2,
    other_direction: f32,
    other_laden: bool,
) -> Option<(Vec2, f32)> {
    let offset = other_pos - self_pos;
    let distance = offset.length();

    if distance > CONTACT_RADIUS {
        return None;
    }

    let (sin, cos) = self_direction.sin_cos();
    let forward = Vec2::new(cos, sin);
    let left = Vec2::new(-sin, cos);
    // Keep-right fallback for coincident ants: a zero offset has no direction,
    // so pick the ant's right deterministically instead of a noisy unit vector.
    let right = Vec2::new(sin, -cos);

    let away = if distance > CONTACT_MIN_DISTANCE {
        -offset / distance
    } else {
        right
    };
    let overlap = CONTACT_RADIUS - distance;
    let correction = away * (overlap * CONTACT_CORRECTION_SPLIT);

    let other_forward = Vec2::new(other_direction.cos(), other_direction.sin());
    let head_on = forward.dot(other_forward) < CONTACT_HEAD_ON_DOT;
    let side = offset.dot(left);

    let turn = if head_on {
        // Both streams keep right; the yield strength is asymmetric between
        // laden and outbound ants, which biases lane assignment.
        let yield_weight = match (self_laden, other_laden) {
            (true, false) => CONTACT_LADEN_YIELD,
            (false, true) => CONTACT_OUTBOUND_YIELD,
            _ => CONTACT_SAME_KIND_YIELD,
        };
        -CONTACT_TURN_FACTOR * yield_weight
    } else if side > CONTACT_MIN_DISTANCE {
        -CONTACT_TURN_FACTOR * CONTACT_SIDE_TURN_FACTOR
    } else if side < -CONTACT_MIN_DISTANCE {
        CONTACT_TURN_FACTOR * CONTACT_SIDE_TURN_FACTOR
    } else {
        0.0
    };

    Some((correction, turn))
}

/// Rebuild the coarse density grid from all live ants in one sequential pass.
///
/// The fine contact grid is rebuilt inside the movement pass from post-move
/// positions; this pass only feeds crowd avoidance and deposit suppression.
pub fn rebuild_ant_density(
    mut density: ResMut<AntDensity>,
    ant_query: Query<&Transform, With<Ant>>,
) {
    density.clear();

    for transform in &ant_query {
        density.add(Vec2::new(transform.translation.x, transform.translation.y));
    }
}

/// Pure crowd rule: returns `(speed_factor, turn_sign)`.
///
/// `turn_sign` is `+1` to turn left (away from a denser right side) and `-1`
/// to turn right; `0` keeps the current heading.
pub fn crowd_response(ahead: u32, left: u32, right: u32) -> (f32, f32) {
    if ahead <= DENSITY_CROWDED_THRESHOLD {
        return (1.0, 0.0);
    }

    let turn_sign = if left > right {
        -1.0
    } else if right > left {
        1.0
    } else {
        0.0
    };

    (DENSITY_SLOWDOWN_FACTOR, turn_sign)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ant::ANT_TURN_RATE;
    use crate::constants::world::{PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};
    use crate::simulation::movement::steering::shortest_angle_diff;
    use std::f32::consts::{PI, TAU};

    const EPS: f32 = 1e-4;

    #[test]
    fn density_index_covers_play_area() {
        let bottom_left = Vec2::new(-PLAY_AREA_WIDTH / 2.0 + 0.1, -PLAY_AREA_HEIGHT / 2.0 + 0.1);
        assert_eq!(density_index(bottom_left), Some(0));

        let top_right = Vec2::new(PLAY_AREA_WIDTH / 2.0 - 0.1, PLAY_AREA_HEIGHT / 2.0 - 0.1);
        assert_eq!(
            density_index(top_right),
            Some(DENSITY_GRID_WIDTH * DENSITY_GRID_HEIGHT - 1)
        );

        assert_eq!(density_index(Vec2::new(PLAY_AREA_WIDTH, 0.0)), None);
        assert_eq!(density_index(Vec2::new(0.0, -PLAY_AREA_HEIGHT)), None);
    }

    #[test]
    fn crowd_response_slows_and_turns_away_from_denser_side() {
        assert_eq!(crowd_response(DENSITY_CROWDED_THRESHOLD, 9, 0), (1.0, 0.0));
        assert_eq!(
            crowd_response(DENSITY_CROWDED_THRESHOLD + 1, 9, 0),
            (DENSITY_SLOWDOWN_FACTOR, -1.0)
        );
        assert_eq!(
            crowd_response(DENSITY_CROWDED_THRESHOLD + 1, 0, 9),
            (DENSITY_SLOWDOWN_FACTOR, 1.0)
        );
        assert_eq!(
            crowd_response(DENSITY_CROWDED_THRESHOLD + 1, 3, 3),
            (DENSITY_SLOWDOWN_FACTOR, 0.0)
        );
    }

    #[test]
    fn contact_index_covers_play_area() {
        let bottom_left = Vec2::new(-PLAY_AREA_WIDTH / 2.0 + 0.1, -PLAY_AREA_HEIGHT / 2.0 + 0.1);
        assert_eq!(contact_index(bottom_left), Some(0));

        let top_right = Vec2::new(PLAY_AREA_WIDTH / 2.0 - 0.1, PLAY_AREA_HEIGHT / 2.0 - 0.1);
        assert_eq!(
            contact_index(top_right),
            Some(CONTACT_GRID_WIDTH * CONTACT_GRID_HEIGHT - 1)
        );

        assert_eq!(contact_index(Vec2::new(PLAY_AREA_WIDTH, 0.0)), None);
    }

    #[test]
    fn nearest_contact_skips_self_and_finds_the_closest_neighbour() {
        let mut density = AntDensity::new();
        density.add_contact(1, Vec2::new(10.0, 0.0), PI, true);
        density.add_contact(2, Vec2::new(12.5, 0.0), 0.0, false);

        let nearest = density
            .nearest_contact(1, Vec2::new(10.0, 0.0))
            .expect("ant 2 is within contact radius of ant 1");
        assert_eq!(nearest.index, 2, "the ant's own record must be skipped");

        let nearest = density
            .nearest_contact(3, Vec2::new(10.5, 0.0))
            .expect("ant 1 is closer than ant 2");
        assert_eq!(nearest.index, 1);
        assert!(nearest.laden);
    }

    #[test]
    fn contact_response_splits_overlap_and_keeps_right_on_head_on() {
        let self_pos = Vec2::ZERO;
        // Other ant almost coincident, slightly to the south (right of east).
        let other_pos = Vec2::new(0.0, -0.5);

        let (correction, turn) =
            contact_response(self_pos, 0.0, false, other_pos, PI, true).expect("in contact");

        assert!(correction.y > 0.0, "push away from the other ant");
        assert!(correction.x.abs() < EPS);
        assert!(
            (correction.length() - (CONTACT_RADIUS - 0.5) * CONTACT_CORRECTION_SPLIT).abs() < EPS,
            "the correction splits the overlap, got {}",
            correction.length()
        );
        assert!(turn < 0.0, "a head-on encounter keeps right");

        let (_, laden_turn) =
            contact_response(self_pos, 0.0, true, other_pos, PI, false).expect("in contact");
        assert!(
            laden_turn.abs() < turn.abs(),
            "laden ants are less manoeuvrable and must yield less"
        );
    }

    #[test]
    fn contact_response_turns_away_from_a_side_neighbour() {
        // Heading east, the neighbour is to the north = the ant's left.
        let (_, turn) = contact_response(Vec2::ZERO, 0.0, false, Vec2::new(0.0, 0.5), 0.0, false)
            .expect("in contact");
        assert!(turn < 0.0, "turn right, away from a left neighbour");

        let (_, turn) = contact_response(Vec2::ZERO, 0.0, false, Vec2::new(0.0, -0.5), 0.0, false)
            .expect("in contact");
        assert!(turn > 0.0, "turn left, away from a right neighbour");
    }

    #[test]
    fn contact_response_ignores_distant_ants() {
        assert!(
            contact_response(
                Vec2::ZERO,
                0.0,
                false,
                Vec2::new(CONTACT_RADIUS + 0.1, 0.0),
                PI,
                true,
            )
            .is_none()
        );
    }

    /// Two head-on ants must stay apart instead of passing through each other.
    ///
    /// Mirrors the two-phase order in `move_ants`: move first, then resolve
    /// contact from the post-move positions.
    #[test]
    fn head_on_ants_separate_instead_of_overlapping() {
        #[derive(Clone, Copy)]
        struct Walker {
            pos: Vec2,
            dir: f32,
            laden: bool,
        }

        let mut a = Walker {
            pos: Vec2::new(-1.75, 0.0),
            dir: 0.0,
            laden: false,
        };
        let mut b = Walker {
            pos: Vec2::new(1.75, 0.0),
            dir: PI,
            laden: true,
        };
        let dt = 1.0 / 64.0;
        let speed = 50.0;
        let mut min_distance = f32::MAX;

        for _ in 0..128 {
            for walker in [&mut a, &mut b] {
                let (sin, cos) = walker.dir.sin_cos();
                walker.pos += Vec2::new(cos, sin) * speed * dt;
            }

            let response_a = contact_response(a.pos, a.dir, a.laden, b.pos, b.dir, b.laden);
            let response_b = contact_response(b.pos, b.dir, b.laden, a.pos, a.dir, a.laden);

            for (walker, response) in [(&mut a, response_a), (&mut b, response_b)] {
                if let Some((correction, turn)) = response {
                    walker.pos += correction;
                    walker.dir = (walker.dir + turn * ANT_TURN_RATE * dt).rem_euclid(TAU);
                }
            }

            min_distance = min_distance.min(a.pos.distance(b.pos));
        }

        assert!(
            min_distance >= CONTACT_RADIUS * 0.6,
            "ants passed through each other: minimum distance {min_distance}"
        );
        assert!(
            a.pos.distance(b.pos) >= CONTACT_RADIUS * 0.8,
            "ants ended overlapped: distance {}",
            a.pos.distance(b.pos)
        );
    }

    /// A bidirectional stream must self-organize so laden and outbound ants
    /// end up laterally offset from each other (Couzin & Franks lanes).
    #[test]
    fn bidirectional_stream_forms_lanes() {
        #[derive(Clone, Copy)]
        struct StreamAnt {
            pos: Vec2,
            dir: f32,
            laden: bool,
        }

        const N: usize = 40;
        const SPEED: f32 = 50.0;
        const DT: f32 = 1.0 / 64.0;
        const TICKS: usize = 64 * 30;
        const LANE_SEPARATION_MIN: f32 = 3.0;

        fn step(ants: &mut [StreamAnt], dt: f32) {
            for ant in ants.iter_mut() {
                let (sin, cos) = ant.dir.sin_cos();
                ant.pos += Vec2::new(cos, sin) * SPEED * dt;
            }

            // A weak goal pull keeps both streams travelling along the axis,
            // standing in for homing/food attraction in the real simulation.
            for ant in ants.iter_mut() {
                let goal = if ant.laden { PI } else { 0.0 };
                ant.dir = (ant.dir + shortest_angle_diff(ant.dir, goal) * 2.0 * dt).rem_euclid(TAU);
            }

            let mut responses = vec![None; ants.len()];

            for (i, ant) in ants.iter().enumerate() {
                let mut nearest: Option<(usize, f32)> = None;

                for (j, other) in ants.iter().enumerate() {
                    if i == j {
                        continue;
                    }

                    let d2 = ant.pos.distance_squared(other.pos);

                    if nearest.is_none_or(|(_, best)| d2 < best) {
                        nearest = Some((j, d2));
                    }
                }

                if let Some((j, _)) = nearest {
                    responses[i] = contact_response(
                        ant.pos,
                        ant.dir,
                        ant.laden,
                        ants[j].pos,
                        ants[j].dir,
                        ants[j].laden,
                    );
                }
            }

            for (ant, response) in ants.iter_mut().zip(responses) {
                if let Some((correction, turn)) = response {
                    ant.pos += correction;
                    ant.dir = (ant.dir + turn * ANT_TURN_RATE * dt).rem_euclid(TAU);
                }
            }
        }

        let mut rng = fastrand::Rng::with_seed(0x1A4E);
        let mut ants = Vec::with_capacity(2 * N);

        for i in 0..N {
            ants.push(StreamAnt {
                pos: Vec2::new(100.0 + i as f32 * 6.0, (rng.f32() - 0.5) * 4.0),
                dir: PI,
                laden: true,
            });
        }

        for i in 0..N {
            ants.push(StreamAnt {
                pos: Vec2::new(-100.0 - i as f32 * 6.0, (rng.f32() - 0.5) * 4.0),
                dir: 0.0,
                laden: false,
            });
        }

        for _ in 0..TICKS {
            step(&mut ants, DT);
        }

        let laden_y = ants
            .iter()
            .filter(|ant| ant.laden)
            .map(|ant| ant.pos.y)
            .sum::<f32>()
            / N as f32;
        let outbound_y = ants
            .iter()
            .filter(|ant| !ant.laden)
            .map(|ant| ant.pos.y)
            .sum::<f32>()
            / N as f32;

        assert!(
            laden_y - outbound_y > LANE_SEPARATION_MIN,
            "no lane separation after {TICKS} ticks: laden mean y {laden_y:.2}, \
             outbound mean y {outbound_y:.2}"
        );
    }
}
