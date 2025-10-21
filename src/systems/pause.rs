use crate::resources::PauseState;
use bevy::prelude::*;

#[derive(Component)]
pub struct PauseButton;

pub fn setup_pause_button(mut commands: Commands) {
    commands
        .spawn((
            PauseButton,
            Button,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(1.),
                top: Val::Percent(1.),
                padding: UiRect::all(Val::Px(8.0)),
                width: Val::Auto,
                height: Val::Auto,
                margin: UiRect {
                    right: Val::Px(8.0),
                    ..default()
                },
                ..default()
            },
            BackgroundColor(Color::srgba(0.3, 0.3, 0.3, 0.8)),
            GlobalZIndex(i32::MAX),
        ))
        .with_child((
            Text::new("Pause"),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ));
}

pub fn toggle_pause(
    button_query: Query<&Interaction, (Changed<Interaction>, With<PauseButton>)>,
    mut pause_state: ResMut<PauseState>,
    mut button_bg: Query<&mut BackgroundColor, With<PauseButton>>,
) {
    for interaction in &button_query {
        if *interaction == Interaction::Pressed {
            pause_state.paused = !pause_state.paused;

            for mut bg in &mut button_bg {
                bg.0 = if pause_state.paused {
                    Color::srgba(0.8, 0.2, 0.2, 0.9)
                } else {
                    Color::srgba(0.3, 0.3, 0.3, 0.8)
                };
            }
        }
    }
}

