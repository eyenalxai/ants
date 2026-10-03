//! Frame-rate counter panel and its update system.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::constants::ui::{
    UI_EDGE_INSET_PERCENT, UI_FONT_SIZE, UI_FPS_GOOD, UI_FPS_LOW, UI_FPS_OK, UI_FPS_PANEL_PADDING,
    UI_Z_FPS,
};
use crate::perf::{PerfEnabled, PerfStats};
use crate::ui::widgets;

#[derive(Component)]
pub struct FpsRoot;

#[derive(Component)]
pub struct FpsText;

/// Marks the numeric span that [`fps_text_update_system`] writes to.
#[derive(Component)]
pub struct FpsValue;

/// Last rounded FPS shown in the span. `format!` and the text/color writes are
/// skipped while the displayed value is unchanged, so an idle frame does not
/// dirty the UI text layout.
#[derive(Component, Default)]
pub struct FpsValueCache {
    rounded: Option<i32>,
}

/// Marks the optional perf span; it stays empty unless `ANTS_PERF=1` enables
/// [`crate::perf`] counters.
#[derive(Component)]
pub struct FpsPerfValue;

/// Last rounded timings shown in the perf span (microseconds).
#[derive(Component, Default)]
pub struct FpsPerfCache {
    chain_micros: u32,
    frame_micros: u32,
}

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
            FpsValue,
            FpsValueCache::default(),
            TextSpan::new(" N/A"),
            TextFont {
                font_size: FontSize::Px(UI_FONT_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .with_child((
            FpsPerfValue,
            FpsPerfCache::default(),
            TextSpan::new(""),
            TextFont {
                font_size: FontSize::Px(UI_FONT_SIZE),
                ..default()
            },
            TextColor(Color::srgb(0.7, 0.7, 0.7)),
        ))
        .id();

    commands.entity(root).add_children(&[text_fps]);
}

/// Color for an FPS reading; green at/above [`UI_FPS_GOOD`], red below
/// [`UI_FPS_LOW`], a yellow ramp in between.
pub fn fps_color(value: f64) -> Color {
    if value >= UI_FPS_GOOD {
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
    }
}

pub fn fps_text_update_system(
    diagnostics: Res<DiagnosticsStore>,
    mut span_query: Query<(&mut TextSpan, &mut TextColor, &mut FpsValueCache), With<FpsValue>>,
) {
    let value = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed());
    let rounded = value.map(|value| value.round() as i32);

    for (mut span, mut color, mut cache) in &mut span_query {
        if cache.rounded == rounded {
            continue;
        }

        cache.rounded = rounded;

        match value {
            Some(value) => {
                span.0 = format!("{value:>4.0}");
                color.0 = fps_color(value);
            }
            None => {
                span.0 = " N/A".into();
                color.0 = Color::WHITE;
            }
        }
    }
}

/// Append the optional perf timings next to the FPS value. Runs only when
/// `ANTS_PERF=1` opted in; the span is left empty otherwise.
pub fn fps_perf_text_update_system(
    enabled: Res<PerfEnabled>,
    stats: Res<PerfStats>,
    mut span_query: Query<(&mut TextSpan, &mut FpsPerfCache), With<FpsPerfValue>>,
) {
    if !enabled.0 {
        return;
    }

    let chain_micros = micros(stats.smoothed_chain_secs);
    let frame_micros = micros(stats.frame_secs);

    for (mut span, mut cache) in &mut span_query {
        if cache.chain_micros == chain_micros && cache.frame_micros == frame_micros {
            continue;
        }

        cache.chain_micros = chain_micros;
        cache.frame_micros = frame_micros;

        span.0 = format!(
            "  sim {:.2} ms | frame {:.2} ms",
            chain_micros as f32 / 1000.0,
            frame_micros as f32 / 1000.0
        );
    }
}

