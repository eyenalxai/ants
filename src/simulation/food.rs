//! Food storage: single source of truth for food amounts and their markers.

use bevy::prelude::*;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::constants::world::{
    DENSITY_CELL_SIZE, DENSITY_GRID_HEIGHT, DENSITY_GRID_WIDTH, FOOD_CELL_RADIUS, FOOD_QUALITY_MAX,
    FOOD_QUALITY_MIN, FOOD_X, FOOD_Y, GRID_HEIGHT, GRID_SIZE, GRID_WIDTH, INITIAL_FOOD_AMOUNT,
};
use crate::core::grid::{grid_to_world, in_bounds, pack, unpack, world_to_grid, world_to_index};
use crate::core::layers::Z_FOOD;

/// Food cells per density cell along one axis. Queries bucket stored cells by
/// density cell, so this relationship is load-bearing.
const FOOD_CELLS_PER_DENSITY_CELL: i64 = (DENSITY_CELL_SIZE / GRID_SIZE) as i64;

/// Number of quality steps in the deterministic initial patch pattern.
const FOOD_QUALITY_STEPS: u32 = 3;

/// Marker entity for one food cell.
#[derive(Component)]
pub struct FoodMarker {
    pub cell: UVec2,
}

/// Amounts, quality and marker entities for every food cell in the world.
///
/// Amounts live in a dense `Box<[f32]>` indexed by [`pack`] (30 000 cells,
/// ~120 KB), so reads and writes are O(1) and storage order is the row-major
/// cell order. A cell counts as *stored* while `amounts[i] != 0.0` or its
/// marker still exists; cells reduced to zero by [`FoodGrid::take`] therefore
/// stay stored and indexed until [`FoodGrid::remove_depleted`] runs, which is
/// what keeps partial loads and the spatial index consistent.
///
/// `presence` and `buckets` form a density-cell-resolution spatial index
/// maintained by `set`/`remove`/`remove_depleted`, so a query only visits
/// local cells. Equidistant cells resolve to the lowest flat cell index
/// (row-major), which is also the order [`FoodGrid::iter`] reports. Storage
/// methods ignore out-of-bounds cells.
///
/// `version` increments on every observable mutation ([`FoodGrid::set`], a
/// non-zero [`FoodGrid::take`], [`FoodGrid::remove`], a non-empty
/// [`FoodGrid::remove_depleted`]) so [`update_food_visuals`] can skip clean
/// ticks; `Res::is_changed` cannot do that because the depletion system takes
/// `ResMut` every tick.
#[derive(Resource)]
pub struct FoodGrid {
    /// Dense per-cell amounts, flat row-major ([`pack`]).
    amounts: Box<[f32]>,
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
    /// Dirty counter for the visual pass; see the type-level docs.
    version: u64,
}

impl Default for FoodGrid {
    fn default() -> Self {
        let density_cells = DENSITY_GRID_WIDTH * DENSITY_GRID_HEIGHT;
        Self {
            amounts: vec![0.0; GRID_WIDTH * GRID_HEIGHT].into_boxed_slice(),
            quality: vec![1.0; GRID_WIDTH * GRID_HEIGHT].into_boxed_slice(),
            markers: HashMap::new(),
            presence: vec![0; density_cells].into_boxed_slice(),
            buckets: vec![Vec::new(); density_cells].into_boxed_slice(),
            scratch: Vec::new(),
            version: 0,
        }
    }
}

impl FoodGrid {
    pub fn amount(&self, cell: UVec2) -> Option<f32> {
        if !in_bounds(cell) {
            return None;
        }

        let key = pack(cell);
        let amount = self.amounts[key as usize];

        (amount != 0.0 || self.markers.contains_key(&key)).then_some(amount)
    }

    pub fn contains(&self, cell: UVec2) -> bool {
        self.amount(cell).is_some()
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

    /// Every stored cell and its amount, in row-major cell order.
    ///
    /// Zero-amount cells awaiting [`Self::remove_depleted`] are included, just
    /// like the keys of the old map storage.
    pub fn iter(&self) -> impl Iterator<Item = (UVec2, f32)> + '_ {
        self.amounts
            .iter()
            .enumerate()
            .filter_map(move |(index, &amount)| {
                let key = index as u32;
                if amount != 0.0 || self.markers.contains_key(&key) {
                    Some((unpack(key), amount))
                } else {
                    None
                }
            })
    }

