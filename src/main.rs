use bevy::prelude::*;

const GRID_SIZE: f32 = 8.0;
const WINDOW_WIDTH: f32 = 800.0;
const WINDOW_HEIGHT: f32 = 600.0;
const GRID_WIDTH: usize = (WINDOW_WIDTH / GRID_SIZE) as usize;
const GRID_HEIGHT: usize = (WINDOW_HEIGHT / GRID_SIZE) as usize;

#[derive(Component)]
struct Nest;

#[derive(Component)]
struct Food;

#[derive(Clone, Copy, Default)]
struct Pheromone {
    to_food: f32,
    to_nest: f32,
}

#[derive(Resource)]
struct PheromoneGrid {
    cells: Vec<Vec<Pheromone>>,
}

impl PheromoneGrid {
    fn new() -> Self {
        Self {
            cells: vec![vec![Pheromone::default(); GRID_WIDTH]; GRID_HEIGHT],
        }
    }

    fn world_to_grid(&self, world_pos: Vec2) -> Option<(usize, usize)> {
        let x = ((world_pos.x + WINDOW_WIDTH / 2.0) / GRID_SIZE) as i32;
        let y = ((world_pos.y + WINDOW_HEIGHT / 2.0) / GRID_SIZE) as i32;

        if x >= 0 && x < GRID_WIDTH as i32 && y >= 0 && y < GRID_HEIGHT as i32 {
            Some((x as usize, y as usize))
        } else {
            None
        }
    }

    fn get(&self, x: usize, y: usize) -> Option<&Pheromone> {
        self.cells.get(y).and_then(|row| row.get(x))
    }

    fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut Pheromone> {
        self.cells.get_mut(y).and_then(|row| row.get_mut(x))
    }
}

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
        .insert_resource(PheromoneGrid::new())
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