/// Seconds as whole microseconds, saturating instead of wrapping.
fn micros(secs: f32) -> u32 {
    (secs * 1e6).round().clamp(0.0, u32::MAX as f32) as u32
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::diagnostic::{Diagnostic, DiagnosticMeasurement};
    use bevy::ecs::change_detection::Tick;
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Instant;

    fn diagnostics_with_fps(value: f64) -> DiagnosticsStore {
        let mut store = DiagnosticsStore::default();
        let mut diagnostic = Diagnostic::new(FrameTimeDiagnosticsPlugin::FPS);
        diagnostic.add_measurement(DiagnosticMeasurement {
            time: Instant::now(),
            value,
        });
        store.add(diagnostic);

        store
    }

    fn fps_span(world: &mut World) -> Entity {
        let mut query = world.query_filtered::<Entity, With<FpsValue>>();
        query.single(world).expect("fps value span")
    }

    fn span_tick(world: &World, entity: Entity) -> Tick {
        world
            .entity(entity)
            .get_change_ticks::<TextSpan>()
            .expect("span has text")
            .changed
    }

    #[test]
    fn color_thresholds_are_shared_and_monotonic() {
        assert_eq!(fps_color(UI_FPS_GOOD), Color::srgb(0.0, 1.0, 0.0));
        assert_eq!(fps_color(UI_FPS_GOOD + 30.0), Color::srgb(0.0, 1.0, 0.0));

        let mid = fps_color((UI_FPS_OK + UI_FPS_GOOD) / 2.0).to_srgba();
        assert!((mid.red - 0.5).abs() < 1e-6);
        assert!((mid.green - 1.0).abs() < 1e-6);

        let low = fps_color((UI_FPS_LOW + UI_FPS_OK) / 2.0).to_srgba();
        assert!((low.red - 1.0).abs() < 1e-6);
        assert!((low.green - 0.5).abs() < 1e-6);

        assert_eq!(fps_color(UI_FPS_LOW - 1.0), Color::srgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn fps_text_is_not_rewritten_while_the_rounded_value_is_stable() {
        let mut world = World::new();
        world.insert_resource(diagnostics_with_fps(60.0));
        world.run_system_once(setup_fps_counter).unwrap();

        world.run_system_once(fps_text_update_system).unwrap();

        let span = fps_span(&mut world);
        assert_eq!(world.get::<TextSpan>(span).unwrap().0, "  60");

        // A sub-integer change must not touch the span.
        let before = span_tick(&world, span);
        world.insert_resource(diagnostics_with_fps(60.2));
        world.run_system_once(fps_text_update_system).unwrap();
        assert_eq!(before, span_tick(&world, span));
        assert_eq!(world.get::<TextSpan>(span).unwrap().0, "  60");

        // A value that rounds differently does.
        world.insert_resource(diagnostics_with_fps(42.0));
        world.run_system_once(fps_text_update_system).unwrap();
        assert_ne!(before, span_tick(&world, span));
        assert_eq!(world.get::<TextSpan>(span).unwrap().0, "  42");
    }

    #[test]
    fn perf_span_stays_empty_until_enabled_and_caches_values() {
        let mut world = World::new();
        world.insert_resource(PerfEnabled(false));
        world.insert_resource(PerfStats::default());
        world.run_system_once(setup_fps_counter).unwrap();

        let mut query = world.query_filtered::<Entity, With<FpsPerfValue>>();
        let span = query.single(&world).expect("perf span");

        world.run_system_once(fps_perf_text_update_system).unwrap();
        assert_eq!(world.get::<TextSpan>(span).unwrap().0, "");

        // Enabled: the line appears and then stays untouched while the
        // displayed timings are unchanged.
        world.resource_mut::<PerfEnabled>().0 = true;
        {
            let mut stats = world.resource_mut::<PerfStats>();
            stats.smoothed_chain_secs = 0.006_27;
            stats.frame_secs = 0.015_0;
        }
        world.run_system_once(fps_perf_text_update_system).unwrap();
        assert_eq!(
            world.get::<TextSpan>(span).unwrap().0,
            "  sim 6.27 ms | frame 15.00 ms"
        );

        let before = span_tick(&world, span);
        world.run_system_once(fps_perf_text_update_system).unwrap();
        assert_eq!(before, span_tick(&world, span));
    }
}
