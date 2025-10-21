use crate::constants::*;
use crate::pheromone::PheromoneGrid;
use crate::resources::{FoodCells, FoodManagementState};
use bevy::prelude::*;

#[derive(Component)]
pub struct FoodManagementButton;

#[derive(Component)]
pub struct FoodMarker {
    pub grid_x: usize,
    pub grid_y: usize,
}

pub fn setup_food_button(mut commands: Commands) {
    commands
        .spawn((
            FoodManagementButton,
            Button,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(1.),
                top: Val::Percent(1.),
                padding: UiRect::all(Val::Px(8.0)),
                width: Val::Auto,
                height: Val::Auto,
                ..default()
            },
            BackgroundColor(Color::srgba(0.3, 0.3, 0.3, 0.8)),
            GlobalZIndex(i32::MAX),
        ))
        .with_child((
            Text::new("Food Mode"),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ));
}

pub fn toggle_food_management(
    button_query: Query<&Interaction, (Changed<Interaction>, With<FoodManagementButton>)>,
    mut food_state: ResMut<FoodManagementState>,
    mut button_bg: Query<&mut BackgroundColor, With<FoodManagementButton>>,
) {
    for interaction in &button_query {
        if *interaction == Interaction::Pressed {
            food_state.enabled = !food_state.enabled;

            for mut bg in &mut button_bg {
                bg.0 = if food_state.enabled {
                    Color::srgba(0.2, 0.8, 0.2, 0.9)
                } else {
                    Color::srgba(0.3, 0.3, 0.3, 0.8)
                };
            }
        }
    }
}

pub fn handle_food_clicks(
    mut commands: Commands,
    mouse_button: Res<ButtonInput<MouseButton>>,
    food_state: Res<FoodManagementState>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    pheromone_grid: Res<PheromoneGrid>,
    mut food_cells: ResMut<FoodCells>,
    food_markers: Query<(Entity, &FoodMarker)>,
) {
    if !food_state.enabled {
        return;
    }

    let window = windows.iter().next();
    let Some(window) = window else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    let camera_data = camera_query.iter().next();
    let Some((camera, camera_transform)) = camera_data else {
        return;
    };

    let world_pos = camera
        .viewport_to_world_2d(camera_transform, cursor_pos)
        .ok();
    let Some(world_pos) = world_pos else {
        return;
    };

    let Some((grid_x, grid_y)) = pheromone_grid.world_to_grid(world_pos) else {
        return;
    };

    if mouse_button.just_pressed(MouseButton::Left) && !food_cells.cells.contains(&(grid_x, grid_y))
    {
        food_cells.cells.push((grid_x, grid_y));

        let world_x = grid_x as f32 * GRID_SIZE - WINDOW_WIDTH as f32 / 2.0 + GRID_SIZE / 2.0;
        let world_y = grid_y as f32 * GRID_SIZE - WINDOW_HEIGHT as f32 / 2.0 + GRID_SIZE / 2.0;

        commands.spawn((
            FoodMarker { grid_x, grid_y },
            Sprite {
                color: Color::srgb(0.2, 0.8, 0.2),
                custom_size: Some(Vec2::new(GRID_SIZE, GRID_SIZE)),
                ..default()
            },
            Transform::from_xyz(world_x, world_y, 0.5),
        ));
    } else if mouse_button.just_pressed(MouseButton::Right) {
        if let Some(pos) = food_cells
            .cells
            .iter()
            .position(|&cell| cell == (grid_x, grid_y))
        {
            food_cells.cells.remove(pos);

            for (entity, marker) in &food_markers {
                if marker.grid_x == grid_x && marker.grid_y == grid_y {
                    commands.entity(entity).despawn();
                    break;
                }
            }
        }
    }
}
