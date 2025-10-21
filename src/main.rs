use bevy::prelude::*;

#[derive(Component)]
struct Nest;

#[derive(Component)]
struct Food;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ants Simulation".into(),
                resolution: (800, 600).into(),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Nest,
        Sprite {
            color: Color::srgb(0.4, 0.25, 0.1),
            custom_size: Some(Vec2::new(40.0, 40.0)),
            ..default()
        },
        Transform::from_xyz(-350.0, 0.0, 0.0),
    ));

    commands.spawn((
        Food,
        Sprite {
            color: Color::srgb(0.2, 0.8, 0.2),
            custom_size: Some(Vec2::new(15.0, 15.0)),
            ..default()
        },
        Transform::from_xyz(320.0, 0.0, 0.0),
    ));
}
