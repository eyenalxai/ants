//! Top-left HUD row (pause + exclusive editor tools) and the paused indicator.

use bevy::prelude::*;

use crate::constants::ui::{
    UI_BUTTON_FOOD_ACTIVE, UI_BUTTON_NEST_ACTIVE, UI_BUTTON_PAUSE_ACTIVE, UI_EDGE_INSET_PERCENT,
    UI_HUD_GAP, UI_PANEL_PADDING, UI_PAUSED_FONT_SIZE, UI_PAUSED_TEXT, UI_PAUSED_TEXT_COLOR,
    UI_PAUSED_TOP_PERCENT, UI_Z_HUD, UI_Z_PAUSED,
};
use crate::core::sets::Paused;
use crate::editor::{EditorMode, EditorModeKind, toggled_mode};
use crate::ui::widgets::{self, ButtonAction, ButtonActiveWhen, ToolButton};

/// Marker for the "PAUSED" overlay root.
#[derive(Component)]
pub struct PausedIndicator;

/// Spawn the HUD row and the paused indicator once.
pub fn setup_hud(mut commands: Commands) {
    let mut hud = widgets::spawn_panel(
        &mut commands,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(UI_EDGE_INSET_PERCENT),
            top: Val::Percent(UI_EDGE_INSET_PERCENT),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(UI_HUD_GAP),
            padding: UiRect::all(Val::Px(UI_PANEL_PADDING)),
            ..default()
        },
        UI_Z_HUD,
    );

    hud.with_children(|parent| {
        widgets::spawn_tool_button(
            parent,
            "Pause",
            ToolButton {
                action: ButtonAction::TogglePause,
                active_when: ButtonActiveWhen::Paused,
                active_color: UI_BUTTON_PAUSE_ACTIVE,
            },
        );

        widgets::spawn_tool_button(
            parent,
            "Food Mode",
            ToolButton {
                action: ButtonAction::ToggleFoodMode,
                active_when: ButtonActiveWhen::Mode(EditorModeKind::Food),
                active_color: UI_BUTTON_FOOD_ACTIVE,
            },
        );

        widgets::spawn_tool_button(
            parent,
            "Nest Mode",
            ToolButton {
                action: ButtonAction::ToggleNestMode,
                active_when: ButtonActiveWhen::Mode(EditorModeKind::Nest),
                active_color: UI_BUTTON_NEST_ACTIVE,
            },
        );
    });

    widgets::spawn_panel(
        &mut commands,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(UI_EDGE_INSET_PERCENT),
            top: Val::Percent(UI_PAUSED_TOP_PERCENT),
            padding: UiRect::all(Val::Px(UI_PANEL_PADDING)),
            ..default()
        },
        UI_Z_PAUSED,
    )
    .insert((PausedIndicator, Visibility::Hidden))
    .with_child((
        Text::new(UI_PAUSED_TEXT),
        TextFont {
            font_size: FontSize::Px(UI_PAUSED_FONT_SIZE),
            ..default()
        },
        TextColor(UI_PAUSED_TEXT_COLOR),
        Node::default(),
    ));
}

/// Dispatch HUD button presses to the pause flag and the exclusive editor mode.
pub fn handle_button_press(
    buttons: Query<(&Interaction, &ToolButton), Changed<Interaction>>,
    mut mode: ResMut<EditorMode>,
    mut paused: ResMut<Paused>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match button.action {
            ButtonAction::TogglePause => {
                paused.0 = !paused.0;

                if paused.0 {
                    virtual_time.pause();
                } else {
                    virtual_time.unpause();
                }
            }
            ButtonAction::ToggleFoodMode => {
                mode.0 = toggled_mode(mode.0, EditorModeKind::Food);
            }
            ButtonAction::ToggleNestMode => {
                mode.0 = toggled_mode(mode.0, EditorModeKind::Nest);
            }
        }
    }
}

/// Show the paused indicator only while `Paused` holds (change-driven).
pub fn sync_paused_indicator(
    paused: Res<Paused>,
    indicator: Single<&mut Visibility, With<PausedIndicator>>,
) {
    if !paused.is_changed() {
        return;
    }

    *indicator.into_inner() = if paused.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}
