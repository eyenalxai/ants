use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

#[derive(Component)]
pub struct FpsRoot;

#[derive(Component)]
pub struct FpsText;

pub fn setup_fps_counter(mut commands: Commands) {
    let root = commands
        .spawn((
            FpsRoot,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Percent(1.),
                top: Val::Percent(1.),
                bottom: Val::Auto,
                left: Val::Auto,
                padding: UiRect::all(Val::Px(4.0)),
                ..Default::default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            GlobalZIndex(i32::MAX),
        ))
        .id();

    let text_fps = commands
        .spawn((
            FpsText,
            Text::new("FPS: "),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node::default(),
        ))
        .with_child((
            TextSpan::new(" N/A"),
            TextFont {
                font_size: 16.0,
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

            color.0 = if value >= 120.0 {
                Color::srgb(0.0, 1.0, 0.0)
            } else if value >= 60.0 {
                Color::srgb((1.0 - (value - 60.0) / (120.0 - 60.0)) as f32, 1.0, 0.0)
            } else if value >= 30.0 {
                Color::srgb(1.0, ((value - 30.0) / (60.0 - 30.0)) as f32, 0.0)
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
