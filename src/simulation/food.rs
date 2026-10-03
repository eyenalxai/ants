//! Food storage: single source of truth for food amounts and their markers.

use bevy::prelude::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::constants::world::{FOOD_CELL_RADIUS, FOOD_X, FOOD_Y, GRID_SIZE, INITIAL_FOOD_AMOUNT};
use crate::core::grid::{grid_to_world, in_bounds, pack, unpack, world_to_grid};
use crate::core::layers::Z_FOOD;

/// Marker entity for one food cell.
#[derive(Component)]
pub struct FoodMarker {
    pub cell: UVec2,
}

/// Amounts and marker entities for every food cell in the world.
///
/// Amounts live in a `BTreeMap` so nearest-cell scans iterate in a stable
/// order; tie-breaking must not depend on hash-map randomization.
#[derive(Resource, Default)]
pub struct FoodGrid {
    amounts: BTreeMap<u32, f32>,
    markers: HashMap<u32, Entity>,
}

impl FoodGrid {
    pub fn amount(&self, cell: UVec2) -> Option<f32> {
        self.amounts.get(&pack(cell)).copied()
    }

    pub fn contains(&self, cell: UVec2) -> bool {
        self.amounts.contains_key(&pack(cell))
    }

    pub fn iter(&self) -> impl Iterator<Item = (UVec2, f32)> + '_ {
        self.amounts
            .iter()
            .map(|(&key, &amount)| (unpack(key), amount))
    }

    /// Insert or update `cell`, ensuring a marker entity exists for it.
    pub fn set(&mut self, commands: &mut Commands, cell: UVec2, amount: f32) {
        let key = pack(cell);
        self.amounts.insert(key, amount);

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
    /// taken.
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
        if self.amounts.remove(&key).is_some()
            && let Some(entity) = self.markers.remove(&key)
        {
            commands.entity(entity).despawn();
        }
    }

    pub fn len(&self) -> usize {
        self.amounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.amounts.is_empty()
    }

    /// Nearest food cell within `radius_cells` of `center`, returned with the
    /// distance between the two cell centers in world units.
    pub fn nearest_within(&self, center: UVec2, radius_cells: i32) -> Option<(UVec2, f32)> {
        let center_pos = grid_to_world(center);
        let max_distance = radius_cells as f32 * GRID_SIZE;
        let mut nearest: Option<(UVec2, f32)> = None;

        for &key in self.amounts.keys() {
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

    /// Cells with a non-positive amount, for a removal pass.
    pub fn depleted(&self) -> impl Iterator<Item = UVec2> {
        self.amounts
            .iter()
            .filter(|&(_, &amount)| amount <= 0.0)
            .map(|(&key, _)| unpack(key))
    }
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
    let depleted: Vec<UVec2> = food_grid.depleted().collect();
    for cell in depleted {
        food_grid.remove(&mut commands, cell);
    }
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
