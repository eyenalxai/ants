//! Single-sprite pheromone overlay (F3), backed by one texel per grid cell.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::constants::pheromone::{PHEROMONE_MAX_INTENSITY, PHEROMONE_VISUAL_ALPHA};
use crate::constants::world::{GRID_HEIGHT, GRID_WIDTH, PLAY_AREA_HEIGHT, PLAY_AREA_WIDTH};
use crate::core::layers::Z_PHEROMONE;
use crate::overlays::PheromoneDisplayState;
use crate::pheromone::grid::{Pheromone, PheromoneGrid};

/// Minimum delay between two overlay texture uploads (30 Hz).
const UPLOAD_INTERVAL_SECS: f32 = 1.0 / 30.0;

/// The single overlay sprite plus its upload bookkeeping.
#[derive(Component)]
pub struct PheromoneOverlay {
    image: Handle<Image>,
    uploaded_version: u64,
    upload_accumulator: f32,
}

/// Spawn the one overlay sprite covering the play area. Its texture is
/// `GRID_WIDTH` x `GRID_HEIGHT`, with nearest sampling so each texel maps to
/// one grid cell.
pub fn setup_pheromone_overlay(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let extent = Extent3d {
        width: GRID_WIDTH as u32,
        height: GRID_HEIGHT as u32,
        depth_or_array_layers: 1,
    };

    let mut image = Image::new(
        extent,
        TextureDimension::D2,
        vec![0; GRID_WIDTH * GRID_HEIGHT * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image.texture_descriptor.label = Some("pheromone_overlay");

    let image_handle = images.add(image);

    commands.spawn((
        PheromoneOverlay {
            image: image_handle.clone(),
            uploaded_version: u64::MAX,
            upload_accumulator: 0.0,
        },
        Sprite {
            image: image_handle,
            color: Color::WHITE,
            custom_size: Some(Vec2::new(PLAY_AREA_WIDTH, PLAY_AREA_HEIGHT)),
            image_mode: SpriteImageMode::Auto,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, Z_PHEROMONE),
        Visibility::Hidden,
    ));
}

/// Upload the grid into the overlay texture when it is visible and changed, at
/// most 30 times per second. Nothing but the visibility is touched while the
/// overlay is off.
pub fn update_pheromone_visuals(
    time: Res<Time>,
    pheromone_grid: Res<PheromoneGrid>,
    display_state: Res<PheromoneDisplayState>,
    mut images: ResMut<Assets<Image>>,
    mut overlay_query: Query<(&mut PheromoneOverlay, &mut Visibility)>,
) {
    let Ok((mut overlay, mut visibility)) = overlay_query.single_mut() else {
        return;
    };

    if !display_state.enabled {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }

        return;
    }

    if *visibility != Visibility::Visible {
        *visibility = Visibility::Visible;
    }

    // First upload after enabling happens immediately; later ones are
    // throttled to 30 Hz.
    if overlay.uploaded_version != u64::MAX {
        overlay.upload_accumulator += time.delta_secs();

        if overlay.upload_accumulator < UPLOAD_INTERVAL_SECS {
            return;
        }

        overlay.upload_accumulator = 0.0;
    }

    if pheromone_grid.version() == overlay.uploaded_version {
        return;
    }

    let Some(mut image) = images.get_mut(&overlay.image) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };

    // Texture rows run top-down, grid rows bottom-up.
    for (row_index, row) in data
        .as_chunks_mut::<{ GRID_WIDTH * 4 }>()
        .0
        .iter_mut()
        .enumerate()
    {
        let y = (GRID_HEIGHT - 1 - row_index) as u32;

        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let color = pheromone_grid
                .get(UVec2::new(x as u32, y))
                .map_or([0; 4], cell_pixel);

            *pixel = color;
        }
    }

    overlay.uploaded_version = pheromone_grid.version();
}

/// RGBA bytes for one cell: red is `to_food`, blue is `to_nest`, alpha scales
/// with the combined intensity.
fn cell_pixel(pheromone: &Pheromone) -> [u8; 4] {
    let to_food = (pheromone.to_food / PHEROMONE_MAX_INTENSITY).clamp(0.0, 1.0);
    let to_nest = (pheromone.to_nest / PHEROMONE_MAX_INTENSITY).clamp(0.0, 1.0);
    let alpha = ((pheromone.to_food.min(PHEROMONE_MAX_INTENSITY)
        + pheromone.to_nest.min(PHEROMONE_MAX_INTENSITY))
        / PHEROMONE_MAX_INTENSITY)
        .clamp(0.0, 1.0)
        * PHEROMONE_VISUAL_ALPHA;

    [
        (to_food * 255.0).round() as u8,
        0,
        (to_nest * 255.0).round() as u8,
        (alpha * 255.0).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_color_maps_channels_with_shared_alpha() {
        let food_only = cell_pixel(&Pheromone {
            to_food: PHEROMONE_MAX_INTENSITY,
            to_nest: 0.0,
        });

        assert_eq!(food_only[0], u8::MAX);
        assert_eq!(food_only[2], 0);
        assert_eq!(food_only[3], (PHEROMONE_VISUAL_ALPHA * 255.0).round() as u8);

        let saturated = cell_pixel(&Pheromone {
            to_food: PHEROMONE_MAX_INTENSITY,
            to_nest: PHEROMONE_MAX_INTENSITY,
        });

        assert_eq!(saturated[0], u8::MAX);
        assert_eq!(saturated[2], u8::MAX);
        assert_eq!(saturated[3], food_only[3]);

        let empty = cell_pixel(&Pheromone::default());

        assert_eq!(empty, [0, 0, 0, 0]);
    }
}
