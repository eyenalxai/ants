//! Reusable HUD building blocks: panels, tool buttons and their shared colors.

use bevy::prelude::*;

use crate::constants::ui::{
    UI_BUTTON_HOVERED, UI_BUTTON_IDLE, UI_BUTTON_PADDING, UI_BUTTON_PRESSED, UI_FONT_SIZE,
    UI_PANEL_BACKGROUND,
};
use crate::core::sets::Paused;
use crate::editor::{EditorMode, EditorModeKind};

/// Behavior of a [`ToolButton`] on press.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonAction {
    /// Toggle the simulation pause.
    TogglePause,
    /// Toggle the exclusive food editor mode.
    ToggleFoodMode,
    /// Toggle the exclusive nest editor mode.
    ToggleNestMode,
}

/// State that highlights a [`ToolButton`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonActiveWhen {
    /// The simulation is paused.
    Paused,
    /// The given editor mode is active.
    Mode(EditorModeKind),
}

/// A HUD button that reacts to hover/press and to its active state.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct ToolButton {
    /// Action performed when the button is pressed.
    pub action: ButtonAction,
    /// Condition that highlights the button.
    pub active_when: ButtonActiveWhen,
    /// Fill while `active_when` holds.
    pub active_color: Color,
}

impl ToolButton {
    /// Background color for the current interaction and active state.
    pub fn color_for(&self, interaction: Interaction, active: bool) -> Color {
        match (active, interaction) {
            (_, Interaction::Pressed) => UI_BUTTON_PRESSED,
            (true, _) => self.active_color,
            (false, Interaction::Hovered) => UI_BUTTON_HOVERED,
            (false, Interaction::None) => UI_BUTTON_IDLE,
        }
    }
}

/// Whether `active_when` holds for the given editor mode and pause state.
pub fn is_active(active_when: ButtonActiveWhen, mode: EditorModeKind, paused: bool) -> bool {
    match active_when {
        ButtonActiveWhen::Paused => paused,
        ButtonActiveWhen::Mode(kind) => mode == kind,
    }
}

/// Spawn a root panel with the shared translucent background and z-order.
pub fn spawn_panel<'a>(commands: &'a mut Commands, node: Node, z_index: i32) -> EntityCommands<'a> {
    commands.spawn((
        node,
        // The whole panel counts as UI for the editor tools, not just its buttons.
        Interaction::default(),
        BackgroundColor(UI_PANEL_BACKGROUND),
        ZIndex(z_index),
    ))
}

/// Spawn a HUD button with `label` as child text and the shared button visuals.
pub fn spawn_tool_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    label: &str,
    button: ToolButton,
) -> EntityCommands<'a> {
    let mut entity = parent.spawn((
        Button,
        button,
        Node {
            padding: UiRect::all(Val::Px(UI_BUTTON_PADDING)),
            ..default()
        },
        BackgroundColor(UI_BUTTON_IDLE),
    ));

    entity.with_child((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(UI_FONT_SIZE),
            ..default()
        },
        TextColor(Color::WHITE),
        Node::default(),
    ));

    entity
}

/// Recolor every [`ToolButton`] from its interaction and active state.
pub fn sync_tool_button_colors(
    mode: Res<EditorMode>,
    paused: Res<Paused>,
    mut buttons: Query<(&Interaction, &ToolButton, &mut BackgroundColor)>,
) {
    for (interaction, button, mut background) in &mut buttons {
        let color = button.color_for(
            *interaction,
            is_active(button.active_when, mode.0, paused.0),
        );

        if background.0 != color {
            background.0 = color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::ui::UI_BUTTON_FOOD_ACTIVE;

    fn food_button() -> ToolButton {
        ToolButton {
            action: ButtonAction::ToggleFoodMode,
            active_when: ButtonActiveWhen::Mode(EditorModeKind::Food),
            active_color: UI_BUTTON_FOOD_ACTIVE,
        }
    }

    #[test]
    fn button_colors_follow_interaction_and_active_state() {
        let button = food_button();

        assert_eq!(button.color_for(Interaction::None, false), UI_BUTTON_IDLE);
        assert_eq!(
            button.color_for(Interaction::Hovered, false),
            UI_BUTTON_HOVERED
        );
        assert_eq!(
            button.color_for(Interaction::Pressed, false),
            UI_BUTTON_PRESSED
        );
        assert_eq!(
            button.color_for(Interaction::None, true),
            UI_BUTTON_FOOD_ACTIVE
        );
        assert_eq!(
            button.color_for(Interaction::Hovered, true),
            UI_BUTTON_FOOD_ACTIVE
        );
        assert_eq!(
            button.color_for(Interaction::Pressed, true),
            UI_BUTTON_PRESSED
        );
    }

    #[test]
    fn active_state_matches_mode_and_pause() {
        assert!(is_active(
            ButtonActiveWhen::Mode(EditorModeKind::Food),
            EditorModeKind::Food,
            false
        ));
        assert!(!is_active(
            ButtonActiveWhen::Mode(EditorModeKind::Food),
            EditorModeKind::Nest,
            false
        ));
        assert!(is_active(
            ButtonActiveWhen::Paused,
            EditorModeKind::Explore,
            true
        ));
        assert!(!is_active(
            ButtonActiveWhen::Paused,
            EditorModeKind::Explore,
            false
        ));
    }
}
