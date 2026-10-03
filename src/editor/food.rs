//! Food brush tool: paint and erase food cells with the pointer.

use bevy::prelude::*;

use crate::constants::ui::FOOD_BRUSH_DIAMETER;
use crate::constants::world::INITIAL_FOOD_AMOUNT;
use crate::core::grid::{in_bounds, world_to_grid};
use crate::editor::cursor::PointerInput;
use crate::simulation::food::FoodGrid;

/// Cells covered by the square food brush anchored at `origin` (top-left
/// corner), clipped to the grid.
pub fn brush_cells(origin: UVec2) -> impl Iterator<Item = UVec2> + Clone {
    (0..FOOD_BRUSH_DIAMETER as u32).flat_map(move |dy| {
        (0..FOOD_BRUSH_DIAMETER as u32).filter_map(move |dx| {
            let cell = origin + UVec2::new(dx, dy);
            in_bounds(cell).then_some(cell)
        })
    })
}

/// Cell offset from the brush origin to the cell at the center of the brush.
pub fn brush_center_offset() -> UVec2 {
    UVec2::splat((FOOD_BRUSH_DIAMETER as u32).saturating_sub(1) / 2)
}

/// Left click paints a 3×3 food patch, right click removes it. Runs only while
/// in food mode (see the plugin run condition); clicks over UI are ignored.
pub fn handle_food_clicks(
    mut commands: Commands,
    pointer: PointerInput,
    mut food_grid: ResMut<FoodGrid>,
) {
    if pointer.over_ui() {
        return;
    }

    let Some(world_pos) = pointer.world_pos() else {
        return;
    };
    let Some(origin) = world_to_grid(world_pos) else {
        return;
    };

    if pointer.left_pressed() {
        for cell in brush_cells(origin) {
            if !food_grid.contains(cell) {
                food_grid.set(&mut commands, cell, INITIAL_FOOD_AMOUNT);
            }
        }
    } else if pointer.right_pressed() {
        for cell in brush_cells(origin) {
            food_grid.remove(&mut commands, cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH};
    use crate::editor::cursor::test_support::{
        TEST_WINDOW_SIZE, pointer_world, press_mouse, window_to_world,
    };
    use bevy::ecs::system::RunSystemOnce;

    /// A world with the food grid and the pointer fixtures; `ui` spawns a
    /// pressed UI widget so the click lands over UI.
    fn click_world(ui: bool) -> World {
        let mut world = pointer_world(TEST_WINDOW_SIZE / 2.0);
        world.init_resource::<FoodGrid>();

        if ui {
            world.spawn(Interaction::Pressed);
        }

        world
    }

    fn run_clicks(world: &mut World) {
        world.run_system_once(handle_food_clicks).unwrap();
    }

    #[test]
    fn brush_covers_a_square_in_the_interior() {
        let origin = UVec2::new(5, 5);
        let cells: Vec<UVec2> = brush_cells(origin).collect();

        assert_eq!(
            cells.len(),
            (FOOD_BRUSH_DIAMETER * FOOD_BRUSH_DIAMETER) as usize
        );
        assert!(cells.contains(&origin));
        assert!(cells.contains(&(origin + UVec2::splat(2))));
        assert_eq!(brush_center_offset(), UVec2::splat(1));
    }

    #[test]
    fn brush_clips_to_grid_bounds() {
        let last = UVec2::new(GRID_WIDTH as u32 - 1, GRID_HEIGHT as u32 - 1);
        assert_eq!(brush_cells(last).collect::<Vec<_>>(), vec![last]);

        // A brush hanging over the right edge keeps only its in-bounds column.
        let edge = UVec2::new(GRID_WIDTH as u32 - 1, 0);
        let cells: Vec<UVec2> = brush_cells(edge).collect();
        assert_eq!(cells.len(), FOOD_BRUSH_DIAMETER as usize);
        assert!(
            cells
                .iter()
                .all(|cell| in_bounds(*cell) && cell.x == edge.x)
        );
    }

    #[test]
    fn food_clicks_paint_and_erase_the_brush_cells() {
        let mut world = click_world(false);
        let center = window_to_world(TEST_WINDOW_SIZE / 2.0);
        let origin = world_to_grid(center).expect("window center is inside the grid");
        let painted: Vec<UVec2> = brush_cells(origin).collect();

        press_mouse(&mut world, MouseButton::Left);
        run_clicks(&mut world);

        {
            let food = world.resource::<FoodGrid>();

            for cell in &painted {
                assert!(food.contains(*cell), "left click paints {cell:?}");
                assert_eq!(food.amount(*cell), Some(INITIAL_FOOD_AMOUNT));
            }
        }

        press_mouse(&mut world, MouseButton::Right);
        run_clicks(&mut world);

        let food = world.resource::<FoodGrid>();

        for cell in &painted {
            assert!(!food.contains(*cell), "right click erases {cell:?}");
        }
    }

    #[test]
    fn food_clicks_over_ui_are_ignored() {
        let mut world = click_world(true);

        press_mouse(&mut world, MouseButton::Left);
        run_clicks(&mut world);

        assert_eq!(world.resource::<FoodGrid>().iter().count(), 0);
    }
}
