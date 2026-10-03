//! Top-left HUD row (pause + exclusive editor tools), the paused indicator and
//! the colony food-economy stats block.

use bevy::prelude::*;

use crate::constants::ui::{
    UI_BUTTON_FOOD_ACTIVE, UI_BUTTON_NEST_ACTIVE, UI_BUTTON_PAUSE_ACTIVE, UI_EDGE_INSET_PERCENT,
    UI_HUD_GAP, UI_PANEL_PADDING, UI_PAUSED_FONT_SIZE, UI_PAUSED_TEXT, UI_PAUSED_TEXT_COLOR,
    UI_PAUSED_TOP_PERCENT, UI_Z_HUD, UI_Z_PAUSED,
};
use crate::core::sets::Paused;
use crate::editor::{EditorMode, EditorModeKind, toggled_mode};
use crate::simulation::ant::AntPopulation;
use crate::simulation::colony::{ColonyStats, NestStore};
use crate::simulation::lifecycle::{Brood, BroodStage, Corpse, Queen};
use crate::ui::widgets::{self, ButtonAction, ButtonActiveWhen, ToolButton};

/// Top offset of the colony stats panel as a percentage, just below the
/// controls row and the paused indicator.
const UI_STATS_TOP_PERCENT: f32 = 17.0;
/// Font size of the colony stats panel in logical pixels.
const UI_STATS_FONT_SIZE: f32 = 14.0;
/// Text color of the colony stats panel.
const UI_STATS_TEXT_COLOR: Color = Color::srgba(0.85, 0.85, 0.85, 1.0);
/// Initial text of the colony stats panel, replaced on the first update.
const COLONY_STATS_PLACEHOLDER: &str = "ants -   nest food -\ndelivery -/s   total -\nbrood eggs -   larvae -   pupae -\nqueen -   deaths -   corpses -   refuse -";

/// Marker for the "PAUSED" overlay root.
#[derive(Component)]
pub struct PausedIndicator;

/// Marker for the colony stats text entity.
#[derive(Component)]
pub struct ColonyStatsText;

/// Live brood entity counts per developmental stage.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct BroodCounts {
    eggs: u32,
    larvae: u32,
    pupae: u32,
}

/// Last displayed colony snapshot, quantized to the precision actually shown.
/// The text is only rewritten when one of these values changes, so a stable
/// colony does not dirty the UI text layout every frame.
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColonyStatsCache {
    population: usize,
    store: i32,
    rate_tenths: i32,
    total: i32,
    deaths: u32,
    corpses: u32,
    refuse: u32,
    eggs: u32,
    larvae: u32,
    pupae: u32,
    laying: bool,
}

impl ColonyStatsCache {
    /// Quantize the live values to what the panel displays: whole ants and
    /// food, delivery rate to one decimal per second.
    #[allow(clippy::too_many_arguments)]
    fn capture(
        population: usize,
        store: f32,
        rate: f32,
        total: f32,
        deaths: u32,
        corpses: u32,
        refuse: u32,
        brood: BroodCounts,
        laying: bool,
    ) -> Self {
        Self {
            population,
            store: store.round() as i32,
            rate_tenths: (rate * 10.0).round() as i32,
            total: total.round() as i32,
            deaths,
            corpses,
            refuse,
            eggs: brood.eggs,
            larvae: brood.larvae,
            pupae: brood.pupae,
            laying,
        }
    }

    /// Panel text for this snapshot.
    fn render(&self) -> String {
        format!(
            "ants {}   nest food {}\ndelivery {:.1}/s   total {}\nbrood eggs {}   larvae {}   pupae {}\nqueen {}   deaths {}   corpses {}   refuse {}",
            self.population,
            self.store,
            self.rate_tenths as f32 / 10.0,
            self.total,
            self.eggs,
            self.larvae,
            self.pupae,
            if self.laying { "laying" } else { "idle" },
            self.deaths,
            self.corpses,
            self.refuse,
        )
    }
}

