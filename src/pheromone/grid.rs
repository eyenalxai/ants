//! Flat pheromone storage with half-life evaporation, diffusion and active-cell
//! tracking.

use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;

use crate::constants::pheromone::{
    PHEROMONE_DIFFUSION, PHEROMONE_HALF_LIFE_SECS, PHEROMONE_KERNEL_CENTER,
    PHEROMONE_KERNEL_DIAGONAL, PHEROMONE_KERNEL_ORTHOGONAL, PHEROMONE_MAX_INTENSITY,
    PHEROMONE_MIN_THRESHOLD, PHEROMONE_TO_NEST_HALF_LIFE_SECS,
};
use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};
use crate::core::grid::in_bounds;

/// Number of cells in the flat grid.
pub(crate) const CELL_COUNT: usize = GRID_WIDTH * GRID_HEIGHT;
/// Number of `u64` words needed for one activity bit per cell.
pub(crate) const ACTIVE_WORDS: usize = CELL_COUNT.div_ceil(64);
/// Upper bound for the per-step diffusion weight. Below `1 / 4` the update is a
/// convex combination of a cell and its neighbors, so it cannot oscillate.
const MAX_DIFFUSION_STEP: f32 = 0.2;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Pheromone {
    pub to_food: f32,
    pub to_nest: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PheromoneKind {
    ToFood,
    ToNest,
}

/// Grid of pheromone cells in flat storage, indexed by
/// `y * GRID_WIDTH + x` (see [`crate::core::grid::pack`]).
///
/// Diffusion reads from one buffer and writes into a second, so results never
/// depend on iteration order. A compact bitset tracks the non-zero cells: an
/// all-empty grid is left untouched by [`PheromoneGrid::step`], while a live
/// grid only visits active cells and their 4-neighborhood.
#[derive(Resource)]
pub struct PheromoneGrid {
    cells: Box<[Pheromone]>,
    scratch: Box<[Pheromone]>,
    active: Box<[u64]>,
    touched: Box<[u64]>,
    active_count: usize,
    version: u64,
}

impl Default for PheromoneGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl PheromoneGrid {
    pub fn new() -> Self {
        Self {
            cells: vec![Pheromone::default(); CELL_COUNT].into_boxed_slice(),
            scratch: vec![Pheromone::default(); CELL_COUNT].into_boxed_slice(),
            active: vec![0; ACTIVE_WORDS].into_boxed_slice(),
            touched: vec![0; ACTIVE_WORDS].into_boxed_slice(),
            active_count: 0,
            version: 0,
        }
    }

    fn index(cell: UVec2) -> Option<usize> {
        in_bounds(cell).then_some(cell.y as usize * GRID_WIDTH + cell.x as usize)
    }

    fn index_xy(x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= GRID_WIDTH as i32 || y >= GRID_HEIGHT as i32 {
            return None;
        }

        Some(y as usize * GRID_WIDTH + x as usize)
    }

    pub fn get(&self, cell: UVec2) -> Option<&Pheromone> {
        Self::index(cell).map(|index| &self.cells[index])
    }

    pub fn sample(&self, cell: UVec2, kind: PheromoneKind) -> f32 {
        match self.get(cell) {
            Some(pheromone) => match kind {
                PheromoneKind::ToFood => pheromone.to_food,
                PheromoneKind::ToNest => pheromone.to_nest,
            },
            None => 0.0,
        }
    }

    /// Add to a single cell, clamping each channel at
    /// [`PHEROMONE_MAX_INTENSITY`]. Test-only: production deposits go through
    /// [`PheromoneGrid::add_kernel`].
    #[cfg(test)]
    pub fn add(&mut self, cell: UVec2, to_food: f32, to_nest: f32) {
        if let Some(index) = Self::index(cell) {
            self.add_to_cell(index, to_food, to_nest);
            self.version = self.version.wrapping_add(1);
        }
    }

    /// Add through a mass-preserving 3x3 kernel: center 0.36, orthogonal 0.12,
    /// diagonal 0.04. Out-of-bounds kernel cells are skipped, so a deposit on
    /// the border loses the clipped mass.
    pub fn add_kernel(&mut self, cell: UVec2, to_food: f32, to_nest: f32) {
        let Some(index) = Self::index(cell) else {
            return;
        };

        let x = (index % GRID_WIDTH) as i32;
        let y = (index / GRID_WIDTH) as i32;

        for dy in -1..=1 {
            for dx in -1..=1 {
                if let Some(neighbor) = Self::index_xy(x + dx, y + dy) {
                    let weight = kernel_weight(dx, dy);
                    self.add_to_cell(neighbor, to_food * weight, to_nest * weight);
                }
            }
        }

        self.version = self.version.wrapping_add(1);
    }

    /// Evaporate for `dt` seconds and diffuse one cell outward.
    ///
    /// Retention is frame-rate independent and per channel:
    /// `0.5^(dt / half_life)`, so the `ToFood` recruitment trail evaporates
    /// faster than the `ToNest` home-range mark. Diffusion is a 5-point
    /// Laplacian with zero-flux borders, double-buffered so the result does not
    /// depend on iteration order. `version` only changes when a cell value
    /// actually changes, so an empty grid (or a step that cannot alter any
    /// cell) leaves it untouched and the overlay can skip its upload.
    pub fn step(&mut self, dt: f32) {
        if self.active_count == 0 || !dt.is_finite() || dt <= 0.0 {
            return;
        }

        let retention_to_food = retention_for(dt, PHEROMONE_HALF_LIFE_SECS);
        let retention_to_nest = retention_for(dt, PHEROMONE_TO_NEST_HALF_LIFE_SECS);
        let diffusion = (PHEROMONE_DIFFUSION * dt * 60.0).min(MAX_DIFFUSION_STEP);

        let Self {
            cells,
            scratch,
            active,
            touched,
            active_count,
            ..
        } = self;

        // Everything that can change this step: active cells plus their
        // orthogonal neighborhood, which is where diffusion reaches.
        for_each_set_bit(&active[..], |index| {
            set_bit(&mut touched[..], index);

            let x = index % GRID_WIDTH;
            let y = index / GRID_WIDTH;

            if x > 0 {
                set_bit(&mut touched[..], index - 1);
            }
            if x + 1 < GRID_WIDTH {
                set_bit(&mut touched[..], index + 1);
            }
            if y > 0 {
                set_bit(&mut touched[..], index - GRID_WIDTH);
            }
            if y + 1 < GRID_HEIGHT {
                set_bit(&mut touched[..], index + GRID_WIDTH);
            }
        });

        let mut changed = false;

        for_each_set_bit(&touched[..], |index| {
            let center = cells[index];
            let x = index % GRID_WIDTH;
            let y = index / GRID_WIDTH;

            let mut food_sum = 0.0;
            let mut nest_sum = 0.0;
            let mut degree = 0.0;

            if x > 0 {
                let neighbor = cells[index - 1];
                food_sum += neighbor.to_food;
                nest_sum += neighbor.to_nest;
                degree += 1.0;
            }
            if x + 1 < GRID_WIDTH {
                let neighbor = cells[index + 1];
                food_sum += neighbor.to_food;
                nest_sum += neighbor.to_nest;
                degree += 1.0;
            }
            if y > 0 {
                let neighbor = cells[index - GRID_WIDTH];
                food_sum += neighbor.to_food;
                nest_sum += neighbor.to_nest;
                degree += 1.0;
            }
            if y + 1 < GRID_HEIGHT {
                let neighbor = cells[index + GRID_WIDTH];
                food_sum += neighbor.to_food;
                nest_sum += neighbor.to_nest;
                degree += 1.0;
            }

            let next = Pheromone {
                to_food: diffuse(
                    center.to_food,
                    food_sum,
                    degree,
                    diffusion,
                    retention_to_food,
                ),
                to_nest: diffuse(
                    center.to_nest,
                    nest_sum,
                    degree,
                    diffusion,
                    retention_to_nest,
                ),
            };

            // Degenerate steps can leave a cell bit-identical (e.g. a zero
            // neighbor of an active cell); those must not dirty the overlay.
            if next != center {
                changed = true;
            }

            scratch[index] = next;
        });

        // Drop the old values so the spare buffer is zeroed again, then swap.
        for_each_set_bit(&active[..], |index| cells[index] = Pheromone::default());
        std::mem::swap(cells, scratch);

        active.fill(0);
        *active_count = 0;

        for_each_set_bit(&touched[..], |index| {
            if cells[index].to_food > 0.0 || cells[index].to_nest > 0.0 {
                set_bit(&mut active[..], index);
                *active_count += 1;
            }
        });

        touched.fill(0);

        if changed {
            self.version = self.version.wrapping_add(1);
        }
    }

    /// Zero every cell. Test-only: production code clears one channel with
    /// [`PheromoneGrid::clear_to_nest`].
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.cells.fill(Pheromone::default());
        self.scratch.fill(Pheromone::default());
        self.active.fill(0);
        self.touched.fill(0);
        self.active_count = 0;
        self.version = self.version.wrapping_add(1);
    }

    /// Zero only the `to_nest` channel.
    pub fn clear_to_nest(&mut self) {
        let mut active_count = 0;

        for (word_index, word) in self.active.iter_mut().enumerate() {
            let mut remaining = *word;
            let mut keep = 0u64;

            while remaining != 0 {
                let offset = remaining.trailing_zeros() as usize;
                let index = word_index * 64 + offset;
                let pheromone = &mut self.cells[index];

                pheromone.to_nest = 0.0;

                if pheromone.to_food > 0.0 {
                    keep |= 1u64 << offset;
                }

                remaining &= remaining - 1;
            }

            *word = keep;
            active_count += keep.count_ones() as usize;
        }

        self.active_count = active_count;
        self.version = self.version.wrapping_add(1);
    }

    /// Monotonically increasing counter bumped on every mutation.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Apply a dense per-cell deposit field through the 3x3 kernel.
    ///
    /// `band_bits` is caller-owned scratch of `bands * ACTIVE_WORDS` words;
    /// `bands` row bands are convolved in parallel and each band records its
    /// active cells in its own bitset. Every output cell is computed
    /// independently from the dense field (bands never race), and the bitsets
    /// are merged in fixed band order, so the result is bit-deterministic
    /// regardless of thread scheduling. Buffers are reused by the caller, so
    /// this allocates nothing per tick and `version` is bumped exactly once.
    ///
    /// The per-cell result matches applying [`PheromoneGrid::add_kernel`] once
    /// per source cell modulo float association: for non-negative deposits
    /// `clamp(clamp(x + a) + b) == clamp(x + a + b)`, so saturation and the
    /// active set are unchanged.
    pub fn apply_dense_deposits(
        &mut self,
        to_food: &[f32],
        to_nest: &[f32],
        band_bits: &mut [u64],
    ) {
        debug_assert_eq!(to_food.len(), CELL_COUNT);
        debug_assert_eq!(to_nest.len(), CELL_COUNT);
        debug_assert!(
            !band_bits.is_empty() && band_bits.len().is_multiple_of(ACTIVE_WORDS),
            "band scratch must hold whole bitsets"
        );

        let bands = (band_bits.len() / ACTIVE_WORDS).max(1);
        let rows_per = GRID_HEIGHT.div_ceil(bands);
        let Self {
            cells,
            active,
            active_count,
            version,
            ..
        } = self;

        // Bands that receive no rows (only possible when `bands > GRID_HEIGHT`)
        // must not merge stale bits.
        band_bits.fill(0);

        if let Some(pool) = ComputeTaskPool::try_get() {
            pool.scope(|scope| {
                for (band, (cell_chunk, bits)) in cells
                    .chunks_mut(rows_per * GRID_WIDTH)
                    .zip(band_bits.chunks_mut(ACTIVE_WORDS))
                    .enumerate()
                {
                    scope.spawn(async move {
                        apply_band(cell_chunk, bits, band * rows_per, to_food, to_nest);
                    });
                }
            });
        } else {
            // No compute pool (bare test worlds): the same bands run serially.
            for (band, (cell_chunk, bits)) in cells
                .chunks_mut(rows_per * GRID_WIDTH)
                .zip(band_bits.chunks_mut(ACTIVE_WORDS))
                .enumerate()
            {
                apply_band(cell_chunk, bits, band * rows_per, to_food, to_nest);
            }
        }

        for local in band_bits.chunks(ACTIVE_WORDS) {
            for (word, local_word) in active.iter_mut().zip(local.iter()) {
                let new_bits = *local_word & !*word;
                *active_count += new_bits.count_ones() as usize;
                *word |= *local_word;
            }
        }

        *version = version.wrapping_add(1);
    }

    /// Add into a known cell and keep the activity bitset in sync.
    fn add_to_cell(&mut self, index: usize, to_food: f32, to_nest: f32) {
        let pheromone = &mut self.cells[index];
        pheromone.to_food = sanitize(pheromone.to_food + to_food);
        pheromone.to_nest = sanitize(pheromone.to_nest + to_nest);

        if (pheromone.to_food > 0.0 || pheromone.to_nest > 0.0) && !bit_is_set(&self.active, index)
        {
            set_bit(&mut self.active, index);
            self.active_count += 1;
        }
    }
}

