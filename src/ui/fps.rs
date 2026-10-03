use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::constants::ui::{
    UI_EDGE_INSET_PERCENT, UI_FONT_SIZE, UI_FPS_GOOD, UI_FPS_LOW, UI_FPS_OK, UI_FPS_PANEL_PADDING,
    UI_Z_FPS,
};
use crate::ui::widgets;

#[derive(Component)]
pub struct FpsRoot;

#[derive(Component)]
pub struct FpsText;

pub fn setup_fps_counter(mut commands: Commands) {
    let root = widgets::spawn_panel(
        &mut commands,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Percent(UI_EDGE_INSET_PERCENT),
            top: Val::Percent(UI_EDGE_INSET_PERCENT),
            padding: UiRect::all(Val::Px(UI_FPS_PANEL_PADDING)),
            ..default()
        },
        UI_Z_FPS,
    )
    .insert(FpsRoot)
    .id();

    let text_fps = commands
        .spawn((
            FpsText,
            Text::new("FPS: "),
            TextFont {
                font_size: FontSize::Px(UI_FONT_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ))
        .with_child((
            TextSpan::new(" N/A"),
            TextFont {
                font_size: FontSize::Px(UI_FONT_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .id();

    commands.entity(root).add_children(&[text_fps]);
}

pub fn fps_text_update_system(
    diagnostics: Res<DiagnosticsStore>,
    mut span_query: Query<(&mut TextSpan, &mut TextColor), Without<FpsText>>,
) {
    if let Some(value) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed())
    {
        for (mut span, mut color) in &mut span_query {
            span.0 = format!("{value:>4.0}");

            color.0 = if value >= UI_FPS_GOOD {
                Color::srgb(0.0, 1.0, 0.0)
            } else if value >= UI_FPS_OK {
                Color::srgb(
                    (1.0 - (value - UI_FPS_OK) / (UI_FPS_GOOD - UI_FPS_OK)) as f32,
                    1.0,
                    0.0,
                )
            } else if value >= UI_FPS_LOW {
                Color::srgb(
                    1.0,
                    ((value - UI_FPS_LOW) / (UI_FPS_OK - UI_FPS_LOW)) as f32,
                    0.0,
                )
            } else {
                Color::srgb(1.0, 0.0, 0.0)
            };
        }
    } else {
        for (mut span, mut color) in &mut span_query {
            span.0 = " N/A".into();
            color.0 = Color::WHITE;
        }
    }
}

pub fn fps_counter_showhide(
    mut q: Query<&mut Visibility, With<FpsRoot>>,
    kbd: Res<ButtonInput<KeyCode>>,
) {
    if kbd.just_pressed(KeyCode::F12) {
        for mut vis in &mut q {
            *vis = match *vis {
                Visibility::Hidden => Visibility::Visible,
                _ => Visibility::Hidden,
            };
        }
    }
}