/// Keep the colony stats panel in sync with the food economy, the brood
/// pipeline and the mortality bookkeeping.
///
/// Runs in `Update`; the fixed-step chain only mutates the resources and the
/// entities, so while the colony is paused or stable the cached snapshot
/// matches and the text is left untouched. The corpse and brood counts are
/// direct entity counts (no extra counters to drift). `Queen` is optional so
/// a bare UI-only app (no simulation plugin) still boots with the queen shown
/// as idle.
pub fn update_colony_stats_hud(
    population: Res<AntPopulation>,
    store: Res<NestStore>,
    colony: Res<ColonyStats>,
    queen: Option<Res<Queen>>,
    corpses: Query<&Corpse>,
    brood: Query<&Brood>,
    mut text_query: Query<(&mut Text, &mut ColonyStatsCache), With<ColonyStatsText>>,
) {
    let corpse_count = corpses.iter().count() as u32;
    let mut brood_counts = BroodCounts::default();

    for item in &brood {
        match item.stage {
            BroodStage::Egg => brood_counts.eggs += 1,
            BroodStage::Larva => brood_counts.larvae += 1,
            BroodStage::Pupa => brood_counts.pupae += 1,
        }
    }

    let laying = queen.is_some_and(|queen| queen.is_laying(store.food()));

    for (mut text, mut cache) in &mut text_query {
        let snapshot = ColonyStatsCache::capture(
            population.count(),
            store.food(),
            colony.delivery_ema,
            colony.total_food_delivered,
            colony.deaths,
            corpse_count,
            colony.refuse,
            brood_counts,
            laying,
        );

        if *cache == snapshot {
            continue;
        }

        text.0 = snapshot.render();
        *cache = snapshot;
    }
}

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
            top: Val::Percent(UI_STATS_TOP_PERCENT),
            padding: UiRect::all(Val::Px(UI_PANEL_PADDING)),
            ..default()
        },
        UI_Z_HUD,
    )
    .with_child((
        ColonyStatsText,
        ColonyStatsCache::default(),
        Text::new(COLONY_STATS_PLACEHOLDER),
        TextFont {
            font_size: FontSize::Px(UI_STATS_FONT_SIZE),
            ..default()
        },
        TextColor(UI_STATS_TEXT_COLOR),
        Node::default(),
    ));

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