#[inline]
fn set_bit(bits: &mut [u64], index: usize) {
    bits[index / 64] |= 1u64 << (index % 64);
}

/// Convolve one row band of the dense deposit field into its cells and record
/// the active cells in the band's own bitset.
fn apply_band(
    cell_chunk: &mut [Pheromone],
    bits: &mut [u64],
    y0: usize,
    to_food: &[f32],
    to_nest: &[f32],
) {
    for (row, cells_row) in cell_chunk.chunks_mut(GRID_WIDTH).enumerate() {
        let y = y0 + row;

        for (x, cell) in cells_row.iter_mut().enumerate() {
            let mut food_sum = 0.0f32;
            let mut nest_sum = 0.0f32;

            for dy in -1i32..=1 {
                let ny = y as i32 + dy;
                if ny < 0 || ny >= GRID_HEIGHT as i32 {
                    continue;
                }

                let row_base = ny as usize * GRID_WIDTH;

                for dx in -1i32..=1 {
                    let nx = x as i32 + dx;
                    if nx < 0 || nx >= GRID_WIDTH as i32 {
                        continue;
                    }

                    let weight = kernel_weight(dx, dy);
                    let neighbor = row_base + nx as usize;
                    food_sum += to_food[neighbor] * weight;
                    nest_sum += to_nest[neighbor] * weight;
                }
            }

            if food_sum == 0.0 && nest_sum == 0.0 {
                continue;
            }

            cell.to_food = sanitize(cell.to_food + food_sum);
            cell.to_nest = sanitize(cell.to_nest + nest_sum);

            if cell.to_food > 0.0 || cell.to_nest > 0.0 {
                let index = y * GRID_WIDTH + x;
                bits[index / 64] |= 1u64 << (index % 64);
            }
        }
    }
}

