//! Colony lifecycle: brood, queen and developmental stages.
//!
//! The colony stream fills this module (brood pipeline); [`register`] is the
//! wiring hook its systems grow into.

use bevy::prelude::*;

/// Wiring hook for the colony lifecycle stream: it registers brood/queen
/// systems here. Empty until that stream lands.
pub fn register(_app: &mut App) {}
