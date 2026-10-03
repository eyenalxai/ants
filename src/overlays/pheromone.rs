//! Single-sprite pheromone overlay (F3), backed by one texel per grid cell.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::constants::pheromone::{PHEROMONE_VISUAL_ALPHA, PHEROMONE_VISUAL_SCALE};
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

/// Compressive visual normalization shared by the overlay and the sensor-cone
/// markers: `v = sqrt((raw / PHEROMONE_VISUAL_SCALE).clamp(0, 1))`.
///
/// The square root lifts operational readings (one ant pass is a small
/// fraction of [`PHEROMONE_VISUAL_SCALE`]) into visible byte values while the
/// clamp saturates busy trails without wrapping.
pub(crate) fn normalized_visual(raw: f32) -> f32 {
    (raw / PHEROMONE_VISUAL_SCALE).clamp(0.0, 1.0).sqrt()
}

/// RGBA bytes for one cell: red is `to_food`, blue is `to_nest`, alpha scales
/// with the combined normalized intensity (capped at 0.9 before the visual
/// alpha multiplier so overlapping channels stay translucent).
fn cell_pixel(pheromone: &Pheromone) -> [u8; 4] {
    let to_food = normalized_visual(pheromone.to_food);
    let to_nest = normalized_visual(pheromone.to_nest);
    let alpha = (to_food + to_nest).min(0.9) * PHEROMONE_VISUAL_ALPHA;

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
    use bevy::ecs::system::RunSystemOnce;

    /// Byte alpha of a single fully saturated channel:
    /// `min(0.9, 1.0) * PHEROMONE_VISUAL_ALPHA * 255`, rounded.
    const FULL_ALPHA_BYTE: u8 = 115;

    #[test]
    fn visual_normalization_is_compressive_and_saturating() {
        assert_eq!(normalized_visual(0.0), 0.0);
        assert_eq!(normalized_visual(-1.0), 0.0);
        assert_eq!(normalized_visual(PHEROMONE_VISUAL_SCALE), 1.0);
        assert_eq!(normalized_visual(PHEROMONE_VISUAL_SCALE * 10.0), 1.0);

        let quarter = normalized_visual(PHEROMONE_VISUAL_SCALE * 0.25);
        assert!(
            (quarter - 0.5).abs() < 1e-6,
            "sqrt should lift mid-range readings"
        );
    }

    #[test]
    fn pixel_color_maps_channels_with_shared_alpha() {
        let food_only = cell_pixel(&Pheromone {
            to_food: PHEROMONE_VISUAL_SCALE,
            to_nest: 0.0,
        });

        assert_eq!(food_only, [u8::MAX, 0, 0, FULL_ALPHA_BYTE]);

        let saturated = cell_pixel(&Pheromone {
            to_food: PHEROMONE_VISUAL_SCALE,
            to_nest: PHEROMONE_VISUAL_SCALE,
        });

        assert_eq!(saturated[0], u8::MAX);
        assert_eq!(saturated[2], u8::MAX);
        assert_eq!(saturated[3], FULL_ALPHA_BYTE);

        let empty = cell_pixel(&Pheromone::default());

        assert_eq!(empty, [0, 0, 0, 0]);
    }

    #[test]
    fn operational_trail_values_are_clearly_visible() {
        // Half of the visual scale: sqrt(0.25) = 0.5 -> 128 red, 64 alpha.
        let half = cell_pixel(&Pheromone {
            to_food: PHEROMONE_VISUAL_SCALE / 4.0,
            to_nest: 0.0,
        });
        assert_eq!(half, [128, 0, 0, 64]);

        // A busy trail (raw ~20) must be unmistakably visible instead of the
        // 1-2 byte alpha the old storage-scale mapping produced.
        let busy = cell_pixel(&Pheromone {
            to_food: PHEROMONE_VISUAL_SCALE,
            to_nest: 0.0,
        });
        assert!(busy[3] >= 96, "busy trail alpha was {}", busy[3]);
    }

    #[test]
    fn overlay_uploads_cell_pixels_and_hides_when_disabled() {
        let mut world = World::new();
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(Time::<()>::default());
        world.insert_resource(PheromoneDisplayState { enabled: true });
        world.init_resource::<PheromoneGrid>();

        let cell = UVec2::new(3, 5);
        world
            .resource_mut::<PheromoneGrid>()
            .add(cell, PHEROMONE_VISUAL_SCALE, 0.0);

        world.run_system_once(setup_pheromone_overlay).unwrap();
        world.run_system_once(update_pheromone_visuals).unwrap();

        let (overlay_entity, image_handle) = {
            let mut query = world.query::<(Entity, &PheromoneOverlay)>();
            let (entity, overlay) = query.single(&world).expect("one overlay sprite");
            (entity, overlay.image.clone())
        };
        assert_eq!(
            *world.get::<Visibility>(overlay_entity).unwrap(),
            Visibility::Visible
        );

        // Texture rows run top-down, grid rows bottom-up.
        let flat = (GRID_HEIGHT - 1 - cell.y as usize) * GRID_WIDTH * 4 + cell.x as usize * 4;
        let expected = cell_pixel(&Pheromone {
            to_food: PHEROMONE_VISUAL_SCALE,
            to_nest: 0.0,
        });
        let uploaded = {
            let images = world.resource::<Assets<Image>>();
            let data = images
                .get(&image_handle)
                .and_then(|image| image.data.as_ref())
                .expect("overlay image data");
            data[flat..flat + 4].to_vec()
        };
        assert_eq!(uploaded, expected);

        // Turning the overlay off hides the sprite and leaves the texture
        // untouched.
        world.resource_mut::<PheromoneDisplayState>().enabled = false;
        world.run_system_once(update_pheromone_visuals).unwrap();

        assert_eq!(
            *world.get::<Visibility>(overlay_entity).unwrap(),
            Visibility::Hidden
        );
        let images = world.resource::<Assets<Image>>();
        let data = images
            .get(&image_handle)
            .and_then(|image| image.data.as_ref())
            .expect("overlay image data");
        assert_eq!(&data[flat..flat + 4], uploaded.as_slice());
    }
}