    /// Insert or update `cell`, ensuring a marker entity exists for it.
    ///
    /// Out-of-bounds cells are ignored.
    pub fn set(&mut self, commands: &mut Commands, cell: UVec2, amount: f32) {
        if !in_bounds(cell) {
            return;
        }

        let key = pack(cell);
        let was_stored = self.amounts[key as usize] != 0.0 || self.markers.contains_key(&key);
        self.amounts[key as usize] = amount;

        if !was_stored {
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

        self.version += 1;
    }

    /// Subtract `amount`, saturating at zero. Returns how much was actually
    /// taken. Zero-amount cells stay stored (and indexed) until the next
    /// [`Self::remove_depleted`] pass.
    ///
    /// Only a non-zero take is an observable mutation and bumps the version.
    pub fn take(&mut self, cell: UVec2, amount: f32) -> f32 {
        if !self.contains(cell) {
            return 0.0;
        }

        let current = &mut self.amounts[pack(cell) as usize];
        let available = current.max(0.0);
        let taken = amount.max(0.0).min(available);
        *current = available - taken;

        if taken > 0.0 {
            self.version += 1;
        }

        taken
    }

    /// Remove `cell` and despawn its marker entity.
    ///
    /// Out-of-bounds and unstored cells are ignored.
    pub fn remove(&mut self, commands: &mut Commands, cell: UVec2) {
        if !self.contains(cell) {
            return;
        }

        let key = pack(cell);
        self.amounts[key as usize] = 0.0;
        self.index_remove(key);

        if let Some(entity) = self.markers.remove(&key) {
            commands.entity(entity).despawn();
        }

        self.version += 1;
    }

    /// Nearest stored food cell within `radius_cells` of `center`, returned
    /// with the distance between the two cell centers in world units.
    ///
    /// Cells are compared by Euclidean cell-center distance, `radius_cells` is
    /// inclusive, and equidistant cells resolve to the lowest flat cell index
    /// (row-major). Only density-cell buckets intersecting the query circle
    /// are visited.
    pub fn nearest_within(&self, center: UVec2, radius_cells: i32) -> Option<(UVec2, f32)> {
        self.nearest_within_matching(center, radius_cells, |_| true)
    }

    /// [`Self::nearest_within`] restricted to cells accepted by `accept`.
    ///
    /// Used by food sensing, which needs the nearest cell inside the ant's
    /// forward cone without a global fallback scan.
    ///
    /// `accept` must be a pure function of the cell: candidates that are
    /// strictly farther than the current best are rejected *before* `accept`
    /// runs, so callers must not rely on it being called for every candidate
    /// in range (or on call order). Equal distances still run `accept`, so the
    /// lowest-index tie-break is exact.
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

                    if d2 > max_d2 {
                        continue;
                    }

                    // Exact early reject: a candidate strictly farther than
                    // the current best can never win, and `accept` is pure, so
                    // skipping it cannot change the result. The comparison is
                    // strict so equal-distance candidates still run `accept`
                    // for the lowest-index tie-break.
                    if best.is_some_and(|(_, best_d2, _)| d2 > best_d2) {
                        continue;
                    }

                    if !accept(cell) {
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
    ///
    /// Iterates the stored cells through the density buckets, so the cost is
    /// proportional to stored cells instead of the 30 000-cell grid. Order is
    /// the spatial-index order; [`Self::remove_depleted`] is the only consumer
    /// and does not depend on it.
    pub fn depleted(&self) -> impl Iterator<Item = UVec2> + '_ {
        self.buckets.iter().flatten().filter_map(move |&key| {
            if self.amounts[key as usize] <= 0.0 {
                Some(unpack(key))
            } else {
                None
            }
        })
    }

    /// Remove every non-positive cell and despawn its marker, reusing an
    /// internal buffer so the per-tick pass allocates nothing.
    fn remove_depleted(&mut self, commands: &mut Commands) {
        // Move the buffer out so the depletion iterator can borrow `self`
        // while the keys are collected.
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.extend(self.depleted().map(pack));

        if !scratch.is_empty() {
            for key in scratch.drain(..) {
                self.amounts[key as usize] = 0.0;
                self.index_remove(key);

                if let Some(entity) = self.markers.remove(&key) {
                    commands.entity(entity).despawn();
                }
            }

            self.version += 1;
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

/// Deterministic per-cell quality for the initial patch.
///
/// `FOOD_QUALITY_STEPS` evenly spaced steps across
/// `FOOD_QUALITY_MIN..=FOOD_QUALITY_MAX`, selected by the cell's own
/// coordinates (`(x + 2y) mod 3`). No RNG is involved, so every run gets the
/// same patch; the pattern balances a 3×3 patch (each step appears three
/// times), keeping its mean quality neutral at 1.0.
fn initial_quality(cell: UVec2) -> f32 {
    let step = (cell.x + 2 * cell.y) % FOOD_QUALITY_STEPS;
    let t = step as f32 / (FOOD_QUALITY_STEPS - 1) as f32;
    FOOD_QUALITY_MIN + (FOOD_QUALITY_MAX - FOOD_QUALITY_MIN) * t
}

/// Spawn the initial 3x3 food patch at [`FOOD_X`], [`FOOD_Y`].
///
/// Every cell starts with [`INITIAL_FOOD_AMOUNT`] units (a finite source of
/// `9 × INITIAL_FOOD_AMOUNT` units, i.e. that many full trips) and a
/// deterministic quality from [`initial_quality`], so the patch is both
/// depletable and heterogeneous.
pub fn setup_food_patch(mut commands: Commands, mut food_grid: ResMut<FoodGrid>) {
    let Some(origin) = world_to_grid(Vec2::new(FOOD_X, FOOD_Y)) else {
        return;
    };

    for dy in 0..3 {
        for dx in 0..3 {
            let cell = UVec2::new(origin.x + dx, origin.y + dy);
            if in_bounds(cell) {
                food_grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
                food_grid.set_quality(pack(cell), initial_quality(cell));
            }
        }
    }
}

/// Remove every depleted cell and its marker.
pub fn deplete_food(mut commands: Commands, mut food_grid: ResMut<FoodGrid>) {
    food_grid.remove_depleted(&mut commands);
}

/// Scale marker alpha with the remaining amount.
///
/// Runs only when [`FoodGrid::version`] changed since the last pass: the
/// depletion pass takes `ResMut` every tick, so `Res::is_changed` cannot
/// express "the food actually changed". `Local<Option<u64>>` (rather than a
/// bare `u64`) guarantees one pass on the first tick even before any mutation.
pub fn update_food_visuals(
    food_grid: Res<FoodGrid>,
    mut markers: Query<(&FoodMarker, &mut Sprite)>,
    mut last_version: Local<Option<u64>>,
) {
    if *last_version == Some(food_grid.version) {
        return;
    }

    *last_version = Some(food_grid.version);

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
    use bevy::ecs::system::RunSystemOnce;
    use fastrand::Rng;
    use std::collections::BTreeMap;
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    use crate::constants::ant::{FOOD_SENSE_HALF_ANGLE, FOOD_SENSE_RANGE};
    use crate::simulation::movement::steering::shortest_angle_diff;

    fn food_grid_with(cells: impl IntoIterator<Item = (UVec2, f32)>) -> FoodGrid {
        let mut world = World::new();
        let mut commands = world.commands();
        let mut grid = FoodGrid::default();

        for (cell, amount) in cells {
            grid.set(&mut commands, cell, amount);
        }

        grid
    }

    /// Stored cells in row-major order, collected once per layout so the
    /// full-scan oracles below stay cheap.
    fn stored_cells(grid: &FoodGrid) -> Vec<(UVec2, f32)> {
        grid.iter().collect()
    }

    /// The pre-index full-scan algorithm, kept as the equivalence oracle. It
    /// walks every stored cell (row-major order) and applies the same
    /// lowest-index tie-break as the indexed query.
    fn full_scan_nearest(
        stored: &[(UVec2, f32)],
        center: UVec2,
        radius_cells: i32,
    ) -> Option<(UVec2, f32)> {
        let center_pos = grid_to_world(center);
        let max_distance = radius_cells as f32 * GRID_SIZE;
        let mut nearest: Option<(UVec2, f32)> = None;

        for &(cell, _) in stored {
            let distance = center_pos.distance(grid_to_world(cell));
            if distance <= max_distance
                && nearest.is_none_or(|(_, nearest_distance)| distance < nearest_distance)
            {
                nearest = Some((cell, distance));
            }
        }

        nearest
    }

    /// Full-scan oracle for [`FoodGrid::nearest_within_matching`]. It computes
    /// `d2` with the same arithmetic as the indexed query so the comparison is
    /// bit-exact, and evaluates `accept` for every stored cell in range.
    fn full_scan_nearest_matching(
        stored: &[(UVec2, f32)],
        center: UVec2,
        radius_cells: i32,
        mut accept: impl FnMut(UVec2) -> bool,
    ) -> Option<(UVec2, f32)> {
        if radius_cells < 0 {
            return None;
        }

        let cx = center.x as i64;
        let cy = center.y as i64;
        let max_distance = radius_cells as f32 * GRID_SIZE;
        let max_d2 = max_distance * max_distance;
        let mut best: Option<(UVec2, f32, u32)> = None;

        for &(cell, _) in stored {
            let ox = (cell.x as i64 - cx) as f32 * GRID_SIZE;
            let oy = (cell.y as i64 - cy) as f32 * GRID_SIZE;
            let d2 = ox * ox + oy * oy;
            let key = pack(cell);

            if d2 > max_d2 || !accept(cell) {
                continue;
            }

            if best.is_none_or(|(_, best_d2, best_key)| {
                d2 < best_d2 || (d2 == best_d2 && key < best_key)
            }) {
                best = Some((cell, d2, key));
            }
        }

        best.map(|(cell, d2, _)| (cell, d2.sqrt()))
    }

    /// The pre-early-reject indexed bucket walk, kept as the benchmark
    /// baseline: `accept` runs for every stored cell inside the radius.
    fn indexed_matching_without_early_reject(
        grid: &FoodGrid,
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
                if grid.presence[density] == 0 {
                    continue;
                }

                for &key in &grid.buckets[density] {
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

    fn random_cell(rng: &mut Rng) -> UVec2 {
        UVec2::new(
            rng.u32(0..GRID_WIDTH as u32),
            rng.u32(0..GRID_HEIGHT as u32),
        )
    }

    /// Time one full query pass and return its summed distances.
    fn time_queries(
        queries: &[(UVec2, Vec2, f32)],
        mut query: impl FnMut(UVec2, Vec2, f32) -> Option<(UVec2, f32)>,
    ) -> (Duration, f32) {
        let start = Instant::now();
        let mut sum = 0.0f32;

        for &(center, pos, direction) in queries {
            if let Some((_, distance)) = query(center, pos, direction) {
                sum += distance;
            }
        }

        (start.elapsed(), sum)
    }

    fn marker_count(world: &mut World) -> usize {
        let mut query = world.query::<&FoodMarker>();
        query.iter(world).count()
    }

    fn sprite_alpha(world: &mut World, cell: UVec2) -> Option<f32> {
        let mut query = world.query::<(&FoodMarker, &Sprite)>();
        query
            .iter(world)
            .find(|(marker, _)| marker.cell == cell)
            .map(|(_, sprite)| sprite.color.to_srgba().alpha)
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

            let stored = stored_cells(&grid);

            for _query in 0..80 {
                let center = random_cell(&mut rng);
                let radius = rng.i32(0..=14);
                assert_eq!(
                    grid.nearest_within(center, radius),
                    full_scan_nearest(&stored, center, radius),
                    "mismatch for center {center:?} radius {radius}"
                );
            }
        }
    }

    #[test]
    fn nearest_within_matching_matches_full_scan_on_random_layouts() {
        let mut rng = Rng::with_seed(0xBAD5EED);

        for case in 0..40u32 {
            let mut grid = FoodGrid::default();
            let mut world = World::new();
            let mut commands = world.commands();

            for _ in 0..rng.usize(0..150) {
                let amount = if rng.f32() < 0.15 {
                    0.0
                } else {
                    rng.f32() * 100.0
                };
                grid.set(&mut commands, random_cell(&mut rng), amount);
            }

            // Pure acceptors with varying rejection patterns; `a`/`b` are
            // drawn once per layout so the predicate is stable across queries.
            let a = rng.u32(1..=7);
            let b = rng.u32(0..=7);
            let acceptors: Vec<Box<dyn Fn(UVec2) -> bool>> = vec![
                Box::new(|_| true),
                Box::new(move |cell: UVec2| {
                    !(cell.x.wrapping_mul(a).wrapping_add(cell.y.wrapping_mul(b))).is_multiple_of(3)
                }),
                Box::new(move |cell: UVec2| cell.x.is_multiple_of(2) || !cell.y.is_multiple_of(2)),
                Box::new(move |cell: UVec2| (cell.x + cell.y) % 5 == case % 5),
            ];
            let stored = stored_cells(&grid);

            for _query in 0..80 {
                let center = random_cell(&mut rng);
                let radius = rng.i32(0..=14);

                for (index, acceptor) in acceptors.iter().enumerate() {
                    let expected = full_scan_nearest_matching(&stored, center, radius, acceptor);
                    let actual = grid.nearest_within_matching(center, radius, acceptor);

                    assert_eq!(
                        actual, expected,
                        "mismatch for center {center:?} radius {radius} acceptor {index}"
                    );
                }
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
        // (4,5).
        let grid = food_grid_with([(UVec2::new(6, 5), 1.0), (UVec2::new(4, 5), 1.0)]);
        let (cell, distance) = grid.nearest_within(UVec2::new(5, 5), 1).unwrap();

        assert_eq!(cell, UVec2::new(4, 5));
        assert_eq!(distance, GRID_SIZE);
    }

    #[test]
    fn matching_keeps_equal_distance_ties_for_lowest_index() {
        // The early reject is strict (`d2 > best_d2`), so an equal-distance
        // candidate still runs `accept` and can win the lowest-index tie-break
        // even when it is visited after an equally distant one.
        let grid = food_grid_with([(UVec2::new(6, 5), 1.0), (UVec2::new(4, 5), 1.0)]);
        let (cell, distance) = grid
            .nearest_within_matching(UVec2::new(5, 5), 1, |_| true)
            .unwrap();

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
    fn zero_amount_cells_stay_stored_until_depleted() {
        let mut world = World::new();
        let mut commands = world.commands();
        let mut grid = FoodGrid::default();
        let cell = UVec2::new(6, 6);

        grid.set(&mut commands, cell, 0.0);
        assert!(grid.contains(cell));
        assert_eq!(grid.amount(cell), Some(0.0));
        assert_eq!(grid.iter().collect::<Vec<_>>(), vec![(cell, 0.0)]);
        assert_eq!(grid.depleted().collect::<Vec<_>>(), vec![cell]);

        grid.remove_depleted(&mut commands);
        assert!(!grid.contains(cell));
        assert_eq!(grid.amount(cell), None);
        assert_eq!(grid.iter().count(), 0);
        assert_eq!(grid.depleted().count(), 0);
    }

    #[test]
    fn iter_yields_stored_cells_in_row_major_order() {
        let grid = food_grid_with([
            (UVec2::new(7, 3), 1.0),
            (UVec2::new(2, 9), 2.0),
            (UVec2::new(5, 3), 3.0),
        ]);
        let cells: Vec<UVec2> = grid.iter().map(|(cell, _)| cell).collect();

        assert_eq!(
            cells,
            vec![UVec2::new(5, 3), UVec2::new(7, 3), UVec2::new(2, 9)]
        );
    }

    #[test]
    fn out_of_bounds_cells_are_ignored() {
        let mut world = World::new();
        let mut commands = world.commands();
        let mut grid = FoodGrid::default();
        let outside = UVec2::new(GRID_WIDTH as u32, 0);

        grid.set(&mut commands, outside, 5.0);
        assert!(!grid.contains(outside));
        assert_eq!(grid.amount(outside), None);
        assert_eq!(grid.take(outside, 1.0), 0.0);
        grid.remove(&mut commands, outside);
        assert_eq!(grid.iter().count(), 0);
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
    fn initial_quality_pattern_is_deterministic_and_balanced() {
        let origin = UVec2::new(100, 50);
        let mut qualities = Vec::new();

        for dy in 0..3 {
            for dx in 0..3 {
                let cell = origin + UVec2::new(dx, dy);
                let quality = initial_quality(cell);

                assert_eq!(quality, initial_quality(cell), "same cell, same quality");
                assert!((FOOD_QUALITY_MIN..=FOOD_QUALITY_MAX).contains(&quality));
                qualities.push(quality);
            }
        }

        qualities.sort_by(f32::total_cmp);
        assert_eq!(qualities.first().copied(), Some(FOOD_QUALITY_MIN));
        assert_eq!(qualities.last().copied(), Some(FOOD_QUALITY_MAX));

        let mean = qualities.iter().sum::<f32>() / qualities.len() as f32;
        assert!(
            (mean - 1.0).abs() < 1e-5,
            "the 3×3 patch averages neutral quality, got {mean}"
        );
    }

    #[test]
    fn setup_food_patch_is_finite_and_heterogeneous() {
        let mut world = World::new();
        world.init_resource::<FoodGrid>();
        world
            .run_system_once(setup_food_patch)
            .expect("food patch setup");

        let grid = world.resource::<FoodGrid>();
        let cells: Vec<(UVec2, f32)> = grid.iter().collect();
        assert_eq!(cells.len(), 9, "the default patch is 3×3");
        assert!(
            cells
                .iter()
                .all(|(_, amount)| *amount == INITIAL_FOOD_AMOUNT)
        );

        let total: f32 = cells.iter().map(|(_, amount)| amount).sum();
        assert_eq!(
            total,
            9.0 * INITIAL_FOOD_AMOUNT,
            "the default patch must be a finite source"
        );

        let mut qualities: Vec<f32> = cells
            .iter()
            .map(|(cell, _)| grid.quality(pack(*cell)))
            .collect();
        qualities.sort_by(f32::total_cmp);
        assert_eq!(qualities.first().copied(), Some(FOOD_QUALITY_MIN));
        assert_eq!(qualities.last().copied(), Some(FOOD_QUALITY_MAX));
        assert!(
            qualities.windows(2).any(|pair| pair[0] != pair[1]),
            "the patch must be heterogeneous"
        );
    }

    #[test]
    fn finite_patch_depletes_after_all_units_are_taken() {
        let mut world = World::new();
        world.init_resource::<FoodGrid>();
        world
            .run_system_once(setup_food_patch)
            .expect("food patch setup");

        let cells: Vec<UVec2> = world
            .resource::<FoodGrid>()
            .iter()
            .map(|(cell, _)| cell)
            .collect();
        let mut taken_total = 0.0;

        for &cell in &cells {
            let mut grid = world.resource_mut::<FoodGrid>();
            // Partial loads: 60 + 60 + the rest takes the whole cell.
            taken_total += grid.take(cell, 60.0);
            taken_total += grid.take(cell, 60.0);
            taken_total += grid.take(cell, INITIAL_FOOD_AMOUNT);
        }

        assert_eq!(taken_total, 9.0 * INITIAL_FOOD_AMOUNT);
        {
            let grid = world.resource::<FoodGrid>();
            assert!(
                cells.iter().all(|&cell| grid.amount(cell) == Some(0.0)),
                "all cells must be stored-but-empty before the depletion pass"
            );
        }

        world.run_system_once(deplete_food).expect("depletion pass");

        let grid = world.resource::<FoodGrid>();
        assert_eq!(
            grid.iter().count(),
            0,
            "the finite patch must fully deplete"
        );
    }

    #[test]
    fn markers_follow_set_take_and_depletion() {
        let mut world = World::new();
        world.init_resource::<FoodGrid>();

        let kept = UVec2::new(8, 8);
        let depleted = UVec2::new(9, 8);

        world
            .run_system_once(move |mut grid: ResMut<FoodGrid>, mut commands: Commands| {
                grid.set(&mut commands, kept, INITIAL_FOOD_AMOUNT);
                grid.set(&mut commands, depleted, INITIAL_FOOD_AMOUNT / 2.0);
            })
            .expect("set");

        assert_eq!(marker_count(&mut world), 2);

        world
            .run_system_once(update_food_visuals)
            .expect("visual pass");
        assert_eq!(sprite_alpha(&mut world, kept), Some(1.0));
        assert_eq!(sprite_alpha(&mut world, depleted), Some(0.5));

        // Taking everything leaves the cell stored (marker alive) until the
        // depletion pass.
        world
            .run_system_once(move |mut grid: ResMut<FoodGrid>| {
                assert_eq!(
                    grid.take(depleted, INITIAL_FOOD_AMOUNT),
                    INITIAL_FOOD_AMOUNT / 2.0
                );
            })
            .expect("take");
        assert_eq!(marker_count(&mut world), 2);

        world.run_system_once(deplete_food).expect("depletion pass");
        assert_eq!(marker_count(&mut world), 1);

        // Removing the last cell despawns its marker too.
        world
            .run_system_once(move |mut grid: ResMut<FoodGrid>, mut commands: Commands| {
                grid.remove(&mut commands, kept);
            })
            .expect("remove");
        assert_eq!(marker_count(&mut world), 0);
    }

    #[test]
    fn food_visuals_skip_clean_ticks_and_run_after_a_mutation() {
        let cell = UVec2::new(6, 6);
        let mut app = App::new();
        app.init_resource::<FoodGrid>()
            .add_systems(Update, update_food_visuals);

        app.world_mut()
            .run_system_once(move |mut grid: ResMut<FoodGrid>, mut commands: Commands| {
                grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
            })
            .expect("set");

        app.update();

        let marker = {
            let world = app.world_mut();
            let mut query = world.query::<(Entity, &FoodMarker)>();
            query
                .iter(world)
                .map(|(entity, _)| entity)
                .next()
                .expect("one marker")
        };

        assert_eq!(
            app.world().get::<Sprite>(marker).expect("sprite").color,
            Color::srgba(0.2, 0.8, 0.2, 1.0)
        );

        // Tampering with the sprite proves whether the next tick rewrites it.
        app.world_mut().entity_mut(marker).insert(Sprite {
            color: Color::srgb(1.0, 0.0, 1.0),
            ..default()
        });
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(marker).expect("sprite").color,
            Color::srgb(1.0, 0.0, 1.0),
            "a clean tick must not touch sprites"
        );

        // A take is an observable mutation: alpha follows the remaining amount.
        app.world_mut()
            .resource_mut::<FoodGrid>()
            .take(cell, INITIAL_FOOD_AMOUNT / 2.0);
        app.update();
        assert_eq!(
            app.world().get::<Sprite>(marker).expect("sprite").color,
            Color::srgba(0.2, 0.8, 0.2, 0.5)
        );
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

            let stored = stored_cells(&grid);
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
                if let Some((_, distance)) = full_scan_nearest(&stored, center, radius) {
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

    /// Before/after harness for the exact early reject in
    /// [`FoodGrid::nearest_within_matching`], with the pre-change storage
    /// (BTreeMap amounts) as the true baseline. The `accept` closures mirror
    /// the work `sense_food` does (amount lookup, position, cone), which is
    /// the cost the early reject skips. Run with:
    /// `cargo test --release bench_matching_early_reject -- --ignored --nocapture`
    #[test]
    #[ignore = "measurement harness, run on demand"]
    fn bench_matching_early_reject() {
        const QUERIES: usize = 30_000;
        let mut rng = Rng::with_seed(0x5EED);

        for cells in [9usize, 625, 9_025] {
            let mut grid = FoodGrid::default();
            let mut world = World::new();
            let mut commands = world.commands();

            let side = (cells as f32).sqrt().ceil() as u32;
            let start_x = (GRID_WIDTH as u32 - side) / 2;
            let start_y = (GRID_HEIGHT as u32 - side) / 2;
            for i in 0..cells as u32 {
                let cell = UVec2::new(start_x + i % side, start_y + i / side);
                grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
            }

            // The old storage: `sense_food`'s accept closure looked amounts up
            // in a BTreeMap.
            let legacy: BTreeMap<u32, f32> = stored_cells(&grid)
                .into_iter()
                .map(|(cell, amount)| (pack(cell), amount))
                .collect();

            // Random query centers over the whole play area, with a random
            // ant position/direction so the cone work is representative.
            let queries: Vec<(UVec2, Vec2, f32)> = (0..QUERIES)
                .map(|_| {
                    let center = random_cell(&mut rng);
                    let pos = grid_to_world(center)
                        + Vec2::new(
                            rng.f32() * GRID_SIZE - GRID_SIZE / 2.0,
                            rng.f32() * GRID_SIZE - GRID_SIZE / 2.0,
                        );
                    (center, pos, rng.f32() * std::f32::consts::TAU)
                })
                .collect();

            let in_cone = |cell: UVec2, ant_pos: Vec2, direction: f32| -> bool {
                let food_pos = grid_to_world(cell);
                let offset = food_pos - ant_pos;
                let distance = offset.length();

                distance <= FOOD_SENSE_RANGE
                    && shortest_angle_diff(direction, offset.y.atan2(offset.x)).abs()
                        <= FOOD_SENSE_HALF_ANGLE
            };
            let legacy_sense = |cell: UVec2, ant_pos: Vec2, direction: f32| -> bool {
                legacy.get(&pack(cell)).is_some_and(|&amount| amount > 0.0)
                    && in_cone(cell, ant_pos, direction)
            };
            let dense_sense = |cell: UVec2, ant_pos: Vec2, direction: f32| -> bool {
                grid.amount(cell).is_some_and(|amount| amount > 0.0)
                    && in_cone(cell, ant_pos, direction)
            };

            // The host is shared, so report the best of a few rounds per
            // variant instead of a single load-inflated sample.
            const ROUNDS: usize = 3;
            let mut legacy_best = Duration::MAX;
            let mut legacy_new_best = Duration::MAX;
            let mut baseline_best = Duration::MAX;
            let mut new_best = Duration::MAX;
            let mut legacy_sum = 0.0f32;
            let mut legacy_new_sum = 0.0f32;
            let mut baseline_sum = 0.0f32;
            let mut new_sum = 0.0f32;

            for _ in 0..ROUNDS {
                let (elapsed, sum) = time_queries(&queries, |center, pos, direction| {
                    indexed_matching_without_early_reject(&grid, center, 5, |cell| {
                        legacy_sense(cell, pos, direction)
                    })
                });
                legacy_best = legacy_best.min(elapsed);
                legacy_sum = sum;

                let (elapsed, sum) = time_queries(&queries, |center, pos, direction| {
                    grid.nearest_within_matching(center, 5, |cell| {
                        legacy_sense(cell, pos, direction)
                    })
                });
                legacy_new_best = legacy_new_best.min(elapsed);
                legacy_new_sum = sum;

                let (elapsed, sum) = time_queries(&queries, |center, pos, direction| {
                    indexed_matching_without_early_reject(&grid, center, 5, |cell| {
                        dense_sense(cell, pos, direction)
                    })
                });
                baseline_best = baseline_best.min(elapsed);
                baseline_sum = sum;

                let (elapsed, sum) = time_queries(&queries, |center, pos, direction| {
                    grid.nearest_within_matching(center, 5, |cell| {
                        dense_sense(cell, pos, direction)
                    })
                });
                new_best = new_best.min(elapsed);
                new_sum = sum;
            }

            black_box(legacy_sum);
            black_box(legacy_new_sum);
            black_box(baseline_sum);
            black_box(new_sum);
            assert_eq!(legacy_sum, new_sum, "the early reject must be exact");
            assert_eq!(legacy_new_sum, new_sum, "the early reject must be exact");
            assert_eq!(baseline_sum, new_sum, "the early reject must be exact");

            println!(
                "{cells:5} food cells: BTreeMap+scan {:8.2} ms | BTreeMap+early-reject {:8.2} ms \
                 ({:4.1}x) | dense+scan {:8.2} ms | dense+early-reject {:8.2} ms | {:5.1}x vs before",
                legacy_best.as_secs_f64() * 1e3,
                legacy_new_best.as_secs_f64() * 1e3,
                legacy_best.as_secs_f64() / legacy_new_best.as_secs_f64().max(f64::MIN_POSITIVE),
                baseline_best.as_secs_f64() * 1e3,
                new_best.as_secs_f64() * 1e3,
                legacy_best.as_secs_f64() / new_best.as_secs_f64().max(f64::MIN_POSITIVE)
            );
        }
    }
}
