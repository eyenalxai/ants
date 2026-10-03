//! Environment tuning constants (day-night cycle, activity and nest geometry).

/// Length of one full day-night cycle in seconds of simulated time.
///
/// Ten minutes at the 64 Hz fixed step, so a typical session lives through
/// roughly one day. The cycle starts at peak activity, which keeps the short
/// regression runs (45 s trail check, 15 s determinism replay) at essentially
/// full activity while a longer session still sees dusk and dawn.
pub const DAY_LENGTH: f32 = 600.0;

/// Lowest activity factor of the day-night cycle, reached at midnight.
///
/// Foraging never fully stops: the floor keeps the colony alive (and the
/// simulation observable) during the inactive half of the cycle.
pub const ACTIVITY_MIN: f32 = 0.3;

/// Radius of the nest entrance mouth in world units.
///
/// Written into
/// [`crate::simulation::nest::NestGeometry::entrance_radius`] once a nest
/// entity exists; the colony stream reads it to size the dropoff/spawn mouth.
pub const ENTRANCE_RADIUS: f32 = 6.0;
