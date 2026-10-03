use bevy::prelude::*;

use crate::constants::ui::{
    UI_BUTTON_PADDING, UI_EDGE_INSET_PERCENT, UI_FONT_SIZE, UI_FOOD_BUTTON_LEFT_PX,
    UI_NEST_BUTTON_LEFT_PX,
};
use crate::core::sets::Paused;
use crate::editor::{EditorMode, EditorModeKind};

const BUTTON_IDLE: Color = Color::srgba(0.3, 0.3, 0.3, 0.8);
const BUTTON_PAUSE_ACTIVE: Color = Color::srgba(0.8, 0.2, 0.2, 0.9);
const BUTTON_FOOD_ACTIVE: Color = Color::srgba(0.2, 0.8, 0.2, 0.9);
const BUTTON_NEST_ACTIVE: Color = Color::srgba(0.8, 0.2, 0.2, 0.9);

#[derive(Component)]
pub struct PauseButton;

#[derive(Component)]
pub struct FoodManagementButton;

#[derive(Component)]
pub struct NestManagementButton;

pub fn setup_pause_button(mut commands: Commands) {
    commands
        .spawn((
            PauseButton,
            Button,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(UI_EDGE_INSET_PERCENT),
                top: Val::Percent(UI_EDGE_INSET_PERCENT),
                padding: UiRect::all(Val::Px(UI_BUTTON_PADDING)),
                width: Val::Auto,
                height: Val::Auto,
                margin: UiRect {
                    right: Val::Px(UI_BUTTON_PADDING),
                    ..default()
                },
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
            GlobalZIndex(i32::MAX),
        ))
        .with_child((
            Text::new("Pause"),
            TextFont {
                font_size: FontSize::Px(UI_FONT_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ));
}

pub fn setup_food_button(mut commands: Commands) {
    commands
        .spawn((
            FoodManagementButton,
            Button,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(UI_FOOD_BUTTON_LEFT_PX),
                top: Val::Percent(UI_EDGE_INSET_PERCENT),
                padding: UiRect::all(Val::Px(UI_BUTTON_PADDING)),
                width: Val::Auto,
                height: Val::Auto,
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
            GlobalZIndex(i32::MAX),
        ))
        .with_child((
            Text::new("Food Mode"),
            TextFont {
                font_size: FontSize::Px(UI_FONT_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ));
}

pub fn setup_nest_button(mut commands: Commands) {
    commands
        .spawn((
            NestManagementButton,
            Button,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(UI_NEST_BUTTON_LEFT_PX),
                top: Val::Percent(UI_EDGE_INSET_PERCENT),
                padding: UiRect::all(Val::Px(UI_BUTTON_PADDING)),
                width: Val::Auto,
                height: Val::Auto,
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
            GlobalZIndex(i32::MAX),
        ))
        .with_child((
            Text::new("Nest Mode"),
            TextFont {
                font_size: FontSize::Px(UI_FONT_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ));
}

/// Pause button toggles the virtual clock (freezing fixed stepping) and the
/// `Paused` mirror used for the tint.
pub fn toggle_pause(
    button_query: Query<&Interaction, (Changed<Interaction>, With<PauseButton>)>,
    mut paused: ResMut<Paused>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut button_bg: Query<&mut BackgroundColor, With<PauseButton>>,
) {
    for interaction in &button_query {
        if *interaction == Interaction::Pressed {
            paused.0 = !paused.0;

            if paused.0 {
                virtual_time.pause();
            } else {
                virtual_time.unpause();
            }

            for mut bg in &mut button_bg {
                bg.0 = if paused.0 {
                    BUTTON_PAUSE_ACTIVE
                } else {
                    BUTTON_IDLE
                };
            }
        }
    }
}

/// Toggle the exclusive food editor mode.
pub fn toggle_food_management(
    button_query: Query<&Interaction, (Changed<Interaction>, With<FoodManagementButton>)>,
    mut mode: ResMut<EditorMode>,
) {
    for interaction in &button_query {
        if *interaction == Interaction::Pressed {
            mode.0 = if mode.0 == EditorModeKind::Food {
                EditorModeKind::Explore
            } else {
                EditorModeKind::Food
            };
        }
    }
}

/// Toggle the exclusive nest editor mode.
pub fn toggle_nest_management(
    button_query: Query<&Interaction, (Changed<Interaction>, With<NestManagementButton>)>,
    mut mode: ResMut<EditorMode>,
) {
    for interaction in &button_query {
        if *interaction == Interaction::Pressed {
            mode.0 = if mode.0 == EditorModeKind::Nest {
                EditorModeKind::Explore
            } else {
                EditorModeKind::Nest
            };
        }
    }
}

/// Keep both mode buttons tinted from the single [`EditorMode`].
pub fn sync_editor_button_colors(
    mode: Res<EditorMode>,
    mut food_button: Query<
        &mut BackgroundColor,
        (With<FoodManagementButton>, Without<NestManagementButton>),
    >,
    mut nest_button: Query<
        &mut BackgroundColor,
        (With<NestManagementButton>, Without<FoodManagementButton>),
    >,
) {
    if !mode.is_changed() {
        return;
    }

    for mut bg in &mut food_button {
        let color = if mode.0 == EditorModeKind::Food {
            BUTTON_FOOD_ACTIVE
        } else {
            BUTTON_IDLE
        };

        if bg.0 != color {
            bg.0 = color;
        }
    }

    for mut bg in &mut nest_button {
        let color = if mode.0 == EditorModeKind::Nest {
            BUTTON_NEST_ACTIVE
        } else {
            BUTTON_IDLE
        };

        if bg.0 != color {
            bg.0 = color;
        }
    }
}