/// Dispatch HUD button presses to virtual time and the exclusive editor mode.
pub fn handle_button_press(
    buttons: Query<(&Interaction, &ToolButton), Changed<Interaction>>,
    mut mode: ResMut<EditorMode>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match button.action {
            ButtonAction::TogglePause => {
                if virtual_time.is_paused() {
                    virtual_time.unpause();
                } else {
                    virtual_time.pause();
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

/// Show the paused indicator while `Time<Virtual>` is paused and keep the
/// [`Paused`] mirror in sync.
///
/// `Time<Virtual>` is the single source of truth; this is the only writer of
/// [`Paused`], which exists so widget styling can read a plain resource. The
/// indicator compares against the previous frame, so it is only rewritten on
/// change.
pub fn sync_paused_indicator(
    virtual_time: Res<Time<Virtual>>,
    mut paused: ResMut<Paused>,
    mut was_paused: Local<Option<bool>>,
    indicator: Single<&mut Visibility, With<PausedIndicator>>,
) {
    let is_paused = virtual_time.is_paused();

    if paused.0 != is_paused {
        paused.0 = is_paused;
    }

    if *was_paused == Some(is_paused) {
        return;
    }

    *was_paused = Some(is_paused);
    *indicator.into_inner() = if is_paused {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn stats_cache_quantizes_to_what_is_displayed() {
        let no_brood = BroodCounts::default();
        let base = ColonyStatsCache::capture(100, 150.4, 2.04, 10.4, 3, 2, 1, no_brood, true);
        assert_eq!(
            base,
            ColonyStatsCache::capture(100, 150.1, 2.01, 10.1, 3, 2, 1, no_brood, true)
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(101, 150.4, 2.04, 10.4, 3, 2, 1, no_brood, true)
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.6, 2.04, 10.4, 3, 2, 1, no_brood, true)
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.4, 2.06, 10.4, 3, 2, 1, no_brood, true)
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.4, 2.04, 10.6, 3, 2, 1, no_brood, true)
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.4, 2.04, 10.4, 4, 2, 1, no_brood, true)
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.4, 2.04, 10.4, 3, 3, 1, no_brood, true)
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.4, 2.04, 10.4, 3, 2, 2, no_brood, true)
        );

        let brood = BroodCounts {
            eggs: 4,
            larvae: 5,
            pupae: 6,
        };
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.4, 2.04, 10.4, 3, 2, 1, brood, true),
            "brood counts must be part of the snapshot"
        );
        assert_ne!(
            base,
            ColonyStatsCache::capture(100, 150.4, 2.04, 10.4, 3, 2, 1, no_brood, false),
            "the queen state must be part of the snapshot"
        );
    }

    #[test]
    fn stats_cache_renders_economy_mortality_and_brood() {
        let brood = BroodCounts {
            eggs: 3,
            larvae: 2,
            pupae: 1,
        };
        let text =
            ColonyStatsCache::capture(1234, 812.4, 12.34, 4567.0, 5, 3, 2, brood, true).render();
        assert!(text.contains("ants 1234"), "{text}");
        assert!(text.contains("nest food 812"), "{text}");
        assert!(text.contains("delivery 12.3/s"), "{text}");
        assert!(text.contains("total 4567"), "{text}");
        assert!(text.contains("deaths 5"), "{text}");
        assert!(text.contains("corpses 3"), "{text}");
        assert!(text.contains("refuse 2"), "{text}");
        assert!(text.contains("brood eggs 3   larvae 2   pupae 1"), "{text}");
        assert!(text.contains("queen laying"), "{text}");

        let idle =
            ColonyStatsCache::capture(1, 0.0, 0.0, 0.0, 0, 0, 0, BroodCounts::default(), false)
                .render();
        assert!(idle.contains("queen idle"), "{idle}");
    }

    #[test]
    fn colony_stats_text_only_rewrites_when_displayed_values_change() {
        let mut world = World::new();
        world.init_resource::<AntPopulation>();
        world.init_resource::<NestStore>();
        world.init_resource::<ColonyStats>();
        world.init_resource::<Queen>();
        world.run_system_once(setup_hud).unwrap();

        world.resource_mut::<AntPopulation>().add(1234);
        world.insert_resource(NestStore::with_food(812.4));
        {
            let mut colony = world.resource_mut::<ColonyStats>();
            colony.delivery_ema = 12.34;
            colony.total_food_delivered = 4567.0;
        }
        world.spawn((Corpse { ttl: 10.0 }, Transform::from_xyz(0.0, 0.0, 0.0)));
        world.spawn((Corpse { ttl: 10.0 }, Transform::from_xyz(1.0, 0.0, 0.0)));
        for (index, stage) in [BroodStage::Egg, BroodStage::Larva, BroodStage::Pupa]
            .into_iter()
            .enumerate()
        {
            world.spawn((
                Brood {
                    stage,
                    timer: 0.0,
                    spawn_index: index as u64,
                },
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
        }

        world.run_system_once(update_colony_stats_hud).unwrap();

        let mut query = world.query_filtered::<Entity, With<ColonyStatsText>>();
        let entity = query.single(&world).expect("stats text entity");
        let rendered = world.get::<Text>(entity).unwrap().0.clone();
        assert!(
            rendered.contains("1234") && rendered.contains("812"),
            "{rendered}"
        );
        assert!(
            rendered.contains("12.3") && rendered.contains("4567"),
            "{rendered}"
        );
        assert!(
            rendered.contains("deaths 0") && rendered.contains("corpses 2"),
            "{rendered}"
        );
        assert!(
            rendered.contains("brood eggs 1   larvae 1   pupae 1"),
            "{rendered}"
        );
        assert!(rendered.contains("queen laying"), "{rendered}");

        // A sub-display change must not touch the text.
        let before = world
            .entity(entity)
            .get_change_ticks::<Text>()
            .unwrap()
            .changed;
        world.insert_resource(NestStore::with_food(812.1));
        world.resource_mut::<ColonyStats>().delivery_ema = 12.31;
        world.run_system_once(update_colony_stats_hud).unwrap();
        assert_eq!(
            before,
            world
                .entity(entity)
                .get_change_ticks::<Text>()
                .unwrap()
                .changed
        );

        // A displayed change does.
        world.insert_resource(NestStore::with_food(700.0));
        world.resource_mut::<ColonyStats>().record_death();
        world.run_system_once(update_colony_stats_hud).unwrap();
        assert_ne!(
            before,
            world
                .entity(entity)
                .get_change_ticks::<Text>()
                .unwrap()
                .changed
        );
        let rendered = world.get::<Text>(entity).unwrap().0.clone();
        assert!(rendered.contains("700"), "{rendered}");
        assert!(rendered.contains("deaths 1"), "{rendered}");
    }

    #[test]
    fn setup_hud_keeps_the_paused_indicator_and_adds_the_stats_panel() {
        let mut world = World::new();
        world.run_system_once(setup_hud).unwrap();

        let mut paused = world.query_filtered::<&Visibility, With<PausedIndicator>>();
        assert_eq!(
            *paused.single(&world).expect("paused indicator"),
            Visibility::Hidden
        );

        let mut stats = world.query_filtered::<Entity, With<ColonyStatsText>>();
        assert!(stats.single(&world).is_ok());
    }

    #[test]
    fn pause_button_toggles_virtual_time_and_the_mirror() {
        let mut world = World::new();
        world.init_resource::<Paused>();
        world.init_resource::<Time<Virtual>>();
        world.init_resource::<EditorMode>();
        world.run_system_once(setup_hud).unwrap();

        let pause_button = {
            let mut query = world.query::<(Entity, &ToolButton)>();
            query
                .iter(&world)
                .find(|(_, button)| button.action == ButtonAction::TogglePause)
                .map(|(entity, _)| entity)
                .expect("pause button")
        };

        let press = |world: &mut World, interaction: Interaction| {
            *world
                .entity_mut(pause_button)
                .get_mut::<Interaction>()
                .unwrap() = interaction;
        };

        press(&mut world, Interaction::Pressed);
        world.run_system_once(handle_button_press).unwrap();
        assert!(world.resource::<Time<Virtual>>().is_paused());

        world.run_system_once(sync_paused_indicator).unwrap();
        assert!(
            world.resource::<Paused>().0,
            "the mirror follows virtual time"
        );

        // Releasing is not a press; a second press resumes.
        press(&mut world, Interaction::None);
        world.run_system_once(handle_button_press).unwrap();
        assert!(world.resource::<Time<Virtual>>().is_paused());

        press(&mut world, Interaction::Pressed);
        world.run_system_once(handle_button_press).unwrap();
        assert!(!world.resource::<Time<Virtual>>().is_paused());

        world.run_system_once(sync_paused_indicator).unwrap();
        assert!(!world.resource::<Paused>().0);
    }
}
