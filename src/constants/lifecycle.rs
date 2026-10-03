//! Colony lifecycle tuning constants: mortality, corpses, necrophoresis, the
//! post-delivery rest budget and the staged brood pipeline (F8).

use crate::constants::ant::{ANT_SPEED, CARRY_SPEED_FACTOR};

/// Background mortality hazard per second for ants outside the nest:
/// `P_die = 1 - exp(-MORTALITY_HAZARD * dt)`.
///
/// ~0.1 %/s, i.e. ~8.6 % over a 90 s median life: predation, desiccation and
/// getting lost are visible without dominating the age-based lifetime. The
/// draw comes from the per-ant [`crate::simulation::ant::AntRng`], so the
/// hazard is fully deterministic.
pub const MORTALITY_HAZARD: f32 = 1e-3;

/// Seconds a corpse stays in the world before it decays and despawns.
pub const CORPSE_TTL: f32 = 120.0;
/// Center-to-center distance at which a foraging ant picks up a corpse.
pub const CORPSE_PICKUP_RADIUS: f32 = 3.0;
/// Distance from the refuse point at which a carried corpse is dropped.
pub const CORPSE_DROP_RADIUS: f32 = 5.0;
/// Walking speed while dragging a corpse (the same penalty as a full crop).
pub const CORPSE_CARRY_SPEED: f32 = ANT_SPEED * CARRY_SPEED_FACTOR;
/// Draw order of corpse sprites: just under the live ants.
pub const CORPSE_Z: f32 = crate::core::layers::Z_ANT - 0.01;

/// Probability that a forager rests after a dropoff.
pub const REST_AFTER: f32 = 0.35;
/// Seconds of rest after a dropoff. The navigation stream's `move_ants` skips
/// ants whose `rest_timer` is positive.
pub const REST_DURATION: f32 = 4.0;

// --- Staged brood pipeline (F8) ----------------------------------------------
//
// Growth is brood-limited: the queen lays eggs, each egg hatches into a larva,
// each fed larva grows into a pupa and the pupa ecloses into a callow worker.
// The stage durations are the developmental lag the old direct-spawn
// recruitment did not have. A nursing ant within [`BROOD_TEND_RADIUS`] of a
// brood item speeds it up by [`BROOD_TEND_BONUS`] (feeding/thermoregulation),
// which is what makes the nursing phase a real job.
//
// Food costs live in `constants/colony.rs` next to the store they draw from.

/// Seconds an egg takes to hatch into a larva.
pub const EGG_DURATION: f32 = 3.0;
/// Seconds a fed larva takes to grow into a pupa. Development is paid per
/// second (`LARVA_FOOD_PER_SEC`); a larva whose payment the store cannot cover
/// stalls until food returns.
pub const LARVA_DURATION: f32 = 6.0;
/// Seconds a pupa takes to eclose into a callow worker. Pupae need no food.
pub const PUPA_DURATION: f32 = 5.0;

/// Extra development speed granted by tending: a tended brood item advances
/// at `1 + BROOD_TEND_BONUS` times the base rate. Presence-based, so extra
/// nurses do not stack.
pub const BROOD_TEND_BONUS: f32 = 0.5;
/// Distance within which a nursing ant tends a brood item, in world units.
/// Must stay `<= DENSITY_CELL_SIZE` so the tending index only has to scan the
/// 3x3 cell neighborhood of a brood item.
pub const BROOD_TEND_RADIUS: f32 = 8.0;

/// Compile-time guard for the 3x3 tending-index invariant above.
const _: () = assert!(BROOD_TEND_RADIUS <= crate::constants::world::DENSITY_CELL_SIZE);

/// Radius of the brood area around the nest centre where the queen lays eggs
/// and the founding brood sits.
pub const BROOD_AREA_RADIUS: f32 = crate::constants::world::NEST_RADIUS * 0.5;
/// Golden-angle increment (radians) of the deterministic brood placement
/// spiral. No RNG is involved, so laying and founding are reproducible.
pub const BROOD_SPIRAL_ANGLE: f32 = 2.399_963_1;
/// Radial fraction step of the deterministic brood placement spiral.
pub const BROOD_SPIRAL_STEP: f32 = 0.618_034;

/// Callow nurses the colony starts with (the founding workforce).
///
/// The old direct-spawn ramp delivered the first `ANT_BATCH_SIZE` (100) adults
/// within 0.05 s and filled the trail harness cap (800) within 0.4 s. The
/// founding cohort is deliberately half of that old cap: it keeps discovery
/// fast enough for the 45 s trail gate (the 450 u gate measured ~36 deliveries
/// and a 0.78 corridor at 45 s against thresholds of 10 and 0.05), while the
/// rest of the colony grows out of brood (the queen's ~4 eggs/s close the gap
/// within the first minute of a session).
pub const INITIAL_NURSES: usize = 400;
/// Eggs the queen has already laid at startup (the founding brood batch).
pub const INITIAL_BROOD: usize = 60;

/// Draw order of brood sprites: under the live ants, above the corpses.
pub const BROOD_Z: f32 = crate::core::layers::Z_ANT - 0.005;