#[inline]
fn bit_is_set(bits: &[u64], index: usize) -> bool {
    bits[index / 64] & (1u64 << (index % 64)) != 0
}

/// Visit every set bit as a flat cell index, cheapest words first.
fn for_each_set_bit(bits: &[u64], mut f: impl FnMut(usize)) {
    for (word_index, &word) in bits.iter().enumerate() {
        let mut word = word;

        while word != 0 {
            let offset = word.trailing_zeros() as usize;
            f(word_index * 64 + offset);
            word &= word - 1;
        }
    }
}

/// Map NaN to zero and clamp a channel into the valid intensity range.
#[inline]
fn sanitize(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, PHEROMONE_MAX_INTENSITY)
    }
}

/// Frame-rate independent retention for a half-life decay of `dt` seconds.
fn retention_for(dt: f32, half_life: f32) -> f32 {
    0.5f32.powf(dt / half_life)
}

/// Decay `dt` seconds and diffuse one channel from its neighbor sum.
///
/// Out-of-bounds neighbors contribute no flux (the ghost value equals the
/// center), which is compensated by only counting in-bounds `degree`.
fn diffuse(value: f32, neighbor_sum: f32, degree: f32, diffusion: f32, retention: f32) -> f32 {
    let value = sanitize((value + diffusion * (neighbor_sum - degree * value)) * retention);

    if value < PHEROMONE_MIN_THRESHOLD {
        0.0
    } else {
        value
    }
}

