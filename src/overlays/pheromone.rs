//! Single-sprite pheromone overlay (F3), backed by one texel per grid cell.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::time::Real;

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
/// most 30 times per second. The throttle counts real time rather than virtual
/// time, so edits made while the simulation is paused (for example a nest drag
/// clearing the `to_nest` channel) still refresh the overlay. Nothing but the
/// visibility is touched while the overlay is off, and an unchanged
/// [`PheromoneGrid::version`] never re-uploads the 30k-cell texture.
pub fn update_pheromone_visuals(
    time: Res<Time<Real>>,
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

    let version = pheromone_grid.version();

    // Unchanged content never uploads and never accumulates throttle time.
    if version == overlay.uploaded_version {
        return;
    }

    // First upload after enabling happens immediately; later ones are
    // throttled to 30 Hz of real time.
    if overlay.uploaded_version != u64::MAX {
        overlay.upload_accumulator += time.delta_secs();

        if overlay.upload_accumulator < UPLOAD_INTERVAL_SECS {
            return;
        }
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

    overlay.uploaded_version = version;
    overlay.upload_accumulator = 0.0;
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
    use crate::core::sets::Paused;
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;

    /// Byte alpha of a single fully saturated channel:
    /// `min(0.9, 1.0) * PHEROMONE_VISUAL_ALPHA * 255`, rounded.
    const FULL_ALPHA_BYTE: u8 = 115;

    fn overlay_world() -> World {
        let mut world = World::new();
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(Time::<Real>::default());
        world.insert_resource(PheromoneDisplayState { enabled: true });
        world.init_resource::<PheromoneGrid>();
        world.run_system_once(setup_pheromone_overlay).unwrap();

        world
    }

    fn overlay_parts(world: &mut World) -> (Entity, Handle<Image>) {
        let mut query = world.query::<(Entity, &PheromoneOverlay)>();
        let (entity, overlay) = query.single(world).expect("one overlay sprite");
        (entity, overlay.image.clone())
    }

    /// Flat byte offset of `cell` in the top-down texture.
    fn flat_offset(cell: UVec2) -> usize {
        (GRID_HEIGHT - 1 - cell.y as usize) * GRID_WIDTH * 4 + cell.x as usize * 4
    }

    fn image_pixel(world: &World, image: &Handle<Image>, offset: usize) -> [u8; 4] {
        let images = world.resource::<Assets<Image>>();
        let data = images
            .get(image)
            .and_then(|image| image.data.as_ref())
            .expect("overlay image data");

        data[offset..offset + 4].try_into().unwrap()
    }

    fn uploaded_version(world: &World, overlay: Entity) -> u64 {
        world
            .get::<PheromoneOverlay>(overlay)
            .unwrap()
            .uploaded_version
    }

    fn advance_real_time(world: &mut World, millis: u64) {
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(millis));
    }

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
        let mut world = overlay_world();

        let cell = UVec2::new(3, 5);
        world
            .resource_mut::<PheromoneGrid>()
            .add(cell, PHEROMONE_VISUAL_SCALE, 0.0);

        world.run_system_once(update_pheromone_visuals).unwrap();

        let (overlay_entity, image_handle) = overlay_parts(&mut world);
        assert_eq!(
            *world.get::<Visibility>(overlay_entity).unwrap(),
            Visibility::Visible
        );

        // Texture rows run top-down, grid rows bottom-up.
        let flat = flat_offset(cell);
        let expected = cell_pixel(&Pheromone {
            to_food: PHEROMONE_VISUAL_SCALE,
            to_nest: 0.0,
        });
        assert_eq!(image_pixel(&world, &image_handle, flat), expected);

        // Turning the overlay off hides the sprite and leaves the texture
        // untouched.
        world.resource_mut::<PheromoneDisplayState>().enabled = false;
        world.run_system_once(update_pheromone_visuals).unwrap();

        assert_eq!(
            *world.get::<Visibility>(overlay_entity).unwrap(),
            Visibility::Hidden
        );
        assert_eq!(image_pixel(&world, &image_handle, flat), expected);
    }

    #[test]
    fn paused_edits_refresh_the_overlay_on_real_time() {
        let mut world = overlay_world();
        // A frozen virtual clock and an active pause flag must not stall the
        // overlay: the throttle counts `Time<Real>` instead.
        world.insert_resource(Time::<()>::default());
        world.insert_resource(Paused(true));

        let cell = UVec2::new(3, 5);
        world
            .resource_mut::<PheromoneGrid>()
            .add(cell, 0.0, PHEROMONE_VISUAL_SCALE);

        world.run_system_once(update_pheromone_visuals).unwrap();

        let (overlay_entity, image_handle) = overlay_parts(&mut world);
        let flat = flat_offset(cell);
        assert_eq!(
            image_pixel(&world, &image_handle, flat),
            cell_pixel(&Pheromone {
                to_food: 0.0,
                to_nest: PHEROMONE_VISUAL_SCALE,
            })
        );

        // A paused nest drag clears the to-nest channel.
        world.resource_mut::<PheromoneGrid>().clear_to_nest();
        let version = world.resource::<PheromoneGrid>().version();

        // Still inside the 30 Hz window: no upload yet.
        advance_real_time(&mut world, 5);
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_ne!(uploaded_version(&world, overlay_entity), version);

        // Past the window: the overlay follows the paused edit.
        advance_real_time(&mut world, 40);
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_eq!(uploaded_version(&world, overlay_entity), version);
        assert_eq!(image_pixel(&world, &image_handle, flat), [0, 0, 0, 0]);
    }

    #[test]
    fn unchanged_or_disabled_overlays_do_not_reupload() {
        let mut world = overlay_world();

        let cell = UVec2::new(3, 5);
        world
            .resource_mut::<PheromoneGrid>()
            .add(cell, PHEROMONE_VISUAL_SCALE, 0.0);
        world.run_system_once(update_pheromone_visuals).unwrap();

        let (overlay_entity, image_handle) = overlay_parts(&mut world);
        let flat = flat_offset(cell);

        // Corrupt the texture so any upload is detectable.
        {
            let mut images = world.resource_mut::<Assets<Image>>();
            let mut image = images.get_mut(&image_handle).expect("overlay image");
            let data = image.data.as_mut().expect("overlay image data");
            data[flat..flat + 4].copy_from_slice(&[0xAB; 4]);
        }

        // Unchanged version: no upload even after the throttle window.
        advance_real_time(&mut world, 40);
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_eq!(image_pixel(&world, &image_handle, flat), [0xAB; 4]);

        // Disabled overlay: no upload either.
        world.resource_mut::<PheromoneDisplayState>().enabled = false;
        advance_real_time(&mut world, 40);
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_eq!(image_pixel(&world, &image_handle, flat), [0xAB; 4]);
        assert_eq!(
            *world.get::<Visibility>(overlay_entity).unwrap(),
            Visibility::Hidden
        );

        // A real change uploads again once enabled and past the throttle.
        world.resource_mut::<PheromoneDisplayState>().enabled = true;
        world
            .resource_mut::<PheromoneGrid>()
            .add(cell, PHEROMONE_VISUAL_SCALE, 0.0);
        advance_real_time(&mut world, 40);
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_ne!(image_pixel(&world, &image_handle, flat), [0xAB; 4]);
    }

    #[test]
    fn visibility_is_only_written_on_transitions() {
        let mut world = overlay_world();
        world.run_system_once(update_pheromone_visuals).unwrap();

        let (overlay_entity, _) = overlay_parts(&mut world);
        let visible_tick = world
            .entity(overlay_entity)
            .get_change_ticks::<Visibility>()
            .unwrap()
            .changed;

        // An enabled, unchanged overlay must not re-mark its visibility.
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_eq!(
            visible_tick,
            world
                .entity(overlay_entity)
                .get_change_ticks::<Visibility>()
                .unwrap()
                .changed
        );

        // The hide transition still writes exactly once.
        world.resource_mut::<PheromoneDisplayState>().enabled = false;
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_eq!(
            *world.get::<Visibility>(overlay_entity).unwrap(),
            Visibility::Hidden
        );
        let hidden_tick = world
            .entity(overlay_entity)
            .get_change_ticks::<Visibility>()
            .unwrap()
            .changed;
        world.run_system_once(update_pheromone_visuals).unwrap();
        assert_eq!(
            hidden_tick,
            world
                .entity(overlay_entity)
                .get_change_ticks::<Visibility>()
                .unwrap()
                .changed
        );
    }
}
