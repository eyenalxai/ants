//! Pure grid geometry for the simulation play area.
//!
//! Every conversion between world space and grid space goes through this
//! module so the layout only has to be reasoned about in one place.

use crate::constants::world::{GRID_HEIGHT, GRID_SIZE, GRID_WIDTH};
use bevy::prelude::*;

/// Flat row-major index (`y * width + x`) of the cell of size `cell_size`
/// containing `world`, or `None` outside the half-open play area
/// `[-width*cell_size/2, width*cell_size/2) x [-height*cell_size/2, height*cell_size/2)`.
///
/// The mapping uses Euclidean `floor` division, so a point even slightly left
/// of or below the origin maps outside the grid instead of truncating into
/// column/row 0. `width`/`height` are passed explicitly so the same helper
/// serves grids with different resolutions (for example the density grid).
pub fn world_to_index(world: Vec2, cell_size: f32, width: usize, height: usize) -> Option<usize> {
    let x = ((world.x + width as f32 * cell_size / 2.0) / cell_size).floor();
    let y = ((world.y + height as f32 * cell_size / 2.0) / cell_size).floor();

    if x < 0.0 || y < 0.0 {
        return None;
    }

    let x = x as usize;
    let y = y as usize;

    if x >= width || y >= height {
        return None;
    }

    Some(y * width + x)
}

/// Cell at flat row-major `index` in a grid `width` cells wide.
pub fn index_to_cell(index: usize, width: usize) -> UVec2 {
    UVec2::new((index % width) as u32, (index / width) as u32)
}

/// Convert a world-space position to the grid cell that contains it.
///
/// Bounds are half-open: `[-PLAY_AREA_WIDTH/2, PLAY_AREA_WIDTH/2)` on x and
/// `[-PLAY_AREA_HEIGHT/2, PLAY_AREA_HEIGHT/2)` on y. Positions outside the
/// play area map to `None`.
pub fn world_to_grid(world: Vec2) -> Option<UVec2> {
    world_to_index(world, GRID_SIZE, GRID_WIDTH, GRID_HEIGHT)
        .map(|index| index_to_cell(index, GRID_WIDTH))
}

/// Center of `cell` in world space.
pub fn grid_to_world(cell: UVec2) -> Vec2 {
    Vec2::new(
        cell.x as f32 * GRID_SIZE - GRID_WIDTH as f32 * GRID_SIZE / 2.0 + GRID_SIZE / 2.0,
        cell.y as f32 * GRID_SIZE - GRID_HEIGHT as f32 * GRID_SIZE / 2.0 + GRID_SIZE / 2.0,
    )
}

/// Whether `cell` lies inside the grid.
pub fn in_bounds(cell: UVec2) -> bool {
    cell.x < GRID_WIDTH as u32 && cell.y < GRID_HEIGHT as u32
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
    use crate::constants::world::{PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};

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
    fn world_to_grid_edges_are_half_open() {
        assert_eq!(
            world_to_grid(Vec2::new(-PLAY_AREA_WIDTH / 2.0, -PLAY_AREA_HEIGHT / 2.0)),
            Some(UVec2::ZERO)
        );

        // The right/top edge already lies one cell past the last cell.
        assert_eq!(world_to_grid(Vec2::new(PLAY_AREA_WIDTH / 2.0, 0.0)), None);
        assert_eq!(world_to_grid(Vec2::new(0.0, PLAY_AREA_HEIGHT / 2.0)), None);
        assert_eq!(world_to_grid(Vec2::new(PLAY_AREA_WIDTH, 0.0)), None);
        assert_eq!(world_to_grid(Vec2::new(0.0, -PLAY_AREA_HEIGHT)), None);

        // Euclidean floor: values even slightly past the left/bottom edge are
        // outside the grid, not truncated into cell 0.
        assert_eq!(
            world_to_grid(Vec2::new(-PLAY_AREA_WIDTH / 2.0 - 0.5, 0.0)),
            None
        );
        assert_eq!(
            world_to_grid(Vec2::new(0.0, -PLAY_AREA_HEIGHT / 2.0 - 0.5)),
            None
        );
        assert_eq!(
            world_to_grid(Vec2::new(-PLAY_AREA_WIDTH / 2.0 - GRID_SIZE, 0.0)),
            None
        );
    }

    #[test]
    fn world_to_index_matches_the_half_open_contract() {
        assert_eq!(
            world_to_index(
                Vec2::new(-PLAY_AREA_WIDTH / 2.0, -PLAY_AREA_HEIGHT / 2.0),
                GRID_SIZE,
                GRID_WIDTH,
                GRID_HEIGHT
            ),
            Some(0)
        );

        let last_index = GRID_WIDTH * GRID_HEIGHT - 1;
        assert_eq!(
            world_to_index(
                Vec2::new(PLAY_AREA_WIDTH / 2.0 - 0.1, PLAY_AREA_HEIGHT / 2.0 - 0.1),
                GRID_SIZE,
                GRID_WIDTH,
                GRID_HEIGHT
            ),
            Some(last_index)
        );
        assert_eq!(
            world_to_index(
                Vec2::new(PLAY_AREA_WIDTH / 2.0, 0.0),
                GRID_SIZE,
                GRID_WIDTH,
                GRID_HEIGHT
            ),
            None
        );
        assert_eq!(
            world_to_index(
                Vec2::new(-PLAY_AREA_WIDTH / 2.0 - 0.1, 0.0),
                GRID_SIZE,
                GRID_WIDTH,
                GRID_HEIGHT
            ),
            None
        );
    }

    #[test]
    fn index_to_cell_inverts_row_major_order() {
        assert_eq!(index_to_cell(3, 10), UVec2::new(3, 0));
        assert_eq!(index_to_cell(2 * 10 + 3, 10), UVec2::new(3, 2));

        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                let cell = UVec2::new(x, y);
                assert_eq!(index_to_cell(pack(cell) as usize, GRID_WIDTH), cell);
            }
        }
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
}
