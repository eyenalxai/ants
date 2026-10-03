//! Food storage: single source of truth for food amounts and their markers.

use bevy::prelude::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::constants::world::{
    DENSITY_CELL_SIZE, DENSITY_GRID_HEIGHT, DENSITY_GRID_WIDTH, FOOD_CELL_RADIUS, FOOD_X, FOOD_Y,
    GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, INITIAL_FOOD_AMOUNT,
};
use crate::core::grid::{grid_to_world, in_bounds, pack, unpack, world_to_grid, world_to_index};
use crate::core::layers::Z_FOOD;

/// Food cells per density cell along one axis. Queries bucket stored cells by
/// density cell, so this relationship is load-bearing.
const FOOD_CELLS_PER_DENSITY_CELL: i64 = (DENSITY_CELL_SIZE / GRID_SIZE) as i64;

/// Marker entity for one food cell.
#[derive(Component)]
pub struct FoodMarker {
    pub cell: UVec2,
}

/// Amounts and marker entities for every food cell in the world.
///
/// Amounts live in a `BTreeMap` so iteration is stable and tie-breaking does
/// not depend on hash-map randomization. `nearest_within` does not scan that
/// map: `presence` and `buckets` form a density-cell-resolution spatial index
/// maintained by `set`/`remove` (and the depletion pass), so a query only
/// visits local cells.
#[derive(Resource)]
pub struct FoodGrid {
    amounts: BTreeMap<u32, f32>,
    /// Per-cell food quality, flat row-major (`pack(cell)`), defaulting to
    /// `1.0`. Owned by the food stream; independent of whether the cell is
    /// currently stored, so a painted cell can carry a value.
    quality: Box<[f32]>,
    markers: HashMap<u32, Entity>,
    /// Number of stored cells per density cell; a zero byte rejects the whole
    /// bucket without touching it.
    presence: Box<[u8]>,
    /// Packed food-cell keys per density cell.
    buckets: Box<[Vec<u32>]>,
    /// Reused key buffer for the per-tick depletion pass.
    scratch: Vec<u32>,
}

impl Default for FoodGrid {
    fn default() -> Self {
        let density_cells = DENSITY_GRID_WIDTH * DENSITY_GRID_HEIGHT;
        Self {
            amounts: BTreeMap::new(),
            quality: vec![1.0; GRID_WIDTH * GRID_HEIGHT].into_boxed_slice(),
            markers: HashMap::new(),
            presence: vec![0; density_cells].into_boxed_slice(),
            buckets: vec![Vec::new(); density_cells].into_boxed_slice(),
            scratch: Vec::new(),
        }
    }
}

impl FoodGrid {
    pub fn amount(&self, cell: UVec2) -> Option<f32> {
        self.amounts.get(&pack(cell)).copied()
    }

    pub fn contains(&self, cell: UVec2) -> bool {
        self.amounts.contains_key(&pack(cell))
    }

    /// Food quality of the cell at flat index `cell` ([`pack`] of the cell).
    ///
    /// Quality is a per-cell multiplier in the food stream's domain, stored
    /// independently of the amount: it survives depletion and re-painting, and
    /// the biology stream reads it at pickup into `Ant::carrying_quality`.
    /// Every cell defaults to the neutral `1.0`; out-of-range indices return
    /// that default too.
    pub fn quality(&self, cell: u32) -> f32 {
        self.quality.get(cell as usize).copied().unwrap_or(1.0)
    }