/// Weight of offset `(dx, dy)` in the mass-preserving 3x3 deposit kernel.
fn kernel_weight(dx: i32, dy: i32) -> f32 {
    match (dx == 0, dy == 0) {
        (true, true) => PHEROMONE_KERNEL_CENTER,
        (true, false) | (false, true) => PHEROMONE_KERNEL_ORTHOGONAL,
        (false, false) => PHEROMONE_KERNEL_DIAGONAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_total(grid: &PheromoneGrid, kind: PheromoneKind) -> f32 {
        let mut total = 0.0;

        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                total += grid.sample(UVec2::new(x, y), kind);
            }
        }

        total
    }

    fn grid_max(grid: &PheromoneGrid) -> f32 {
        let mut max = 0.0f32;

        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                let pheromone = grid.get(UVec2::new(x, y)).unwrap();

                assert!(pheromone.to_food.is_finite() && pheromone.to_food >= 0.0);
                assert!(pheromone.to_nest.is_finite() && pheromone.to_nest >= 0.0);

                max = max.max(pheromone.to_food).max(pheromone.to_nest);
            }
        }

        max
    }

    #[test]
    fn add_is_an_exact_single_cell() {
        let mut grid = PheromoneGrid::new();
        let cell = UVec2::new(7, 8);

        grid.add(cell, 1.5, 2.5);

        assert_eq!(grid.sample(cell, PheromoneKind::ToFood), 1.5);
        assert_eq!(grid.sample(cell, PheromoneKind::ToNest), 2.5);
        assert_eq!(grid.sample(UVec2::new(8, 8), PheromoneKind::ToFood), 0.0);
    }

    #[test]
    fn kernel_preserves_mass_and_clips_at_borders() {
        let mut grid = PheromoneGrid::new();
        let center = UVec2::new(30, 20);

        grid.add_kernel(center, 1.0, 2.0);

        assert!((grid_total(&grid, PheromoneKind::ToFood) - 1.0).abs() < 1e-5);
        assert!((grid_total(&grid, PheromoneKind::ToNest) - 2.0).abs() < 1e-5);
        assert!((grid.sample(center, PheromoneKind::ToFood) - 0.36).abs() < 1e-6);
        assert!((grid.sample(UVec2::new(31, 20), PheromoneKind::ToFood) - 0.12).abs() < 1e-6);
        assert!((grid.sample(UVec2::new(31, 21), PheromoneKind::ToFood) - 0.04).abs() < 1e-6);

        // A corner deposit drops the clipped mass but stays in bounds.
        let mut corner = PheromoneGrid::new();
        corner.add_kernel(UVec2::ZERO, 1.0, 0.0);
        let corner_total = grid_total(&corner, PheromoneKind::ToFood);
        assert!(corner_total > 0.0 && corner_total < 1.0);
    }

    #[test]
    fn add_and_kernel_clamp_at_max_intensity() {
        let mut grid = PheromoneGrid::new();
        let cell = UVec2::new(10, 10);

        grid.add(cell, PHEROMONE_MAX_INTENSITY, PHEROMONE_MAX_INTENSITY);
        grid.add(cell, PHEROMONE_MAX_INTENSITY, PHEROMONE_MAX_INTENSITY);
        grid.add_kernel(cell, PHEROMONE_MAX_INTENSITY, PHEROMONE_MAX_INTENSITY);

        for dy in -1..=1 {
            for dx in -1..=1 {
                let neighbor = UVec2::new((cell.x as i32 + dx) as u32, (cell.y as i32 + dy) as u32);
                let pheromone = grid.get(neighbor).unwrap();

                assert!(pheromone.to_food <= PHEROMONE_MAX_INTENSITY);
                assert!(pheromone.to_nest <= PHEROMONE_MAX_INTENSITY);
            }
        }

        assert!((grid.sample(cell, PheromoneKind::ToFood) - PHEROMONE_MAX_INTENSITY).abs() < 1e-3);
    }

    #[test]
    fn step_diffuses_and_stays_stable() {
        let mut grid = PheromoneGrid::new();
        let center = UVec2::new(100, 75);

        grid.add(center, 100.0, 50.0);

        let mut previous_max = grid_max(&grid);
        assert!(previous_max > 0.0);

        for _ in 0..120 {
            grid.step(1.0 / 60.0);

            let current_max = grid_max(&grid);

            // A convex update with retention <= 1 can never exceed the
            // previous maximum.
            assert!(current_max <= previous_max + 1e-3);
            previous_max = current_max;
        }

        // Diffusion actually spread mass into the orthogonal neighborhood.
        assert!(grid.sample(UVec2::new(center.x + 1, center.y), PheromoneKind::ToFood) > 0.0);
        assert!(grid.sample(UVec2::new(center.x, center.y + 1), PheromoneKind::ToNest) > 0.0);
    }

    #[test]
    fn half_life_step_halves_isolated_active_cell() {
        let mut grid = PheromoneGrid::new();
        grid.add(UVec2::new(100, 75), 1.0, 1.0);

        let before = grid_total(&grid, PheromoneKind::ToFood);
        grid.step(PHEROMONE_HALF_LIFE_SECS);
        let after = grid_total(&grid, PheromoneKind::ToFood);

        // Diffusion is interior mass-neutral; only retention changes the sum.
        assert!((after - before * 0.5).abs() < 1e-4);
        assert!(
            (retention_for(PHEROMONE_HALF_LIFE_SECS, PHEROMONE_HALF_LIFE_SECS) - 0.5).abs() < 1e-6
        );
        // The home-range mark evaporates slower than the recruitment trail.
        assert!(grid_total(&grid, PheromoneKind::ToNest) > after);
    }

    /// Mass-normalized RMS radius in cells (one cell = 4 u), measured from the
    /// centre of mass. This is the spread metric the realism audit used.
    fn rms_radius(grid: &PheromoneGrid, kind: PheromoneKind) -> f32 {
        let mut mass = 0.0f32;
        let mut wx = 0.0f32;
        let mut wy = 0.0f32;

        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                let value = grid.sample(UVec2::new(x, y), kind);
                mass += value;
                wx += value * x as f32;
                wy += value * y as f32;
            }
        }

        if mass <= 0.0 {
            return 0.0;
        }

        let cx = wx / mass;
        let cy = wy / mass;
        let mut variance = 0.0f32;

        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                let value = grid.sample(UVec2::new(x, y), kind);
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                variance += value * (dx * dx + dy * dy);
            }
        }

        (variance / mass).sqrt()
    }

    /// F1: the low diffusion constant must keep a kernel deposit a *trail*
    /// (a couple of cells wide) instead of a cloud that spans the map.
    #[test]
    fn kernel_deposit_rms_radius_stays_within_two_cells_over_a_half_life() {
        let mut grid = PheromoneGrid::new();
        let center = UVec2::new(GRID_WIDTH as u32 / 2, GRID_HEIGHT as u32 / 2);
        grid.add_kernel(center, 1.0, 0.0);

        let initial = rms_radius(&grid, PheromoneKind::ToFood);
        assert!(initial < 1.3, "kernel RMS radius {initial} cells");

        for _ in 0..(PHEROMONE_HALF_LIFE_SECS * 64.0) as u32 {
            grid.step(1.0 / 64.0);
        }

        let rms = rms_radius(&grid, PheromoneKind::ToFood);
        assert!(
            rms <= 2.0,
            "RMS radius {rms} cells after one {PHEROMONE_HALF_LIFE_SECS}s half-life"
        );
    }

    /// F1: the promoted snap threshold must not erase an isolated deposit.
    /// The old `0.01` threshold zeroed a single pass after 0.375 s.
    #[test]
    fn isolated_single_pass_deposit_survives_ten_seconds() {
        let mut grid = PheromoneGrid::new();
        let center = UVec2::new(GRID_WIDTH as u32 / 2, GRID_HEIGHT as u32 / 2);
        // A single forager pass peaks near 0.04 raw; see constants/sensor.rs.
        grid.add_kernel(center, 0.04, 0.0);

        for _ in 0..(PHEROMONE_HALF_LIFE_SECS * 64.0) as u32 {
            grid.step(1.0 / 64.0);
        }

        assert!(
            grid.sample(center, PheromoneKind::ToFood) > 0.0,
            "the deposit centre must survive the snap threshold"
        );
        assert!(grid_total(&grid, PheromoneKind::ToFood) > 0.0);
    }

    /// The dense deposit path must equal one kernel application per source
    /// cell, independent of how many row bands are used.
    #[test]
    fn dense_apply_matches_kernel_applications_and_band_count() {
        let mut dense_food = vec![0.0f32; CELL_COUNT];
        let mut dense_nest = vec![0.0f32; CELL_COUNT];
        let mut expected = PheromoneGrid::new();

        // Two clusters plus a border deposit so clipping is covered.
        for (cell, food, nest) in [
            (UVec2::new(50, 40), 0.3f32, 0.1f32),
            (UVec2::new(51, 41), 0.05, 0.2),
            (UVec2::new(0, 0), 0.4, 0.0),
            (UVec2::new(GRID_WIDTH as u32 - 1, 7), 0.0, 0.7),
        ] {
            let index = cell.y as usize * GRID_WIDTH + cell.x as usize;
            dense_food[index] += food;
            dense_nest[index] += nest;
            expected.add_kernel(cell, food, nest);
        }

        let mut one_band = PheromoneGrid::new();
        one_band.apply_dense_deposits(&dense_food, &dense_nest, &mut vec![0u64; ACTIVE_WORDS]);

        let mut seven_bands = PheromoneGrid::new();
        seven_bands.apply_dense_deposits(
            &dense_food,
            &dense_nest,
            &mut vec![0u64; 7 * ACTIVE_WORDS],
        );

        assert_eq!(one_band.version(), 1);
        assert_eq!(seven_bands.version(), 1);

        for y in 0..GRID_HEIGHT as u32 {
            for x in 0..GRID_WIDTH as u32 {
                let cell = UVec2::new(x, y);
                let want = expected.get(cell).copied().unwrap_or_default();
                let got = one_band.get(cell).copied().unwrap_or_default();
                let seven = seven_bands.get(cell).copied().unwrap_or_default();

                assert_eq!(got, seven, "band count changed cell {cell:?}");
                assert!(
                    (got.to_food - want.to_food).abs() < 1e-6
                        && (got.to_nest - want.to_nest).abs() < 1e-6,
                    "cell {cell:?}: got {got:?}, want {want:?}"
                );
            }
        }

        // The merged bitsets must count exactly the non-zero cells.
        let populated = (0..GRID_HEIGHT as u32)
            .flat_map(|y| (0..GRID_WIDTH as u32).map(move |x| UVec2::new(x, y)))
            .filter(|cell| {
                let value = one_band.get(*cell).unwrap();
                value.to_food > 0.0 || value.to_nest > 0.0
            })
            .count();
        assert_eq!(one_band.active_count, populated);
        assert_eq!(seven_bands.active_count, populated);
    }

    #[test]
    fn active_set_tracks_deposits_and_empty_steps() {
        let mut grid = PheromoneGrid::new();

        // An empty grid step is a no-op and leaves the version unchanged.
        let version = grid.version();
        grid.step(1.0 / 60.0);
        assert_eq!(grid.version(), version);
        assert_eq!(grid.active_count, 0);

        // A deposit far from the origin activates its cell ...
        let far = UVec2::new(150, 100);
        grid.add(far, 4.0, 0.0);
        assert_eq!(grid.active_count, 1);
        let version = grid.version();

        let before = grid.sample(far, PheromoneKind::ToFood);
        grid.step(1.0 / 60.0);
        let after = grid.sample(far, PheromoneKind::ToFood);

        assert!(
            after < before,
            "active cell should decay: {after} !< {before}"
        );
        // A real step is a mutation.
        assert!(grid.version() > version);
        // ... and the step activates the orthogonal neighborhood too.
        assert!(grid.active_count >= 5);

        // Once every value falls under the snap threshold the set empties.
        for _ in 0..200 {
            grid.step(1.0);
        }
        assert_eq!(grid.active_count, 0);
        assert_eq!(grid_total(&grid, PheromoneKind::ToFood), 0.0);

        // Back to empty: further steps are no-ops again.
        let version = grid.version();
        grid.step(1.0 / 60.0);
        assert_eq!(grid.version(), version);
    }

    #[test]
    fn step_only_bumps_the_version_when_a_cell_changes() {
        let mut grid = PheromoneGrid::new();
        let cell = UVec2::new(40, 30);

        // Empty grid: no step (valid or not) may bump the version.
        let version = grid.version();
        grid.step(1.0 / 64.0);
        grid.step(0.0);
        grid.step(-1.0);
        grid.step(f32::NAN);
        grid.step(f32::INFINITY);
        assert_eq!(grid.version(), version);

        // A populated grid changes on a real step.
        grid.add(cell, 1.0, 1.0);
        let version = grid.version();
        grid.step(1.0 / 64.0);
        assert!(grid.version() > version);

        // Non-positive or non-finite `dt` leaves live content untouched.
        let before = *grid.get(cell).unwrap();
        let version = grid.version();
        grid.step(0.0);
        grid.step(-0.5);
        grid.step(f32::NAN);
        assert_eq!(grid.version(), version);
        assert_eq!(*grid.get(cell).unwrap(), before);
    }

    #[test]
    fn version_increments_on_every_mutation() {
        let mut grid = PheromoneGrid::new();
        let cell = UVec2::new(3, 3);
        let mut version = grid.version();

        grid.add(cell, 1.0, 0.0);
        assert!(grid.version() > version);
        version = grid.version();

        grid.add_kernel(cell, 0.0, 1.0);
        assert!(grid.version() > version);
        version = grid.version();

        grid.step(1.0 / 60.0);
        assert!(grid.version() > version);
        version = grid.version();

        grid.clear_to_nest();
        assert!(grid.version() > version);
        version = grid.version();

        grid.clear();
        assert!(grid.version() > version);
    }
}
