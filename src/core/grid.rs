//! Pure grid geometry for the simulation play area.
//!
//! Every conversion between world space and grid space goes through this
//! module so the layout only has to be reasoned about in one place.

use crate::constants::world::{
    GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH,
};
use bevy::prelude::*;

/// Convert a world-space position to the grid cell that contains it.
///
/// Preserves the original simulation semantics: the division result is cast to
/// `i32` (truncating toward zero) before the bounds check, and positions
/// outside the play area map to `None`.
pub fn world_to_grid(world: Vec2) -> Option<UVec2> {
    let x = ((world.x + PLAY_AREA_WIDTH / 2.0) / GRID_SIZE) as i32;
    let y = ((world.y + PLAY_AREA_HEIGHT / 2.0) / GRID_SIZE) as i32;

    if x >= 0 && x < GRID_WIDTH as i32 && y >= 0 && y < GRID_HEIGHT as i32 {
        Some(UVec2::new(x as u32, y as u32))
    } else {
        None
    }
}

/// Center of `cell` in world space.
pub fn grid_to_world(cell: UVec2) -> Vec2 {
    Vec2::new(
        cell.x as f32 * GRID_SIZE - PLAY_AREA_WIDTH / 2.0 + GRID_SIZE / 2.0,
        cell.y as f32 * GRID_SIZE - PLAY_AREA_HEIGHT / 2.0 + GRID_SIZE / 2.0,
    )
}

/// Whether `cell` lies inside the grid.
pub fn in_bounds(cell: UVec2) -> bool {
    cell.x < GRID_WIDTH as u32 && cell.y < GRID_HEIGHT as u32
}

/// All in-bounds cells within Chebyshev distance `radius` of `center`,
/// including `center` itself.
pub fn neighborhood(center: UVec2, radius: i32) -> impl Iterator<Item = UVec2> + Clone {
    (-radius..=radius).flat_map(move |dy| {
        (-radius..=radius).filter_map(move |dx| {
            let x = center.x as i64 + dx as i64;
            let y = center.y as i64 + dy as i64;
            if x < 0 || y < 0 {
                return None;
            }
            let cell = UVec2::new(x as u32, y as u32);
            in_bounds(cell).then_some(cell)
        })
    })
}

/// Pack a cell into a single flat index (`y * GRID_WIDTH + x`).
pub fn pack(cell: UVec2) -> u32 {
    cell.y * GRID_WIDTH as u32 + cell.x
}

/// Inverse of [`pack`].
pub fn unpack(key: u32) -> UVec2 {
    UVec2::new(key % GRID_WIDTH as u32, key / GRID_WIDTH as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_edges() {
        assert!(in_bounds(UVec2::ZERO));
        assert!(in_bounds(UVec2::new(
            GRID_WIDTH as u32 - 1,
            GRID_HEIGHT as u32 - 1
        )));
        assert!(!in_bounds(UVec2::new(GRID_WIDTH as u32, 0)));
        assert!(!in_bounds(UVec2::new(0, GRID_HEIGHT as u32)));
        assert!(!in_bounds(UVec2::new(u32::MAX, u32::MAX)));
    }

    #[test]
    fn cell_center_mapping() {
        let origin = grid_to_world(UVec2::ZERO);
        assert_eq!(origin.x, -PLAY_AREA_WIDTH / 2.0 + GRID_SIZE / 2.0);
        assert_eq!(origin.y, -PLAY_AREA_HEIGHT / 2.0 + GRID_SIZE / 2.0);
        assert_eq!(world_to_grid(origin), Some(UVec2::ZERO));

        let last = UVec2::new(GRID_WIDTH as u32 - 1, GRID_HEIGHT as u32 - 1);
        assert_eq!(world_to_grid(grid_to_world(last)), Some(last));
    }

    #[test]
    fn world_to_grid_edges() {
        assert_eq!(
            world_to_grid(Vec2::new(-PLAY_AREA_WIDTH / 2.0, -PLAY_AREA_HEIGHT / 2.0)),
            Some(UVec2::ZERO)
        );

        // The right/top edge already lies one cell past the last cell.
        assert_eq!(world_to_grid(Vec2::new(PLAY_AREA_WIDTH / 2.0, 0.0)), None);
        assert_eq!(world_to_grid(Vec2::new(0.0, PLAY_AREA_HEIGHT / 2.0)), None);
        assert_eq!(world_to_grid(Vec2::new(PLAY_AREA_WIDTH, 0.0)), None);
        assert_eq!(world_to_grid(Vec2::new(0.0, -PLAY_AREA_HEIGHT)), None);

        // Values slightly past the edge truncate toward zero into cell 0,
        // matching the original cast semantics.
        assert_eq!(
            world_to_grid(Vec2::new(-PLAY_AREA_WIDTH / 2.0 - 0.5, 0.0)),
            Some(UVec2::new(0, (PLAY_AREA_HEIGHT / 2.0 / GRID_SIZE) as u32))
        );
    }

    #[test]
    fn pack_unpack_roundtrip() {
        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                let cell = UVec2::new(x, y);
                assert_eq!(unpack(pack(cell)), cell);
            }
        }
        assert_eq!(pack(UVec2::new(3, 2)), 2 * GRID_WIDTH as u32 + 3);
    }

    #[test]
    fn neighborhood_clips_to_bounds() {
        let corner: Vec<UVec2> = neighborhood(UVec2::ZERO, 1).collect();
        assert_eq!(corner.len(), 4);
        for cell in [
            UVec2::ZERO,
            UVec2::new(1, 0),
            UVec2::new(0, 1),
            UVec2::new(1, 1),
        ] {
            assert!(corner.contains(&cell));
        }

        let interior: Vec<UVec2> = neighborhood(UVec2::new(5, 5), 1).collect();
        assert_eq!(interior.len(), 9);

        let far = UVec2::new(GRID_WIDTH as u32 - 1, GRID_HEIGHT as u32 - 1);
        let far_cells: Vec<UVec2> = neighborhood(far, 1).collect();
        assert_eq!(far_cells.len(), 4);
        assert!(!far_cells.contains(&UVec2::new(GRID_WIDTH as u32, GRID_HEIGHT as u32)));
    }
}
