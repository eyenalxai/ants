//! Flat pheromone storage.

use bevy::prelude::*;

use crate::constants::pheromone::{PHEROMONE_DECAY_RATE, PHEROMONE_MAX_INTENSITY};
use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};
use crate::core::grid::in_bounds;

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
#[derive(Resource)]
pub struct PheromoneGrid {
    cells: Box<[Pheromone]>,
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
            cells: vec![Pheromone::default(); GRID_WIDTH * GRID_HEIGHT].into_boxed_slice(),
            version: 0,
        }
    }

    fn index(cell: UVec2) -> Option<usize> {
        in_bounds(cell).then_some(cell.y as usize * GRID_WIDTH + cell.x as usize)
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

    /// Add to a cell, clamping each channel at [`PHEROMONE_MAX_INTENSITY`].
    pub fn add(&mut self, cell: UVec2, to_food: f32, to_nest: f32) {
        if let Some(index) = Self::index(cell) {
            let pheromone = &mut self.cells[index];
            pheromone.to_food = (pheromone.to_food + to_food).min(PHEROMONE_MAX_INTENSITY);
            pheromone.to_nest = (pheromone.to_nest + to_nest).min(PHEROMONE_MAX_INTENSITY);
            self.version = self.version.wrapping_add(1);
        }
    }

    /// Spreading extension point. Currently identical to [`Self::add`].
    pub fn add_kernel(&mut self, cell: UVec2, to_food: f32, to_nest: f32) {
        self.add(cell, to_food, to_nest);
    }

    /// Apply the current exponential decay behavior for `dt` seconds.
    ///
    /// Frame independent: `PHEROMONE_DECAY_RATE^(dt * 60)`. Kept free of any
    /// diffusion so that can be layered on here later.
    pub fn step(&mut self, dt: f32) {
        let decay_factor = PHEROMONE_DECAY_RATE.powf(dt * 60.0);
        const THRESHOLD: f32 = 0.01;

        for pheromone in self.cells.iter_mut() {
            if pheromone.to_food > THRESHOLD {
                pheromone.to_food *= decay_factor;
                if pheromone.to_food < THRESHOLD {
                    pheromone.to_food = 0.0;
                }
            }
            if pheromone.to_nest > THRESHOLD {
                pheromone.to_nest *= decay_factor;
                if pheromone.to_nest < THRESHOLD {
                    pheromone.to_nest = 0.0;
                }
            }
        }

        self.version = self.version.wrapping_add(1);
    }

    pub fn clear(&mut self) {
        self.cells.fill(Pheromone::default());
        self.version = self.version.wrapping_add(1);
    }

    /// Zero only the `to_nest` channel.
    pub fn clear_to_nest(&mut self) {
        for pheromone in self.cells.iter_mut() {
            pheromone.to_nest = 0.0;
        }
        self.version = self.version.wrapping_add(1);
    }

    /// Monotonically increasing counter bumped on every mutation.
    pub fn version(&self) -> u64 {
        self.version
    }
}