    /// Set the food quality of the cell at flat index `cell` ([`pack`]).
    ///
    /// This does not touch the amount, the marker or the spatial index:
    /// quality is a property of the location. Out-of-range indices are a
    /// no-op.
    pub fn set_quality(&mut self, cell: u32, quality: f32) {
        if let Some(slot) = self.quality.get_mut(cell as usize) {
            *slot = quality;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (UVec2, f32)> + '_ {
        self.amounts
            .iter()
            .map(|(&key, &amount)| (unpack(key), amount))
    }

    /// Insert or update `cell`, ensuring a marker entity exists for it.
    pub fn set(&mut self, commands: &mut Commands, cell: UVec2, amount: f32) {
        let key = pack(cell);

        if self.amounts.insert(key, amount).is_none() {
            self.index_insert(cell, key);
        }

        if let Entry::Vacant(slot) = self.markers.entry(key) {
            let world = grid_to_world(cell);
            let entity = commands
                .spawn((
                    FoodMarker { cell },
                    Sprite {
                        color: Color::srgb(0.2, 0.8, 0.2),
                        custom_size: Some(Vec2::splat(FOOD_CELL_RADIUS * 2.0)),
                        ..default()
                    },
                    Transform::from_xyz(world.x, world.y, Z_FOOD),
                ))
                .id();
            slot.insert(entity);
        }
    }

    /// Subtract `amount`, saturating at zero. Returns how much was actually
    /// taken. Zero-amount cells stay stored (and indexed) until the next
    /// [`Self::remove_depleted`] pass.
    pub fn take(&mut self, cell: UVec2, amount: f32) -> f32 {
        let Some(current) = self.amounts.get_mut(&pack(cell)) else {
            return 0.0;
        };

        let available = current.max(0.0);
        let taken = amount.max(0.0).min(available);
        *current = available - taken;
        taken
    }

    /// Remove `cell` and despawn its marker entity.
    pub fn remove(&mut self, commands: &mut Commands, cell: UVec2) {
        let key = pack(cell);
        if self.amounts.remove(&key).is_some() {
            self.index_remove(key);

            if let Some(entity) = self.markers.remove(&key) {
                commands.entity(entity).despawn();
            }
        }
    }

    /// Nearest stored food cell within `radius_cells` of `center`, returned
    /// with the distance between the two cell centers in world units.
    ///
    /// Semantics match the original full-map scan exactly: cells are compared
    /// by Euclidean cell-center distance, `radius_cells` is inclusive, and
    /// equidistant cells resolve to the lowest flat cell index (row-major),
    /// which is the order a `BTreeMap` scan visited them in. Only density-cell
    /// buckets intersecting the query circle are visited.
    pub fn nearest_within(&self, center: UVec2, radius_cells: i32) -> Option<(UVec2, f32)> {
        self.nearest_within_matching(center, radius_cells, |_| true)
    }

    /// [`Self::nearest_within`] restricted to cells accepted by `accept`.
    ///
    /// Used by food sensing, which needs the nearest cell inside the ant's
    /// forward cone without a global fallback scan.
    pub fn nearest_within_matching(
        &self,
        center: UVec2,
        radius_cells: i32,
        mut accept: impl FnMut(UVec2) -> bool,
    ) -> Option<(UVec2, f32)> {
        if radius_cells < 0 {
            return None;
        }

        let max_distance = radius_cells as f32 * GRID_SIZE;
        let max_d2 = max_distance * max_distance;
        let cx = center.x as i64;
        let cy = center.y as i64;
        let r = radius_cells as i64;
        let last_density_x = DENSITY_GRID_WIDTH as i64 - 1;
        let last_density_y = DENSITY_GRID_HEIGHT as i64 - 1;

        // Every cell whose center is within the radius has an index in
        // `[c - r, c + r]`; map that range to the density cells that hold it.
        let min_dx =
            ((cx - r).div_euclid(FOOD_CELLS_PER_DENSITY_CELL)).clamp(0, last_density_x) as usize;
        let max_dx =
            ((cx + r).div_euclid(FOOD_CELLS_PER_DENSITY_CELL)).clamp(0, last_density_x) as usize;
        let min_dy =
            ((cy - r).div_euclid(FOOD_CELLS_PER_DENSITY_CELL)).clamp(0, last_density_y) as usize;
        let max_dy =
            ((cy + r).div_euclid(FOOD_CELLS_PER_DENSITY_CELL)).clamp(0, last_density_y) as usize;

        let mut best: Option<(UVec2, f32, u32)> = None;

        for density_y in min_dy..=max_dy {
            let row = density_y * DENSITY_GRID_WIDTH;

            for density_x in min_dx..=max_dx {
                let density = row + density_x;
                if self.presence[density] == 0 {
                    continue;
                }

                for &key in &self.buckets[density] {
                    let cell = unpack(key);
                    let ox = (cell.x as i64 - cx) as f32 * GRID_SIZE;
                    let oy = (cell.y as i64 - cy) as f32 * GRID_SIZE;
                    let d2 = ox * ox + oy * oy;

                    if d2 > max_d2 || !accept(cell) {
                        continue;
                    }

                    if best.is_none_or(|(_, best_d2, best_key)| {
                        d2 < best_d2 || (d2 == best_d2 && key < best_key)
                    }) {
                        best = Some((cell, d2, key));
                    }
                }
            }
        }

        best.map(|(cell, d2, _)| (cell, d2.sqrt()))
    }

    /// Cells with a non-positive amount, for a removal pass.
    pub fn depleted(&self) -> impl Iterator<Item = UVec2> {
        self.amounts
            .iter()
            .filter(|&(_, &amount)| amount <= 0.0)
            .map(|(&key, _)| unpack(key))
    }

    /// Remove every non-positive cell and despawn its marker, reusing an
    /// internal buffer so the per-tick pass allocates nothing.
    fn remove_depleted(&mut self, commands: &mut Commands) {
        // Move the buffer out so the depletion iterator can borrow `self`
        // while the keys are collected.
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.extend(self.depleted().map(pack));

        for key in scratch.drain(..) {
            if self.amounts.remove(&key).is_some() {
                self.index_remove(key);

                if let Some(entity) = self.markers.remove(&key) {
                    commands.entity(entity).despawn();
                }
            }
        }

        self.scratch = scratch;
    }

    /// Add `key` to its density bucket. Only in-bounds cells are indexed.
    fn index_insert(&mut self, cell: UVec2, key: u32) {
        if !in_bounds(cell) {
            return;
        }

        let Some(density) = density_cell_of(cell) else {
            return;
        };

        self.buckets[density].push(key);
        self.presence[density] = self.presence[density].saturating_add(1);
    }

    /// Drop `key` from its density bucket; leaves other cells untouched.
    fn index_remove(&mut self, key: u32) {
        let Some(density) = density_cell_of(unpack(key)) else {
            return;
        };

        let bucket = &mut self.buckets[density];
        if let Some(position) = bucket.iter().position(|&entry| entry == key) {
            bucket.swap_remove(position);
            self.presence[density] = self.presence[density].saturating_sub(1);
        }
    }
}

/// Density cell containing a food cell, or `None` when the cell has no density
/// bucket (out of the play area).
fn density_cell_of(cell: UVec2) -> Option<usize> {
    world_to_index(
        grid_to_world(cell),
        DENSITY_CELL_SIZE,
        DENSITY_GRID_WIDTH,
        DENSITY_GRID_HEIGHT,
    )
}

/// Spawn the initial 3x3 food patch at [`FOOD_X`], [`FOOD_Y`].
pub fn setup_food_patch(mut commands: Commands, mut food_grid: ResMut<FoodGrid>) {
    let Some(origin) = world_to_grid(Vec2::new(FOOD_X, FOOD_Y)) else {
        return;
    };

    for dy in 0..3 {
        for dx in 0..3 {
            let cell = UVec2::new(origin.x + dx, origin.y + dy);
            if in_bounds(cell) {
                food_grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
            }
        }
    }
}

/// Remove every depleted cell and its marker.
pub fn deplete_food(mut commands: Commands, mut food_grid: ResMut<FoodGrid>) {
    food_grid.remove_depleted(&mut commands);
}

/// Scale marker alpha with the remaining amount.
pub fn update_food_visuals(
    food_grid: Res<FoodGrid>,
    mut markers: Query<(&FoodMarker, &mut Sprite)>,
) {
    for (marker, mut sprite) in &mut markers {
        if let Some(amount) = food_grid.amount(marker.cell) {
            let opacity = (amount / INITIAL_FOOD_AMOUNT).clamp(0.0, 1.0);
            let color = Color::srgba(0.2, 0.8, 0.2, opacity);

            if sprite.color != color {
                sprite.color = color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastrand::Rng;
    use std::hint::black_box;
    use std::time::Instant;

    fn food_grid_with(cells: impl IntoIterator<Item = (UVec2, f32)>) -> FoodGrid {
        let mut world = World::new();
        let mut commands = world.commands();
        let mut grid = FoodGrid::default();

        for (cell, amount) in cells {
            grid.set(&mut commands, cell, amount);
        }

        grid
    }

    /// The pre-index full-scan algorithm, kept as the equivalence oracle.
    fn full_scan_nearest(
        grid: &FoodGrid,
        center: UVec2,
        radius_cells: i32,
    ) -> Option<(UVec2, f32)> {
        let center_pos = grid_to_world(center);
        let max_distance = radius_cells as f32 * GRID_SIZE;
        let mut nearest: Option<(UVec2, f32)> = None;

        for &key in grid.amounts.keys() {
            let cell = unpack(key);
            let distance = center_pos.distance(grid_to_world(cell));
            if distance <= max_distance
                && nearest.is_none_or(|(_, nearest_distance)| distance < nearest_distance)
            {
                nearest = Some((cell, distance));
            }
        }

        nearest
    }

    fn random_cell(rng: &mut Rng) -> UVec2 {
        UVec2::new(
            rng.u32(0..GRID_WIDTH as u32),
            rng.u32(0..GRID_HEIGHT as u32),
        )
    }

    #[test]
    fn nearest_within_matches_full_scan_on_random_layouts() {
        let mut rng = Rng::with_seed(0xC0FFEE);

        for _case in 0..40 {
            let mut grid = FoodGrid::default();
            let mut world = World::new();
            let mut commands = world.commands();

            for _ in 0..rng.usize(0..150) {
                // A few zero-amount cells exercise cells awaiting removal.
                let amount = if rng.f32() < 0.15 {
                    0.0
                } else {
                    rng.f32() * 100.0
                };
                grid.set(&mut commands, random_cell(&mut rng), amount);
            }

            for _query in 0..80 {
                let center = random_cell(&mut rng);
                let radius = rng.i32(0..=14);
                assert_eq!(
                    grid.nearest_within(center, radius),
                    full_scan_nearest(&grid, center, radius),
                    "mismatch for center {center:?} radius {radius}"
                );
            }
        }
    }

    #[test]
    fn nearest_within_radius_is_inclusive() {
        // Center (5,5); cells at integer cell offsets from it.
        let center = UVec2::new(5, 5);
        let grid = food_grid_with([
            (UVec2::new(8, 5), 1.0), // 3 cells right: 12 world units
            (UVec2::new(5, 9), 1.0), // 4 cells up: 16 world units
        ]);

        // Radius 3 covers the cell at distance 12, not the one at distance 16.
        assert_eq!(
            grid.nearest_within(center, 3),
            Some((UVec2::new(8, 5), 12.0))
        );

        // Radius 4 covers both; the closer one wins.
        assert_eq!(
            grid.nearest_within(center, 4),
            Some((UVec2::new(8, 5), 12.0))
        );

        // Radius 5 reaches the farther cell when the closer one is filtered.
        assert_eq!(
            grid.nearest_within_matching(center, 5, |cell| cell.y > 5),
            Some((UVec2::new(5, 9), 16.0))
        );
    }

    #[test]
    fn equidistant_cells_break_ties_by_lowest_index() {
        // (4,5) and (6,5) are equidistant from (5,5); row-major order picks
        // (4,5), exactly like the old BTreeMap scan.
        let grid = food_grid_with([(UVec2::new(6, 5), 1.0), (UVec2::new(4, 5), 1.0)]);
        let (cell, distance) = grid.nearest_within(UVec2::new(5, 5), 1).unwrap();

        assert_eq!(cell, UVec2::new(4, 5));
        assert_eq!(distance, GRID_SIZE);
    }

    #[test]
    fn index_survives_set_take_remove_and_depletion() {
        let mut world = World::new();
        let mut commands = world.commands();
        let mut grid = FoodGrid::default();
        let near = UVec2::new(10, 10);
        let far = UVec2::new(12, 10);

        grid.set(&mut commands, far, 1.0);
        grid.set(&mut commands, near, 1.0);
        // Re-setting an existing cell must not duplicate its bucket entry.
        grid.set(&mut commands, near, 2.0);
        assert_eq!(grid.nearest_within(near, 1), Some((near, 0.0)));

        grid.remove(&mut commands, near);
        assert_eq!(
            grid.nearest_within(near, 2),
            Some((far, 2.0 * GRID_SIZE)),
            "removal must drop the cell from the spatial index"
        );

        // A partial take leaves the cell indexed (it is still stored) until
        // the depletion pass removes it.
        grid.set(&mut commands, near, 0.5);
        assert_eq!(grid.take(near, 1.0), 0.5);
        assert!(grid.contains(near));
        assert_eq!(grid.nearest_within(near, 1), Some((near, 0.0)));

        grid.remove_depleted(&mut commands);
        assert!(!grid.contains(near));
        assert_eq!(
            grid.nearest_within(near, 2),
            Some((far, 2.0 * GRID_SIZE)),
            "depleted cells must leave the spatial index"
        );

        grid.remove(&mut commands, far);
        assert_eq!(grid.nearest_within(near, 5), None);
    }

    #[test]
    fn quality_defaults_to_one_and_roundtrips_per_cell() {
        let mut world = World::new();
        let mut commands = world.commands();
        let mut grid = FoodGrid::default();
        let empty = pack(UVec2::new(4, 4));
        let stored_cell = UVec2::new(7, 9);
        let stored = pack(stored_cell);

        // Default: neutral quality on empty and stored cells alike.
        assert_eq!(grid.quality(empty), 1.0);
        assert_eq!(grid.quality(stored), 1.0);

        // Roundtrip on an empty (not stored) cell.
        grid.set_quality(empty, 0.4);
        assert_eq!(grid.quality(empty), 0.4);
        assert!(!grid.contains(unpack(empty)));

        // Roundtrip on a stored cell; quality is independent of the amount.
        grid.set(&mut commands, stored_cell, INITIAL_FOOD_AMOUNT);
        grid.set_quality(stored, 1.7);
        assert_eq!(grid.quality(stored), 1.7);
        assert_eq!(grid.amount(stored_cell), Some(INITIAL_FOOD_AMOUNT));

        // Out-of-range indices read the neutral default and ignore writes.
        let out_of_range = (GRID_WIDTH * GRID_HEIGHT) as u32;
        assert_eq!(grid.quality(out_of_range), 1.0);
        grid.set_quality(out_of_range, 0.2);
        assert_eq!(grid.quality(out_of_range), 1.0);
    }

    #[test]
    fn empty_grid_and_negative_radius_return_none() {
        let empty = FoodGrid::default();
        assert_eq!(empty.nearest_within(UVec2::new(3, 3), 10), None);

        let grid = food_grid_with([(UVec2::new(3, 3), 1.0)]);
        assert_eq!(grid.nearest_within(UVec2::new(3, 3), -1), None);
    }

    #[test]
    fn nearest_within_handles_grid_edges() {
        let corner = UVec2::new(0, 0);
        let grid = food_grid_with([(corner, 1.0), (UVec2::new(1, 0), 1.0)]);

        assert_eq!(grid.nearest_within(UVec2::ZERO, 1), Some((corner, 0.0)));

        // A query at the far corner still finds the local cells.
        let far = UVec2::new(GRID_WIDTH as u32 - 1, GRID_HEIGHT as u32 - 1);
        assert_eq!(grid.nearest_within(far, 3), None);
        let (cell, distance) = grid
            .nearest_within_matching(far, 250, |cell| cell.x == 0 && cell.y == 0)
            .unwrap();
        assert_eq!(cell, corner);
        assert!((distance - grid_to_world(corner).distance(grid_to_world(far))).abs() < 1e-3);
    }

    /// Measurement harness comparing the indexed query against the old full
    /// scan. Run with:
    /// `cargo test --release bench_nearest_within -- --ignored --nocapture`
    #[test]
    #[ignore = "measurement harness, run on demand"]
    fn bench_nearest_within() {
        const QUERIES: usize = 30_000;
        let mut rng = Rng::with_seed(0xBEEF);

        for cells in [9, 600, 1500] {
            let mut grid = FoodGrid::default();
            let mut world = World::new();
            let mut commands = world.commands();

            // A contiguous patch of `cells` food cells around the middle.
            let side = (cells as f32).sqrt().ceil() as u32;
            let start_x = (GRID_WIDTH as u32 - side) / 2;
            let start_y = (GRID_HEIGHT as u32 - side) / 2;
            for i in 0..cells as u32 {
                let cell = UVec2::new(start_x + i % side, start_y + i / side);
                grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
            }

            let queries: Vec<(UVec2, i32)> =
                (0..QUERIES).map(|_| (random_cell(&mut rng), 5)).collect();

            let start = Instant::now();
            let mut indexed_sum = 0.0f32;
            for &(center, radius) in &queries {
                if let Some((_, distance)) = grid.nearest_within(center, radius) {
                    indexed_sum += distance;
                }
            }
            let indexed = start.elapsed();

            let start = Instant::now();
            let mut scan_sum = 0.0f32;
            for &(center, radius) in &queries {
                if let Some((_, distance)) = full_scan_nearest(&grid, center, radius) {
                    scan_sum += distance;
                }
            }
            let scan = start.elapsed();

            black_box(indexed_sum);
            black_box(scan_sum);

            println!(
                "{cells:5} food cells: full scan {:8.2} ms | indexed {:8.2} ms | {:6.1}x faster",
                scan.as_secs_f64() * 1e3,
                indexed.as_secs_f64() * 1e3,
                scan.as_secs_f64() / indexed.as_secs_f64().max(f64::MIN_POSITIVE)
            );
        }
    }
}
