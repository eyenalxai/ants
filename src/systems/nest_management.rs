use crate::components::Nest;
use crate::resources::NestManagementState;
use bevy::prelude::*;

#[derive(Component)]
pub struct NestManagementButton;

#[derive(Component)]
pub struct NestCursorMarker;

pub fn setup_nest_button(mut commands: Commands) {
    commands
        .spawn((
            NestManagementButton,
            Button,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(210.),
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
            Text::new("Nest Mode"),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ));
}

pub fn toggle_nest_management(
    button_query: Query<&Interaction, (Changed<Interaction>, With<NestManagementButton>)>,
    mut nest_state: ResMut<NestManagementState>,
    mut button_bg: Query<&mut BackgroundColor, With<NestManagementButton>>,
) {
    for interaction in &button_query {
        if *interaction == Interaction::Pressed {
            nest_state.enabled = !nest_state.enabled;
            nest_state.dragging = false;

            for mut bg in &mut button_bg {
                bg.0 = if nest_state.enabled {
                    Color::srgba(0.8, 0.2, 0.2, 0.9)
                } else {
                    Color::srgba(0.3, 0.3, 0.3, 0.8)
                };
            }
        }
    }
}

pub fn handle_nest_drag(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut nest_state: ResMut<NestManagementState>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut nest_query: Query<&mut Transform, With<Nest>>,
    ui_query: Query<&Interaction>,
) {
    if !nest_state.enabled {
        return;
    }

    for interaction in &ui_query {
        if *interaction != Interaction::None {
            return;
        }
    }

    let window = windows.iter().next();
    let Some(window) = window else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        nest_state.dragging = false;
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

    if mouse_button.just_pressed(MouseButton::Left) {
        nest_state.dragging = true;
    }

    if mouse_button.just_released(MouseButton::Left) {
        nest_state.dragging = false;
    }

    if nest_state.dragging
        && mouse_button.pressed(MouseButton::Left)
        && let Ok(mut nest_transform) = nest_query.single_mut()
    {
        nest_transform.translation.x = world_pos.x;
        nest_transform.translation.y = world_pos.y;
    }
}

pub fn update_nest_cursor(
    mut commands: Commands,
    nest_state: Res<NestManagementState>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut cursor_query: Query<(&mut Transform, &mut Sprite, &mut Visibility), With<NestCursorMarker>>,
    ui_query: Query<&Interaction>,
) {
    if !nest_state.enabled {
        for (_, _, mut vis) in &mut cursor_query {
            *vis = Visibility::Hidden;
        }
        return;
    }

    for interaction in &ui_query {
        if *interaction != Interaction::None {
            for (_, _, mut vis) in &mut cursor_query {
                *vis = Visibility::Hidden;
            }
            return;
        }
    }

    let window = windows.iter().next();
    let Some(window) = window else {
        for (_, _, mut vis) in &mut cursor_query {
            *vis = Visibility::Hidden;
        }
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        for (_, _, mut vis) in &mut cursor_query {
            *vis = Visibility::Hidden;
        }
        return;
    };

    let camera_data = camera_query.iter().next();
    let Some((camera, camera_transform)) = camera_data else {
        for (_, _, mut vis) in &mut cursor_query {
            *vis = Visibility::Hidden;
        }
        return;
    };

    let world_pos = camera
        .viewport_to_world_2d(camera_transform, cursor_pos)
        .ok();
    let Some(world_pos) = world_pos else {
        for (_, _, mut vis) in &mut cursor_query {
            *vis = Visibility::Hidden;
        }
        return;
    };

    let cursor_color = if nest_state.dragging {
        Color::srgba(1.0, 0.0, 0.0, 0.7)
    } else {
        Color::srgba(1.0, 0.0, 0.0, 0.4)
    };

    if let Ok((mut t, mut sprite, mut vis)) = cursor_query.single_mut() {
        t.translation.x = world_pos.x;
        t.translation.y = world_pos.y;
        sprite.color = cursor_color;
        *vis = Visibility::Visible;
    } else {
        commands.spawn((
            NestCursorMarker,
            Sprite {
                color: cursor_color,
                custom_size: Some(Vec2::new(20.0, 20.0)),
                ..default()
            },
            Transform::from_xyz(world_pos.x, world_pos.y, 0.6),
        ));
    }
}
