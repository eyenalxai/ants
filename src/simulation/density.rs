//! Coarse ant-density grid for crowd avoidance and deposit suppression.

use bevy::prelude::*;

use crate::constants::world::{
    DENSITY_CELL_SIZE, DENSITY_CROWDED_THRESHOLD, DENSITY_GRID_HEIGHT, DENSITY_GRID_WIDTH,
    DENSITY_SLOWDOWN_FACTOR, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH,
};
use crate::simulation::ant::Ant;

/// Flat per-cell ant counts, rebuilt once per fixed tick before movement.
#[derive(Resource)]
pub struct AntDensity {
    cells: Box<[u32]>,
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
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(0);
    }

    /// Record one ant at `world`.
    pub fn add(&mut self, world: Vec2) {
        if let Some(index) = density_index(world) {
            self.cells[index] += 1;
        }
    }

    /// Ants recorded in the cell containing `world` (0 outside the play area).
    pub fn sample(&self, world: Vec2) -> u32 {
        density_index(world).map_or(0, |index| self.cells[index])
    }
}

/// Flat index of the density cell containing `world`, if inside the play area.
pub fn density_index(world: Vec2) -> Option<usize> {
    let x = ((world.x + PLAY_AREA_WIDTH / 2.0) / DENSITY_CELL_SIZE) as i32;
    let y = ((world.y + PLAY_AREA_HEIGHT / 2.0) / DENSITY_CELL_SIZE) as i32;

    if x < 0 || y < 0 || x >= DENSITY_GRID_WIDTH as i32 || y >= DENSITY_GRID_HEIGHT as i32 {
        return None;
    }

    Some(y as usize * DENSITY_GRID_WIDTH + x as usize)
}

/// Rebuild the density grid from all live ants in one sequential pass.
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
}
